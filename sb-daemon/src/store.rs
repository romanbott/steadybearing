use std::collections::VecDeque;
use std::sync::{Arc, Mutex};

use sb_protocol::types::{MetricSample, TIER1_CAPACITY};

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
}

#[derive(Clone)]
pub struct TelemetryStore {
    pub tier1: Arc<RingBufferTier>,
}

impl TelemetryStore {
    pub fn new() -> Self {
        TelemetryStore {
            tier1: Arc::new(RingBufferTier::new(TIER1_CAPACITY)),
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
}
