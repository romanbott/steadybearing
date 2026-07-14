use anyhow::{Context, Result};
use std::path::PathBuf;

#[derive(Debug, Clone)]
pub struct HwmonLocator {
    pub coretemp_path: PathBuf,
    pub pwm_path: PathBuf,
    pub pwm_enable_path: PathBuf,
    pub fan_input_path: PathBuf,
}

impl HwmonLocator {
    pub fn discover() -> Result<Self> {
        let hwmon_base = std::path::Path::new("/sys/class/hwmon");

        let mut coretemp_path = None;
        let mut pwm_path = None;
        let mut pwm_enable_path = None;
        let mut fan_input_path = None;

        let entries = std::fs::read_dir(hwmon_base)
            .with_context(|| "reading /sys/class/hwmon")?;

        for entry in entries {
            let entry = entry?;
            let name_path = entry.path().join("name");
            let driver_name = std::fs::read_to_string(&name_path)
                .unwrap_or_default()
                .trim()
                .to_string();

            if driver_name == "coretemp" {
                coretemp_path = Some(entry.path().join("temp1_input"));
            }

            if entry.path().join("pwm2").exists() {
                pwm_path = Some(entry.path().join("pwm2"));
                pwm_enable_path = Some(entry.path().join("pwm2_enable"));

                for candidate in &["fan2_input", "fan1_input"] {
                    let fan_path = entry.path().join(candidate);
                    if fan_path.exists() {
                        fan_input_path = Some(fan_path);
                        break;
                    }
                }
            }
        }

        Ok(HwmonLocator {
            coretemp_path: coretemp_path.context("coretemp hwmon not found")?,
            pwm_path: pwm_path.context("pwm2 hwmon not found (expected N95 EC driver)")?,
            pwm_enable_path: pwm_enable_path
                .context("pwm2_enable hwmon not found")?,
            fan_input_path: fan_input_path
                .context("fan1_input / fan2_input hwmon not found")?,
        })
    }
}
