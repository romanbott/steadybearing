use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};

use anyhow::{Context, Result};
use sb_protocol::types::MetricSample;

use crate::hwmon::HwmonLocator;

pub struct Sampler {
    hwmon: HwmonLocator,
    proc_stat_path: PathBuf,
    prev_idle: AtomicU64,
    prev_total: AtomicU64,
}

impl Sampler {
    pub fn new(hwmon: HwmonLocator) -> Self {
        Self::with_proc_path(hwmon, proc_stat_path())
    }

    /// Construct a sampler that reads CPU counters from an explicit path.
    /// Used by tests and by the `test-hwmon` harness build.
    pub(crate) fn with_proc_path(hwmon: HwmonLocator, proc_stat_path: PathBuf) -> Self {
        Sampler {
            hwmon,
            proc_stat_path,
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
        let stat = std::fs::read_to_string(&self.proc_stat_path)
            .with_context(|| format!("reading {}", self.proc_stat_path.display()))?;
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
        match raw.trim().parse() {
            Ok(rpm) => Ok(rpm),
            Err(err) => {
                tracing::debug!(
                    ?err,
                    path = %self.hwmon.fan_input_path.display(),
                    "malformed fan rpm"
                );
                Ok(0)
            }
        }
    }

    fn read_current_pwm(&self) -> Result<u8> {
        let raw = std::fs::read_to_string(&self.hwmon.pwm_path)
            .with_context(|| format!("reading {}", self.hwmon.pwm_path.display()))?;
        match raw.trim().parse() {
            Ok(pwm) => Ok(pwm),
            Err(err) => {
                tracing::debug!(
                    ?err,
                    path = %self.hwmon.pwm_path.display(),
                    "malformed pwm value"
                );
                Ok(0)
            }
        }
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

/// `/proc/stat` path used by the real sampler. The environment override only
/// exists in builds with the `test-hwmon` feature.
fn proc_stat_path() -> PathBuf {
    #[cfg(feature = "test-hwmon")]
    if let Some(path) = std::env::var_os("SB_PROC_STAT") {
        return PathBuf::from(path);
    }
    PathBuf::from("/proc/stat")
}

#[cfg(test)]
mod tests {
    use super::{cpu_usage_percent, Sampler};
    use crate::hwmon::HwmonLocator;
    use std::path::{Path, PathBuf};

    fn fixture(name: &str) -> PathBuf {
        let base = std::env::temp_dir().join(format!(
            "sb-sampler-test-{}-{name}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&base);
        std::fs::create_dir_all(&base).unwrap();
        base
    }

    fn fixture_locator(base: &Path) -> HwmonLocator {
        HwmonLocator {
            coretemp_path: base.join("temp1_input"),
            pwm_path: base.join("pwm2"),
            pwm_enable_path: base.join("pwm2_enable"),
            fan_input_path: base.join("fan1_input"),
        }
    }

    #[test]
    fn samples_fake_hwmon_and_cpu_counters() {
        let base = fixture("sample");
        let locator = fixture_locator(&base);
        let proc_stat = base.join("proc_stat");

        std::fs::write(&locator.coretemp_path, "45000\n").unwrap();
        std::fs::write(&locator.fan_input_path, "2000\n").unwrap();
        std::fs::write(&locator.pwm_path, "110\n").unwrap();
        std::fs::write(&locator.pwm_enable_path, "2\n").unwrap();
        std::fs::write(&proc_stat, "cpu  100 0 100 800 0 0 0 0 0 0\n").unwrap();

        let sampler = Sampler::with_proc_path(locator, proc_stat.clone());

        let first = sampler.sample().unwrap();
        assert_eq!(first.cpu_temp, 45.0);
        assert_eq!(first.cpu_usage, 0.0);
        assert_eq!(first.fan_rpm, 2000);
        assert_eq!(first.pwm_value, 110);

        // 200 ticks elapsed, 50 of them idle -> 75% busy.
        std::fs::write(&proc_stat, "cpu  200 0 150 850 0 0 0 0 0 0\n").unwrap();
        let second = sampler.sample().unwrap();
        assert!((second.cpu_usage - 75.0).abs() < 1e-6);

        let _ = std::fs::remove_dir_all(&base);
    }

    #[test]
    fn malformed_fan_and_pwm_values_read_as_zero() {
        let base = fixture("malformed");
        let locator = fixture_locator(&base);
        let proc_stat = base.join("proc_stat");

        std::fs::write(&locator.coretemp_path, "45000\n").unwrap();
        std::fs::write(&locator.fan_input_path, "not-a-number\n").unwrap();
        std::fs::write(&locator.pwm_path, "nope\n").unwrap();
        std::fs::write(&locator.pwm_enable_path, "2\n").unwrap();
        std::fs::write(&proc_stat, "cpu  100 0 100 800 0 0 0 0 0 0\n").unwrap();

        let sampler = Sampler::with_proc_path(locator, proc_stat);
        let sample = sampler.sample().unwrap();
        assert_eq!(sample.fan_rpm, 0);
        assert_eq!(sample.pwm_value, 0);

        let _ = std::fs::remove_dir_all(&base);
    }

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
