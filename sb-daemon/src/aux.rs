// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Roman Contreras

//! Auxiliary thermal sensors for tuning and component monitoring.
//!
//! The main fan loop only needs CPU package temperature. This module discovers
//! additional hwmon sensors (NVMe drives and the IT8613 board sensors) and
//! samples them into a small in-memory buffer. The database task persists
//! them as 10 s rollups; they do not influence fan control yet.

use std::collections::{BTreeMap, VecDeque};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

/// Readings older than this are dropped from the in-memory buffer.
const BUFFER_SECS: i64 = 180;

/// Plausible temperature range in millidegrees Celsius. The unconnected
/// IT8613 `temp3` input reads 127000 on the ME Mini and is filtered here.
const VALID_MIN_MILLI: i64 = -40_000;
const VALID_MAX_MILLI: i64 = 125_000;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AuxSensor {
    /// Stable identifier: `nvme:<serial-or-bdf>:tempN_input` or
    /// `it8613:tempN_input`.
    pub id: String,
    /// `nvme` or `it8613`.
    pub kind: String,
    /// Human label from the driver, e.g. `Composite` or `Sensor 1`.
    pub label: String,
    pub model: Option<String>,
    pub serial: Option<String>,
    /// PCI BDF for NVMe sensors, `it8613` for board sensors.
    pub device: Option<String>,
    pub path: PathBuf,
}

