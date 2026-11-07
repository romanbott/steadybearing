use serde::{Deserialize, Serialize};

#[repr(C)]
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct MetricSample {
    pub timestamp: i64,
    pub cpu_temp: f32,
    pub cpu_usage: f32,
    pub fan_rpm: u16,
    pub pwm_value: u8,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HistoricalRow {
    pub timestamp: i64,
    pub temp_min: f32,
    pub temp_max: f32,
    pub temp_mean: f32,
    pub usage_min: f32,
    pub usage_max: f32,
    pub usage_mean: f32,
    pub rpm_mean: f32,
    pub pwm_mean: f32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Snapshot {
    pub tier1: Vec<MetricSample>,
    pub tier2: Vec<MetricSample>,
    pub tier3: Vec<MetricSample>,
    pub tier4: Vec<MetricSample>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum HistoricalResolution {
    Minute,
    Hourly,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum IpcRequest {
    SnapshotAll,
    StreamLive,
    Historical { resolution: HistoricalResolution, count: usize },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum IpcResponse {
    Snapshot(Snapshot),
    LiveTick(MetricSample),
    Historical(Vec<HistoricalRow>),
    Error(String),
}

pub const TIER1_CAPACITY: usize = 3600;
pub const TIER2_CAPACITY: usize = 8640;
pub const TIER3_CAPACITY: usize = 4320;
pub const TIER4_CAPACITY: usize = 2016;

pub const DEFAULT_SOCKET_PATH: &str = "/run/sb/sb.sock";
pub const DEFAULT_DB_PATH: &str = "/var/log/sb/telemetry.db";
pub const DEFAULT_DB_ARCHIVE_PATH: &str = "/var/log/sb/archive.db";

pub const PWM_BASELINE: u8 = 110;

pub const FAN_MIN_RPM: u16 = 1200;
pub const FAN_MAX_RPM: u16 = 3000;
