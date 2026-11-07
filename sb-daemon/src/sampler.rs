use std::sync::atomic::{AtomicU64, Ordering};

use anyhow::{Context, Result};
use sb_protocol::types::MetricSample;

use crate::hwmon::HwmonLocator;

pub struct Sampler {
    hwmon: HwmonLocator,
    prev_idle: AtomicU64,
    prev_total: AtomicU64,
}

impl Sampler {
    pub fn new(hwmon: HwmonLocator) -> Self {
        Sampler {
            hwmon,
            prev_idle: AtomicU64::new(0),
            prev_total: AtomicU64::new(0),
        }
    }

    pub fn sample(&self) -> Result<MetricSample> {
        Ok(MetricSample {
            timestamp: std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs() as i64,
            cpu_temp: self.read_cpu_temp()?,
            cpu_usage: self.read_cpu_usage()?,
            fan_rpm: self.read_fan_rpm()?,
            pwm_value: self.read_current_pwm()?,
        })
    }

    fn read_cpu_temp(&self) -> Result<f32> {
        let raw = std::fs::read_to_string(&self.hwmon.coretemp_path)
            .with_context(|| format!("reading {}", self.hwmon.coretemp_path.display()))?;
        let millicelsius: f32 = raw.trim().parse()?;
        Ok(millicelsius / 1000.0)
    }

    fn read_cpu_usage(&self) -> Result<f32> {
        let stat = std::fs::read_to_string("/proc/stat")
            .with_context(|| "reading /proc/stat")?;
        let cpu_line = stat.lines().find(|l| l.starts_with("cpu ")).unwrap_or("");
        let fields: Vec<u64> = cpu_line
            .split_whitespace()
            .skip(1)
            .filter_map(|s| s.parse().ok())
            .collect();

        if fields.len() < 4 {
            return Ok(0.0);
        }

        let idle = fields[3];
        let total: u64 = fields.iter().take(8).sum();

        let prev_idle = self.prev_idle.load(Ordering::Relaxed);
        let prev_total = self.prev_total.load(Ordering::Relaxed);

        let usage = cpu_usage_percent(prev_idle, prev_total, idle, total);

        self.prev_idle.store(idle, Ordering::Relaxed);
        self.prev_total.store(total, Ordering::Relaxed);

        Ok(usage)
    }

    fn read_fan_rpm(&self) -> Result<u16> {
        let raw = std::fs::read_to_string(&self.hwmon.fan_input_path)
            .with_context(|| format!("reading {}", self.hwmon.fan_input_path.display()))?;
        Ok(raw.trim().parse().unwrap_or(0))
    }

    fn read_current_pwm(&self) -> Result<u8> {
        let raw = std::fs::read_to_string(&self.hwmon.pwm_path)
            .with_context(|| format!("reading {}", self.hwmon.pwm_path.display()))?;
        Ok(raw.trim().parse().unwrap_or(0))
    }
}

/// Share of CPU time spent busy between two `/proc/stat` snapshots.
///
/// Deltas are saturating because the aggregate counters are not strictly
/// monotonic (`iowait` in particular can move backwards).
fn cpu_usage_percent(prev_idle: u64, prev_total: u64, idle: u64, total: u64) -> f32 {
    if prev_total == 0 || total <= prev_total {
        return 0.0;
    }

    let total_delta = total - prev_total;
    let idle_delta = idle.saturating_sub(prev_idle);
    let busy_delta = total_delta.saturating_sub(idle_delta);

    100.0 * busy_delta as f32 / total_delta as f32
}

#[cfg(test)]
mod tests {
    use super::cpu_usage_percent;

    #[test]
    fn first_sample_is_zero() {
        assert_eq!(cpu_usage_percent(0, 0, 100, 200), 0.0);
    }

    #[test]
    fn uses_the_busy_share_of_the_delta() {
        // 100 ticks elapsed, 50 of them idle -> 50% busy.
        assert!((cpu_usage_percent(100, 200, 150, 300) - 50.0).abs() < 1e-6);
    }

    #[test]
    fn regressing_counters_do_not_underflow() {
        let usage = cpu_usage_percent(1000, 2000, 900, 2100);
        assert!((0.0..=100.0).contains(&usage));
    }

    #[test]
    fn stalled_total_is_zero() {
        assert_eq!(cpu_usage_percent(10, 100, 10, 100), 0.0);
    }
}
