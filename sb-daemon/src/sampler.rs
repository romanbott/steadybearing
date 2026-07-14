use anyhow::{Context, Result};
use sb_protocol::types::MetricSample;

use crate::hwmon::HwmonLocator;

pub struct Sampler {
    hwmon: HwmonLocator,
}

impl Sampler {
    pub fn new(hwmon: HwmonLocator) -> Self {
        Sampler { hwmon }
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

        static mut PREV_IDLE: u64 = 0;
        static mut PREV_TOTAL: u64 = 0;

        let prev_idle = unsafe { PREV_IDLE };
        let prev_total = unsafe { PREV_TOTAL };

        let usage = if prev_total > 0 && total > prev_total {
            let total_delta = total - prev_total;
            let idle_delta = idle - prev_idle;
            100.0 * (total_delta - idle_delta) as f32 / total_delta as f32
        } else {
            0.0
        };

        unsafe {
            PREV_IDLE = idle;
            PREV_TOTAL = total;
        }

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
