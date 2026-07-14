use std::sync::{Arc, Mutex};

use sb_protocol::types::{MetricSample, TIER1_CAPACITY, TIER2_CAPACITY, TIER3_CAPACITY, TIER4_CAPACITY};

pub struct RingBufferTier {
    buffer: Mutex<Vec<MetricSample>>,
    capacity: usize,
}

impl RingBufferTier {
    pub fn new(capacity: usize) -> Self {
        RingBufferTier {
            buffer: Mutex::new(Vec::with_capacity(capacity)),
            capacity,
        }
    }

    pub fn push(&self, sample: MetricSample) {
        let mut buf = self.buffer.lock().unwrap();
        if buf.len() >= self.capacity {
            buf.remove(0);
        }
        buf.push(sample);
    }

    pub fn snapshot(&self) -> Vec<MetricSample> {
        let buf = self.buffer.lock().unwrap();
        buf.clone()
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
