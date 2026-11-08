// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Roman Contreras

mod aux;
mod config;
mod db;
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

    let hwmon = match hwmon::HwmonLocator::discover() {
        Ok(hwmon) => hwmon,
        Err(err) => {
            if config.check {
                eprintln!("sb-daemon --check failed: {err}");
                eprintln!(
                    "Hint: check the it87 driver (docs/it87.md) and run tools/collect-me-mini.sh."
                );
            }
            return Err(err);
        }
    };
    tracing::info!(
        coretemp = ?hwmon.coretemp_path,
        pwm = ?hwmon.pwm_path,
        fan = ?hwmon.fan_input_path,
        "hwmon paths discovered"
    );

    let aux_locator = aux::AuxLocator::discover();
    tracing::info!(
        count = aux_locator.sensors().len(),
        "auxiliary thermal sensors discovered"
    );

    if config.check {
        print_hwmon_check(&hwmon, &aux_locator);
        return Ok(());
    }

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

    let (tx, _rx) = broadcast::channel::<MetricSample>(256);
    let (db_shutdown_tx, db_shutdown_rx) = tokio::sync::watch::channel(false);

    let rt = tokio::runtime::Runtime::new()?;
    rt.block_on(async {
        let store = store::TelemetryStore::new();
        let aux_store = aux::AuxStore::new();
        aux_store.replace_sensors(aux_locator.sensors().to_vec());

        let fan_handle = fan::spawn(
            store.clone(),
            aux_store.clone(),
            hwmon.clone(),
            tx.clone(),
            config.clone(),
        );
        let mut db_handle =
            db::spawn(store.clone(), aux_store.clone(), config.clone(), db_shutdown_rx)?;
        let ipc_handle = ipc::spawn(
            store.clone(),
            config.socket_path.clone(),
            tx.clone(),
            config.clone(),
        )?;

        let mut db_finished = false;
        let exit_reason = tokio::select! {
            r = fan_handle => {
                tracing::error!(?r, "fan task exited");
                "fan"
            }
            r = &mut db_handle => {
                db_finished = true;
                tracing::error!(?r, "db task exited");
                "db"
            }
            r = ipc_handle => {
                tracing::error!(?r, "ipc task exited");
                "ipc"
            }
            () = shutdown.notified() => {
                tracing::info!("shutdown signal received, stopping tasks");
                "shutdown"
            }
        };

        if !db_finished {
            let _ = db_shutdown_tx.send(true);
            match tokio::time::timeout(
                tokio::time::Duration::from_secs(10),
                &mut db_handle,
            )
            .await
            {
                Ok(Ok(Ok(()))) => tracing::info!("final telemetry flush complete"),
                Ok(Ok(Err(err))) => tracing::error!(?err, "final telemetry flush failed"),
                Ok(Err(err)) => tracing::error!(?err, "db task failed during shutdown"),
                Err(_) => tracing::error!("timed out waiting for final telemetry flush"),
            }
        }

        tracing::info!(reason = exit_reason, "sb-daemon stopping");
        if exit_reason == "shutdown" {
            Ok::<(), anyhow::Error>(())
        } else {
            Err(anyhow::anyhow!("{exit_reason} task exited unexpectedly"))
        }
    })
}

fn print_hwmon_check(hwmon: &hwmon::HwmonLocator, aux_locator: &aux::AuxLocator) {
    println!("coretemp:   {}", hwmon.coretemp_path.display());
    println!("pwm:        {}", hwmon.pwm_path.display());
    println!("pwm_enable: {}", hwmon.pwm_enable_path.display());
    println!("fan:        {}", hwmon.fan_input_path.display());
    println!();

    for (label, path) in [
        ("cpu_temp", &hwmon.coretemp_path),
        ("pwm", &hwmon.pwm_path),
        ("pwm_enable", &hwmon.pwm_enable_path),
        ("fan", &hwmon.fan_input_path),
    ] {
        let value = std::fs::read_to_string(path)
            .map(|value| value.trim().to_string())
            .unwrap_or_else(|err| format!("<{err}>"));
        println!("{label:>10}: {value}");
    }

    let writable = std::fs::OpenOptions::new()
        .write(true)
        .open(&hwmon.pwm_path)
        .is_ok();
    println!(
        "pwm writable: {}",
        if writable { "yes" } else { "no (run as root?)" }
    );
    println!();

    println!("auxiliary thermal sensors:");
    if aux_locator.is_empty() {
        println!("  none detected");
    } else {
        for sensor in aux_locator.sensors() {
            let value = match sensor.read_millicelsius() {
                Some(millicelsius) => format!("{:.1} C", millicelsius as f32 / 1000.0),
                None => "<invalid/unavailable>".to_string(),
            };
            println!(
                "  {:<40} {:<12} {}",
                sensor.id,
                sensor.label,
                value
            );
        }
    }

    println!();
    println!("No changes were made.");
}
