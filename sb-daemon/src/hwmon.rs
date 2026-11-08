// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Roman Contreras

use anyhow::{Context, Result};
use std::path::{Path, PathBuf};

#[derive(Debug, Clone)]
pub struct HwmonLocator {
    pub coretemp_path: PathBuf,
    pub pwm_path: PathBuf,
    pub pwm_enable_path: PathBuf,
    pub fan_input_path: PathBuf,
}

impl HwmonLocator {
    pub fn discover() -> Result<Self> {
        Self::discover_in(&hwmon_root())
    }

    fn discover_in(base: &Path) -> Result<Self> {
        let mut entries: Vec<PathBuf> = std::fs::read_dir(base)
            .with_context(|| format!("reading {}", base.display()))?
            .filter_map(|entry| entry.ok())
            .map(|entry| entry.path())
            .collect();
        entries.sort();

        let mut coretemp_path = None;
        let mut pwm_path = None;
        let mut pwm_enable_path = None;
        let mut fan_input_path = None;

        let mut coretemp_count = 0;
        let mut pwm_count = 0;

        for dir in entries {
            let driver_name = std::fs::read_to_string(dir.join("name"))
                .unwrap_or_default()
                .trim()
                .to_string();

            if driver_name == "coretemp" {
                let candidate = dir.join("temp1_input");
                if candidate.exists() {
                    coretemp_count += 1;
                    if coretemp_path.is_none() {
                        coretemp_path = Some(candidate);
                    }
                }
            }

            let pwm = dir.join("pwm2");
            let pwm_enable = dir.join("pwm2_enable");
            if pwm.exists() && pwm_enable.exists() {
                pwm_count += 1;
                if pwm_path.is_none() {
                    pwm_path = Some(pwm);
                    pwm_enable_path = Some(pwm_enable);

                    for candidate in ["fan2_input", "fan1_input"] {
                        let fan_path = dir.join(candidate);
                        if fan_path.exists() {
                            fan_input_path = Some(fan_path);
                            break;
                        }
                    }
                }
            }
        }

        if coretemp_count > 1 {
            tracing::warn!(
                count = coretemp_count,
                "multiple coretemp hwmon devices found; using the first"
            );
        }
        if pwm_count > 1 {
            tracing::warn!(
                count = pwm_count,
                "multiple pwm2 hwmon devices found; using the first"
            );
        }

        Ok(HwmonLocator {
            coretemp_path: coretemp_path
                .context("coretemp hwmon with temp1_input not found")?,
            pwm_path: pwm_path.context("pwm2 hwmon not found (expected N95 EC driver)")?,
            pwm_enable_path: pwm_enable_path
                .context("pwm2_enable hwmon not found")?,
            fan_input_path: fan_input_path
                .context("fan1_input / fan2_input hwmon not found")?,
        })
    }
}

/// Root used by the real discovery. The environment override only exists in
/// builds with the `test-hwmon` feature (used by the fake-sysfs harness).
fn hwmon_root() -> PathBuf {
    #[cfg(feature = "test-hwmon")]
    if let Some(root) = std::env::var_os("SB_HWMON_ROOT") {
        return PathBuf::from(root);
    }
    PathBuf::from("/sys/class/hwmon")
}

#[cfg(test)]
mod tests {
    use super::HwmonLocator;
    use std::path::PathBuf;

    fn fixture(name: &str) -> PathBuf {
        let base = std::env::temp_dir().join(format!(
            "sb-hwmon-test-{}-{name}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&base);
        base
    }

    fn write(path: &PathBuf, contents: &str) {
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, contents).unwrap();
    }

    #[test]
    fn finds_coretemp_and_ec_fan() {
        let base = fixture("finds");
        let core = base.join("hwmon0");
        write(&core.join("name"), "coretemp\n");
        write(&core.join("temp1_input"), "45000\n");

        let ec = base.join("hwmon1");
        write(&ec.join("name"), "n95_ec\n");
        write(&ec.join("pwm2"), "110\n");
        write(&ec.join("pwm2_enable"), "2\n");
        write(&ec.join("fan2_input"), "2000\n");

        let found = HwmonLocator::discover_in(&base).unwrap();
        assert_eq!(found.coretemp_path, core.join("temp1_input"));
        assert_eq!(found.pwm_path, ec.join("pwm2"));
        assert_eq!(found.pwm_enable_path, ec.join("pwm2_enable"));
        assert_eq!(found.fan_input_path, ec.join("fan2_input"));

        let _ = std::fs::remove_dir_all(&base);
    }

    #[test]
    fn ignores_coretemp_without_temp_input() {
        let base = fixture("ignores");
        let broken = base.join("hwmon0");
        write(&broken.join("name"), "coretemp\n");

        let ec = base.join("hwmon1");
        write(&ec.join("name"), "n95_ec\n");
        write(&ec.join("pwm2"), "110\n");
        write(&ec.join("pwm2_enable"), "2\n");
        write(&ec.join("fan1_input"), "2000\n");

        assert!(HwmonLocator::discover_in(&base).is_err());

        let _ = std::fs::remove_dir_all(&base);
    }

    #[test]
    fn picks_the_first_candidate_deterministically() {
        let base = fixture("first");
        for (index, name) in [(0, "coretemp"), (1, "coretemp")] {
            let dir = base.join(format!("hwmon{index}"));
            write(&dir.join("name"), name);
            write(&dir.join("temp1_input"), "45000\n");
        }
        for index in [2, 3] {
            let dir = base.join(format!("hwmon{index}"));
            write(&dir.join("name"), "n95_ec");
            write(&dir.join("pwm2"), "110\n");
            write(&dir.join("pwm2_enable"), "2\n");
            write(&dir.join("fan1_input"), "2000\n");
        }

        let found = HwmonLocator::discover_in(&base).unwrap();
        assert_eq!(found.coretemp_path, base.join("hwmon0/temp1_input"));
        assert_eq!(found.pwm_path, base.join("hwmon2/pwm2"));

        let _ = std::fs::remove_dir_all(&base);
    }
}
