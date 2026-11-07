mod config;
mod db;
mod downsample;
mod fan;
mod hwmon;
mod ipc;
mod sampler;
mod signal;
mod store;

use anyhow::Result;
use tokio::sync::broadcast;

use sb_protocol::types::MetricSample;

fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info")),
        )
        .init();

    let config = config::Config::load()?;
    tracing::info!(socket = %config.socket_path, "sb-daemon starting");

    let hwmon = hwmon::HwmonLocator::discover()?;
    tracing::info!(
        coretemp = ?hwmon.coretemp_path,
        pwm = ?hwmon.pwm_path,
        fan = ?hwmon.fan_input_path,
        "hwmon paths discovered"
    );

    let prev_enable = std::fs::read_to_string(&hwmon.pwm_enable_path)
        .unwrap_or_else(|_| "unknown".into())
        .trim()
        .to_string();
    println!("pwm2_enable previous value: {prev_enable}");
    std::fs::write(&hwmon.pwm_enable_path, b"1\n")?;

    let (_guard, shutdown) = match signal::SignalGuard::new(hwmon.pwm_enable_path.clone()) {
        Ok(guard) => guard,
        Err(err) => {
            // We already put the fan into manual mode; make sure a failed
            // handler install does not leave it there.
            let _ = signal::restore_auto(&hwmon.pwm_enable_path);
            return Err(err);
        }
    };

    if let Some(parent) = std::path::Path::new(&config.db_path).parent() {
        std::fs::create_dir_all(parent)?;
    }
    if let Some(parent) = std::path::Path::new(&config.db_archive_path).parent() {
        std::fs::create_dir_all(parent)?;
    }

    let (tx, _rx) = broadcast::channel::<MetricSample>(256);

    let rt = tokio::runtime::Runtime::new()?;
    rt.block_on(async {
        let store = store::TelemetryStore::new();

        let ds_handle = downsample::spawn(store.clone());
        let fan_handle = fan::spawn(store.clone(), hwmon.clone(), tx.clone(), config.clone());
        let db_handle = db::spawn(store.clone(), config.clone())?;
        let ipc_handle = ipc::spawn(store.clone(), config.socket_path.clone(), tx.clone(), config.clone());

        tokio::select! {
            r = ds_handle => {
                tracing::error!(?r, "downsample task exited");
            }
            r = fan_handle => {
                tracing::error!(?r, "fan task exited");
            }
            r = db_handle => {
                tracing::error!(?r, "db task exited");
            }
            r = ipc_handle => {
                tracing::error!(?r, "ipc task exited");
            }
            () = shutdown.notified() => {
                tracing::info!("shutdown signal received, stopping tasks");
            }
        };

        Ok::<_, anyhow::Error>(())
    })
}
