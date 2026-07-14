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
pub struct Snapshot {
    pub tier1: Vec<MetricSample>,
    pub tier2: Vec<MetricSample>,
    pub tier3: Vec<MetricSample>,
    pub tier4: Vec<MetricSample>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum IpcRequest {
    SnapshotAll,
    StreamLive,
    Historical { bucket_secs: u32, count: usize },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum IpcResponse {
    Snapshot(Snapshot),
    LiveTick(MetricSample),
    Historical(Vec<MetricSample>),
}

pub const TIER1_CAPACITY: usize = 3600;
pub const TIER2_CAPACITY: usize = 8640;
pub const TIER3_CAPACITY: usize = 4320;
pub const TIER4_CAPACITY: usize = 2016;

pub const DEFAULT_SOCKET_PATH: &str = "/tmp/sb.sock";
pub const DEFAULT_DB_PATH: &str = "/var/log/sb/telemetry.db";

pub const PWM_BASELINE: u8 = 110;
pub const PWM_RAMP_START: f32 = 55.0;
pub const PWM_RAMP_SLOPE: f32 = 2.5;
pub const PWM_FULL_SPEED: f32 = 85.0;

pub const FAN_MIN_RPM: u16 = 1200;
pub const FAN_MAX_RPM: u16 = 3000;
