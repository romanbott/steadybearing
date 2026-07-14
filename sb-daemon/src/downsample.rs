use std::sync::Arc;
use tokio::task::JoinHandle;

use sb_protocol::types::MetricSample;

use crate::store::{RingBufferTier, TelemetryStore};

struct DownsampleConfig {
    source: Arc<RingBufferTier>,
    target: Arc<RingBufferTier>,
    chunk_size: usize,
}

pub fn spawn(store: TelemetryStore) -> JoinHandle<anyhow::Result<()>> {
    tokio::spawn(async move {
        let mut interval = tokio::time::interval(tokio::time::Duration::from_secs(5));
        interval.tick().await;

        let pipelines = vec![
            DownsampleConfig {
                source: store.tier1.clone(),
                target: store.tier2.clone(),
                chunk_size: 10,
            },
            DownsampleConfig {
                source: store.tier2.clone(),
                target: store.tier3.clone(),
                chunk_size: 6,
            },
            DownsampleConfig {
                source: store.tier3.clone(),
                target: store.tier4.clone(),
                chunk_size: 5,
            },
        ];

        loop {
            interval.tick().await;

            for pipeline in &pipelines {
                if pipeline.source.is_full() {
                    let samples = pipeline.source.drain_oldest(pipeline.chunk_size);
                    if samples.is_empty() {
                        continue;
                    }

                    let mean_temp: f32 =
                        samples.iter().map(|s| s.cpu_temp).sum::<f32>() / samples.len() as f32;
                    let mean_usage: f32 =
                        samples.iter().map(|s| s.cpu_usage).sum::<f32>() / samples.len() as f32;
                    let mean_rpm: u16 =
                        (samples.iter().map(|s| s.fan_rpm as u32).sum::<u32>() / samples.len() as u32) as u16;
                    let mean_pwm: u8 =
                        (samples.iter().map(|s| s.pwm_value as u32).sum::<u32>() / samples.len() as u32) as u8;

                    let aggregated = MetricSample {
                        timestamp: samples[samples.len() - 1].timestamp,
                        cpu_temp: mean_temp,
                        cpu_usage: mean_usage,
                        fan_rpm: mean_rpm,
                        pwm_value: mean_pwm,
                    };

                    pipeline.target.push(aggregated);
                }
            }
        }
    })
}
