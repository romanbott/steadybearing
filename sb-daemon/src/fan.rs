use std::time::Duration;
use anyhow::Context;
use tokio::sync::broadcast;
use tokio::task::JoinHandle;

use crate::config::Config;
use crate::hwmon::HwmonLocator;
use crate::sampler::Sampler;
use crate::store::TelemetryStore;
use sb_protocol::types::MetricSample;

/// Consecutive failed samples tolerated before giving up and restoring
/// automatic fan control.
const MAX_SENSOR_ERRORS: u32 = 5;

enum FanState {
    Stable,
    Cooling { ticks: u64 },
    Idling { ticks: u64 },
    Drifting { ticks: u64 },
    HardMax { started_at: tokio::time::Instant, start_pwm: u8 },
}

pub fn spawn(
    store: TelemetryStore,
    hwmon: HwmonLocator,
    tx: broadcast::Sender<MetricSample>,
    config: Config,
) -> JoinHandle<anyhow::Result<()>> {
    let sampler = Sampler::new(hwmon.clone());
    let verbose = config.verbose;

    tokio::spawn(async move {
        let mut ticker = tokio::time::interval(Duration::from_secs(1));
        ticker.tick().await;
        let mut tick_count: u64 = 0;
        let mut current_pwm: u8 = config.pwm_baseline;
        let mut state = FanState::Stable;
        let mut sensor_errors: u32 = 0;

        loop {
            ticker.tick().await;
            tick_count += 1;

            let sample = match sampler.sample() {
                Ok(sample) => sample,
                Err(err) => {
                    sensor_errors += 1;
                    tracing::warn!(
                        ?err,
                        consecutive = sensor_errors,
                        "sensor read failed"
                    );
                    if sensor_errors >= MAX_SENSOR_ERRORS {
                        return Err(err
                            .context("too many consecutive sensor read failures"));
                    }
                    continue;
                }
            };
            sensor_errors = 0;
            store.tier1.push(sample);
            let _ = tx.send(sample);

            let ma_temp = compute_10s_ma(&store);
            let target_pwm = step_pwm(ma_temp, &mut state, &mut current_pwm, &config);
            apply_pwm(&hwmon, target_pwm)
                .await
                .with_context(|| format!("writing pwm to {}", hwmon.pwm_path.display()))?;

            if verbose && tick_count % 5 == 0 {
                tracing::info!(
                    temp = sample.cpu_temp,
                    ma_temp = ma_temp,
                    usage = sample.cpu_usage,
                    rpm = sample.fan_rpm,
                    pwm = target_pwm,
                    "heartbeat"
                );
            }
        }
    })
}

fn compute_10s_ma(store: &TelemetryStore) -> f32 {
    let snapshot = store.tier1.snapshot();
    if snapshot.len() < 10 {
        return snapshot.iter().map(|s| s.cpu_temp).sum::<f32>() / snapshot.len().max(1) as f32;
    }
    let last_10: Vec<f32> = snapshot
        .iter()
        .rev()
        .take(10)
        .map(|s| s.cpu_temp)
        .collect();
    last_10.iter().sum::<f32>() / 10.0
}

fn step_pwm(ma_temp: f32, state: &mut FanState, current_pwm: &mut u8, config: &Config) -> u8 {
    if ma_temp >= config.fan_hard_max && !matches!(state, FanState::HardMax { .. }) {
        *state = FanState::HardMax {
            started_at: tokio::time::Instant::now(),
            start_pwm: *current_pwm,
        };
    }

    if let FanState::HardMax { started_at, start_pwm } = *state {
        if ma_temp < config.fan_soft_max {
            *state = FanState::Stable;
            return *current_pwm;
        }

        let elapsed = started_at.elapsed().as_secs_f32();
        let ramp_secs = config.fan_hard_ramp_secs.max(0.1);
        let t = (elapsed / ramp_secs).min(1.0);
        let target = start_pwm as f32 + (255.0 - start_pwm as f32) * t;
        *current_pwm = target as u8;
        return *current_pwm;
    }

    if ma_temp > config.fan_soft_max {
        match *state {
            FanState::Cooling { ticks } => {
                let new_ticks = ticks + 1;
                if new_ticks as f32 >= config.fan_dwell_secs {
                    *current_pwm = ((*current_pwm as u16 + config.fan_step as u16).min(255)) as u8;
                    *state = FanState::Cooling { ticks: 0 };
                } else {
                    *state = FanState::Cooling { ticks: new_ticks };
                }
            }
            _ => {
                *state = FanState::Cooling { ticks: 1 };
            }
        }
        return *current_pwm;
    }

    if ma_temp < config.fan_soft_min {
        match *state {
            FanState::Idling { ticks } => {
                let new_ticks = ticks + 1;
                if new_ticks as f32 >= config.fan_dwell_secs {
                    *current_pwm = ((*current_pwm as u16).saturating_sub(config.fan_step as u16) as u8).max(config.pwm_baseline);
                    *state = FanState::Idling { ticks: 0 };
                } else {
                    *state = FanState::Idling { ticks: new_ticks };
                }
            }
            _ => {
                *state = FanState::Idling { ticks: 1 };
            }
        }
        return *current_pwm;
    }

    if ma_temp < config.fan_drift_temp && *current_pwm > config.pwm_baseline {
        match *state {
            FanState::Drifting { ticks } => {
                let new_ticks = ticks + 1;
                if new_ticks as f32 >= config.fan_drift_secs {
                    *current_pwm = ((*current_pwm as u16).saturating_sub(1) as u8)
                        .max(config.pwm_baseline);
                    *state = FanState::Drifting { ticks: 0 };
                } else {
                    *state = FanState::Drifting { ticks: new_ticks };
                }
            }
            _ => {
                *state = FanState::Drifting { ticks: 1 };
            }
        }
        return *current_pwm;
    }

    *state = FanState::Stable;
    *current_pwm
}

async fn apply_pwm(hwmon: &HwmonLocator, value: u8) -> anyhow::Result<()> {
    let pwm_str = format!("{}\n", value);
    tokio::fs::write(&hwmon.pwm_path, pwm_str.as_bytes()).await?;
    Ok(())
}