impl AuxSensor {
    /// Read the sensor, returning millidegrees Celsius if the value is plausible.
    pub fn read_millicelsius(&self) -> Option<i64> {
        let raw = std::fs::read_to_string(&self.path).ok()?;
        let value: i64 = raw.trim().parse().ok()?;
        (VALID_MIN_MILLI..=VALID_MAX_MILLI)
            .contains(&value)
            .then_some(value)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AuxReading {
    pub timestamp: i64,
    pub sensor_id: String,
    pub millicelsius: i64,
}

#[derive(Debug, Clone, Default)]
pub struct AuxBatch {
    pub timestamp: i64,
    pub readings: Vec<AuxReading>,
}

#[derive(Debug, Clone, Default)]
pub struct AuxLocator {
    sensors: Vec<AuxSensor>,
}

impl AuxLocator {
    pub fn discover() -> Self {
        Self::discover_in(&crate::hwmon::hwmon_root())
    }

    pub(crate) fn discover_in(base: &Path) -> Self {
        let mut entries: Vec<PathBuf> = match std::fs::read_dir(base) {
            Ok(read_dir) => read_dir
                .filter_map(|entry| entry.ok())
                .map(|entry| entry.path())
                .collect(),
            Err(_) => return Self::default(),
        };
        entries.sort();

        let mut sensors = Vec::new();
        for dir in entries {
            if !dir.is_dir() {
                continue;
            }
            let name = read_trim(&dir.join("name")).unwrap_or_default();
            match name.as_str() {
                "nvme" => discover_nvme(&dir, &mut sensors),
                "it8613" => discover_it8613(&dir, &mut sensors),
                _ => {}
            }
        }

        sensors.sort_by(|a, b| a.id.cmp(&b.id));
        sensors.dedup_by(|a, b| a.id == b.id);
        Self { sensors }
    }

    pub fn sensors(&self) -> &[AuxSensor] {
        &self.sensors
    }

    pub fn is_empty(&self) -> bool {
        self.sensors.is_empty()
    }

    pub fn sample(&self, timestamp: i64) -> AuxBatch {
        let readings = self
            .sensors
            .iter()
            .filter_map(|sensor| {
                sensor.read_millicelsius().map(|millicelsius| AuxReading {
                    timestamp,
                    sensor_id: sensor.id.clone(),
                    millicelsius,
                })
            })
            .collect();
        AuxBatch {
            timestamp,
            readings,
        }
    }
}

/// Thread-safe buffer shared by the fan loop and the database task.
#[derive(Clone, Default)]
pub struct AuxStore {
    inner: Arc<Mutex<AuxStoreInner>>,
}

#[derive(Default)]
struct AuxStoreInner {
    sensors: BTreeMap<String, AuxSensor>,
    readings: VecDeque<AuxReading>,
}

impl AuxStore {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn replace_sensors(&self, sensors: Vec<AuxSensor>) {
        let mut inner = self.inner.lock().unwrap();
        inner.sensors = sensors
            .into_iter()
            .map(|sensor| (sensor.id.clone(), sensor))
            .collect();
    }

    pub fn sensors(&self) -> Vec<AuxSensor> {
        let inner = self.inner.lock().unwrap();
        inner.sensors.values().cloned().collect()
    }

    pub fn push(&self, batch: AuxBatch) {
        let mut inner = self.inner.lock().unwrap();
        for reading in batch.readings {
            inner.readings.push_back(reading);
        }

        let cutoff = batch.timestamp.saturating_sub(BUFFER_SECS);
        while inner
            .readings
            .front()
            .map(|reading| reading.timestamp < cutoff)
            .unwrap_or(false)
        {
            inner.readings.pop_front();
        }
    }

    pub fn snapshot(&self) -> Vec<AuxReading> {
        let inner = self.inner.lock().unwrap();
        inner.readings.iter().cloned().collect()
    }
}

fn discover_nvme(dir: &Path, sensors: &mut Vec<AuxSensor>) {
    let device_path = std::fs::canonicalize(dir.join("device")).ok();
    let device = device_path.as_deref().and_then(pci_bdf);
    let nvme_name = device_path
        .as_deref()
        .and_then(Path::file_name)
        .and_then(|name| name.to_str())
        .map(str::to_owned);

    let model = nvme_name
        .as_deref()
        .and_then(|name| read_trim(&Path::new("/sys/class/nvme").join(name).join("model")));
    let serial = nvme_name
        .as_deref()
        .and_then(|name| read_trim(&Path::new("/sys/class/nvme").join(name).join("serial")));

    let id_base = serial
        .clone()
        .or_else(|| device.clone())
        .or(nvme_name)
        .unwrap_or_else(|| "unknown".into());

    for path in temp_files(dir) {
        let Some(file) = path.file_name().and_then(|name| name.to_str()) else {
            continue;
        };
        let Some(index) = temp_index(file) else {
            continue;
        };
        let label = read_trim(&dir.join(format!("temp{index}_label")))
            .unwrap_or_else(|| format!("temp{index}"));

        sensors.push(AuxSensor {
            id: format!("nvme:{id_base}:{file}"),
            kind: "nvme".into(),
            label,
            model: model.clone(),
            serial: serial.clone(),
            device: device.clone(),
            path,
        });
    }
}

fn discover_it8613(dir: &Path, sensors: &mut Vec<AuxSensor>) {
    for path in temp_files(dir) {
        let Some(file) = path.file_name().and_then(|name| name.to_str()) else {
            continue;
        };
        let Some(index) = temp_index(file) else {
            continue;
        };
        // temp3 is unconnected on the ME Mini and reads 127 C. The board
        // sensors that are useful are temp1 and temp2.
        if index == "3" {
            continue;
        }

        sensors.push(AuxSensor {
            id: format!("it8613:temp{index}_input"),
            kind: "it8613".into(),
            label: format!("board temp{index}"),
            model: None,
            serial: None,
            device: Some("it8613".into()),
            path,
        });
    }
}

fn temp_files(dir: &Path) -> Vec<PathBuf> {
    let mut files: Vec<PathBuf> = match std::fs::read_dir(dir) {
        Ok(read_dir) => read_dir
            .filter_map(|entry| entry.ok())
            .map(|entry| entry.path())
            .filter(|path| {
                path.file_name()
                    .and_then(|name| name.to_str())
                    .map(|name| name.starts_with("temp") && name.ends_with("_input"))
                    .unwrap_or(false)
            })
            .collect(),
        Err(_) => return Vec::new(),
    };
    files.sort();
    files
}

fn temp_index(file: &str) -> Option<&str> {
    file.strip_prefix("temp")?.strip_suffix("_input")
}

fn read_trim(path: &Path) -> Option<String> {
    std::fs::read_to_string(path)
        .ok()
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
}

/// Extract a PCI BDF such as `0000:03:00.0` from a sysfs device path.
fn pci_bdf(path: &Path) -> Option<String> {
    path.components()
        .filter_map(|component| component.as_os_str().to_str())
        .find(|component| {
            component.len() == 12
                && component.as_bytes().get(4) == Some(&b':')
                && component.as_bytes().get(7) == Some(&b':')
                && component.as_bytes().get(10) == Some(&b'.')
        })
        .map(str::to_owned)
}

#[cfg(test)]
mod tests {
    use super::{AuxLocator, AuxStore, AuxBatch, AuxReading};
    use std::path::{Path, PathBuf};

    fn fixture(name: &str) -> PathBuf {
        let base = std::env::temp_dir().join(format!(
            "sb-aux-test-{}-{name}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&base);
        std::fs::create_dir_all(&base).unwrap();
        base
    }

    fn write(path: &Path, contents: &str) {
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, contents).unwrap();
    }

    fn nvme_dir(base: &Path, index: usize) -> PathBuf {
        let dir = base.join(format!("hwmon{index}"));
        write(&dir.join("name"), "nvme\n");
        dir
    }

    #[test]
    fn discovers_nvme_sensors_and_labels() {
        let base = fixture("nvme");
        let dir = nvme_dir(&base, 0);
        write(&dir.join("temp1_input"), "45850\n");
        write(&dir.join("temp1_label"), "Composite\n");
        write(&dir.join("temp2_input"), "58850\n");
        write(&dir.join("temp2_label"), "Sensor 1\n");

        let locator = AuxLocator::discover_in(&base);
        let sensors = locator.sensors();
        assert_eq!(sensors.len(), 2);
        assert_eq!(sensors[0].kind, "nvme");
        assert_eq!(sensors[0].label, "Composite");
        assert_eq!(sensors[1].label, "Sensor 1");
        assert!(sensors[0].id.contains("temp1_input"));

        let batch = locator.sample(100);
        assert_eq!(batch.readings.len(), 2);
        assert_eq!(batch.readings[0].millicelsius, 45850);

        let _ = std::fs::remove_dir_all(&base);
    }

    #[test]
    fn discovers_it8613_board_sensors_but_skips_temp3() {
        let base = fixture("it8613");
        let dir = base.join("hwmon1");
        write(&dir.join("name"), "it8613\n");
        write(&dir.join("temp1_input"), "43000\n");
        write(&dir.join("temp2_input"), "50000\n");
        write(&dir.join("temp3_input"), "127000\n");

        let locator = AuxLocator::discover_in(&base);
        let sensors = locator.sensors();
        assert_eq!(sensors.len(), 2);
        assert_eq!(sensors[0].id, "it8613:temp1_input");
        assert_eq!(sensors[1].id, "it8613:temp2_input");
        assert!(sensors.iter().all(|sensor| sensor.kind == "it8613"));

        let _ = std::fs::remove_dir_all(&base);
    }

    #[test]
    fn rejects_implausible_values() {
        let base = fixture("invalid");
        let dir = nvme_dir(&base, 0);
        write(&dir.join("temp1_input"), "127000\n");

        let locator = AuxLocator::discover_in(&base);
        assert_eq!(locator.sensors().len(), 1);
        assert_eq!(locator.sensors()[0].read_millicelsius(), None);

        let _ = std::fs::remove_dir_all(&base);
    }

    #[test]
    fn store_prunes_old_readings() {
        let store = AuxStore::new();
        store.push(AuxBatch {
            timestamp: 0,
            readings: vec![AuxReading {
                timestamp: 0,
                sensor_id: "a".into(),
                millicelsius: 40_000,
            }],
        });
        store.push(AuxBatch {
            timestamp: 200,
            readings: vec![AuxReading {
                timestamp: 200,
                sensor_id: "a".into(),
                millicelsius: 41_000,
            }],
        });

        let snapshot = store.snapshot();
        assert_eq!(snapshot.len(), 1);
        assert_eq!(snapshot[0].timestamp, 200);
    }
}
