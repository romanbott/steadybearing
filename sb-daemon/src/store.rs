use std::collections::VecDeque;
use std::sync::{Arc, Mutex};

use sb_protocol::types::{MetricSample, TIER1_CAPACITY, TIER2_CAPACITY, TIER3_CAPACITY, TIER4_CAPACITY};

pub struct RingBufferTier {
    buffer: Mutex<VecDeque<MetricSample>>,
    capacity: usize,
}

impl RingBufferTier {
    pub fn new(capacity: usize) -> Self {
        RingBufferTier {
            buffer: Mutex::new(VecDeque::with_capacity(capacity)),
            capacity,
        }
    }

    pub fn push(&self, sample: MetricSample) {
        let mut buf = self.buffer.lock().unwrap();
        if buf.len() >= self.capacity {
            buf.pop_front();
        }
        buf.push_back(sample);
    }

    pub fn snapshot(&self) -> Vec<MetricSample> {
        let buf = self.buffer.lock().unwrap();
        buf.iter().copied().collect()
    }

    /// Copy the most recent `count` samples without cloning the whole buffer.
    pub fn last_n(&self, count: usize) -> Vec<MetricSample> {
        let buf = self.buffer.lock().unwrap();
        let start = buf.len().saturating_sub(count);
        buf.iter().skip(start).copied().collect()
    }

    pub fn len(&self) -> usize {
        self.buffer.lock().unwrap().len()
    }

    pub fn capacity(&self) -> usize {
        self.capacity
    }

    pub fn drain_oldest(&self, count: usize) -> Vec<MetricSample> {
        let mut buf = self.buffer.lock().unwrap();
        let count = count.min(buf.len());
        buf.drain(..count).collect()
    }

    pub fn is_full(&self) -> bool {
        self.buffer.lock().unwrap().len() >= self.capacity
    }
}

#[derive(Clone)]
pub struct TelemetryStore {
    pub tier1: Arc<RingBufferTier>,
    pub tier2: Arc<RingBufferTier>,
    pub tier3: Arc<RingBufferTier>,
    pub tier4: Arc<RingBufferTier>,
}

impl TelemetryStore {
    pub fn new() -> Self {
        TelemetryStore {
            tier1: Arc::new(RingBufferTier::new(TIER1_CAPACITY)),
            tier2: Arc::new(RingBufferTier::new(TIER2_CAPACITY)),
            tier3: Arc::new(RingBufferTier::new(TIER3_CAPACITY)),
            tier4: Arc::new(RingBufferTier::new(TIER4_CAPACITY)),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::RingBufferTier;
    use sb_protocol::types::MetricSample;

    fn sample(timestamp: i64) -> MetricSample {
        MetricSample {
            timestamp,
            cpu_temp: 50.0,
            cpu_usage: 10.0,
            fan_rpm: 2000,
            pwm_value: 128,
        }
    }

    #[test]
    fn full_buffer_drops_the_oldest_sample() {
        let tier = RingBufferTier::new(3);
        for ts in 0..5 {
            tier.push(sample(ts));
        }

        let timestamps: Vec<i64> = tier
            .snapshot()
            .iter()
            .map(|sample| sample.timestamp)
            .collect();
        assert_eq!(timestamps, vec![2, 3, 4]);
        assert_eq!(tier.len(), 3);
        assert_eq!(tier.capacity(), 3);
    }

    #[test]
    fn drain_oldest_returns_samples_in_order() {
        let tier = RingBufferTier::new(5);
        for ts in 0..5 {
            tier.push(sample(ts));
        }

        let drained: Vec<i64> = tier
            .drain_oldest(2)
            .iter()
            .map(|sample| sample.timestamp)
            .collect();
        assert_eq!(drained, vec![0, 1]);
        assert_eq!(tier.len(), 3);
    }

    #[test]
    fn last_n_returns_only_the_tail() {
        let tier = RingBufferTier::new(5);
        for ts in 0..5 {
            tier.push(sample(ts));
        }

        let tail: Vec<i64> = tier
            .last_n(2)
            .iter()
            .map(|sample| sample.timestamp)
            .collect();
        assert_eq!(tail, vec![3, 4]);
        assert_eq!(tier.last_n(10).len(), 5);
    }

    #[test]
    fn drain_oldest_is_bounded_by_the_buffer_length() {
        let tier = RingBufferTier::new(5);
        tier.push(sample(0));

        assert_eq!(tier.drain_oldest(10).len(), 1);
        assert!(!tier.is_full());
    }
}
