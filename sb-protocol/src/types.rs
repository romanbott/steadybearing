// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Roman Contreras

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

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum HistoricalResolution {
    TenSecond,
    Minute,
    TenMinute,
    Hourly,
}

/// Bump this whenever `IpcRequest` or `IpcResponse` changes shape.
pub const PROTOCOL_VERSION: u32 = 1;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum IpcRequest {
    Hello { protocol: u32 },
    SnapshotAll,
    StreamLive,
    Historical { resolution: HistoricalResolution, count: usize },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum IpcResponse {
    Hello { protocol: u32 },
    Snapshot(Vec<MetricSample>),
    LiveTick(MetricSample),
    Historical(Vec<HistoricalRow>),
    Error(String),
}

pub const TIER1_CAPACITY: usize = 3600;

pub const DEFAULT_SOCKET_PATH: &str = "/run/sb/sb.sock";
pub const DEFAULT_DB_PATH: &str = "/var/lib/sb/history.db";

pub const PWM_BASELINE: u8 = 110;

pub const FAN_MIN_RPM: u16 = 1200;
pub const FAN_MAX_RPM: u16 = 3000;
