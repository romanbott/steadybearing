use std::collections::BTreeMap;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use tokio::task::JoinHandle;

use crate::config::Config;
use crate::store::TelemetryStore;

struct RollupRow {
    ts: i64,
    temp_min: f32,
    temp_max: f32,
    temp_mean: f32,
    usage_min: f32,
    usage_max: f32,
    usage_mean: f32,
    rpm_mean: f32,
    pwm_mean: f32,
}

const SCHEMA: &str = "CREATE TABLE IF NOT EXISTS rollups (
    bucket_ts     INTEGER NOT NULL PRIMARY KEY,
    temp_min      REAL NOT NULL,
    temp_max      REAL NOT NULL,
    temp_mean     REAL NOT NULL,
    usage_min     REAL NOT NULL,
    usage_max     REAL NOT NULL,
    usage_mean    REAL NOT NULL,
    rpm_mean      REAL NOT NULL,
    pwm_mean      REAL NOT NULL
);";

pub fn spawn(
    store: TelemetryStore,
    config: Config,
) -> anyhow::Result<JoinHandle<anyhow::Result<()>>> {
    create_dir_if_needed(&config.db_path)?;
    create_dir_if_needed(&config.db_archive_path)?;
    init_db(&config.db_path)?;
    init_db(&config.db_archive_path)?;

    Ok(tokio::spawn(async move {
        let flush_secs = config.db_flush_secs.max(1.0);
        let mut interval = tokio::time::interval(Duration::from_secs_f32(flush_secs));
        interval.tick().await;

        loop {
            interval.tick().await;

            let snapshot = store.tier1.snapshot();
            if snapshot.is_empty() {
                continue;
            }

            let now = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs();

            let minute_rows = bucket_by(&snapshot, 60);

            let telemetry_path = config.db_path.clone();
            let retention_cutoff = now - (config.db_retention_days as u64 * 86400);

            tokio::task::spawn_blocking(move || {
                let conn = rusqlite::Connection::open(&telemetry_path)?;
                for row in &minute_rows {
                    insert_row(&conn, row)?;
                }
                conn.execute(
                    "DELETE FROM rollups WHERE bucket_ts < ?1",
                    rusqlite::params![retention_cutoff as i64],
                )?;
                Ok::<_, anyhow::Error>(())
            })
            .await??;

            let telemetry_path = config.db_path.clone();
            let archive_path = config.db_archive_path.clone();
            let retention_days = config.db_retention_days;

            tokio::task::spawn_blocking(move || {
                let hour_rows =
                    build_hourly_from_minute_db(&telemetry_path, retention_days);
                let conn = rusqlite::Connection::open(&archive_path)?;
                for row in &hour_rows {
                    insert_row(&conn, row)?;
                }
                Ok::<_, anyhow::Error>(())
            })
            .await??;
        }
    }))
}

fn build_hourly_from_minute_db(
    telemetry_path: &str,
    retention_days: u32,
) -> Vec<RollupRow> {
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    let cutoff = now.saturating_sub(retention_days as u64 * 86400);

    let conn = match rusqlite::Connection::open(telemetry_path) {
        Ok(c) => c,
        Err(_) => return vec![],
    };

    let mut stmt = match conn.prepare(
        "SELECT bucket_ts, temp_min, temp_max, temp_mean,
                usage_min, usage_max, usage_mean, rpm_mean, pwm_mean
         FROM rollups WHERE bucket_ts >= ?1 ORDER BY bucket_ts",
    ) {
        Ok(s) => s,
        Err(_) => return vec![],
    };

    let rows: Vec<(i64, f32, f32, f32, f32, f32, f32, f32, f32)> = stmt
        .query_map(rusqlite::params![cutoff as i64], |row| {
            Ok((
                row.get::<_, i64>(0)?,
                row.get::<_, f64>(1)? as f32,
                row.get::<_, f64>(2)? as f32,
                row.get::<_, f64>(3)? as f32,
                row.get::<_, f64>(4)? as f32,
                row.get::<_, f64>(5)? as f32,
                row.get::<_, f64>(6)? as f32,
                row.get::<_, f64>(7)? as f32,
                row.get::<_, f64>(8)? as f32,
            ))
        })
        .map_or(vec![], |iter| iter.filter_map(|r| r.ok()).collect());

    let mut buckets: BTreeMap<i64, Vec<usize>> = BTreeMap::new();
    for (idx, (ts, ..)) in rows.iter().enumerate() {
        let hour_ts = ts / 3600 * 3600;
        buckets.entry(hour_ts).or_default().push(idx);
    }

    buckets
        .into_iter()
        .map(|(hour_ts, indices)| {
            let temp_min = indices
                .iter()
                .map(|&i| rows[i].1)
                .fold(f32::MAX, f32::min);
            let temp_max = indices
                .iter()
                .map(|&i| rows[i].2)
                .fold(f32::MIN, f32::max);
            let temp_mean: f32 =
                indices.iter().map(|&i| rows[i].3).sum::<f32>() / indices.len() as f32;
            let usage_min = indices
                .iter()
                .map(|&i| rows[i].4)
                .fold(f32::MAX, f32::min);
            let usage_max = indices
                .iter()
                .map(|&i| rows[i].5)
                .fold(f32::MIN, f32::max);
            let usage_mean: f32 =
                indices.iter().map(|&i| rows[i].6).sum::<f32>() / indices.len() as f32;
            let rpm_mean: f32 =
                indices.iter().map(|&i| rows[i].7).sum::<f32>() / indices.len() as f32;
            let pwm_mean: f32 =
                indices.iter().map(|&i| rows[i].8).sum::<f32>() / indices.len() as f32;

            RollupRow {
                ts: hour_ts,
                temp_min,
                temp_max,
                temp_mean,
                usage_min,
                usage_max,
                usage_mean,
                rpm_mean,
                pwm_mean,
            }
        })
        .collect()
}

fn create_dir_if_needed(path: &str) -> anyhow::Result<()> {
    if let Some(parent) = std::path::Path::new(path).parent() {
        std::fs::create_dir_all(parent)?;
    }
    Ok(())
}

fn init_db(path: &str) -> anyhow::Result<()> {
    let conn = rusqlite::Connection::open(path)?;
    conn.execute_batch(SCHEMA)?;
    Ok(())
}

fn bucket_by(
    samples: &[sb_protocol::types::MetricSample],
    interval: i64,
) -> Vec<RollupRow> {
    let mut buckets: BTreeMap<i64, Vec<&sb_protocol::types::MetricSample>> = BTreeMap::new();
    for s in samples {
        let ts = s.timestamp / interval * interval;
        buckets.entry(ts).or_default().push(s);
    }

    buckets
        .into_iter()
        .map(|(ts, group)| {
            let temps: Vec<f32> = group.iter().map(|s| s.cpu_temp).collect();
            let usages: Vec<f32> = group.iter().map(|s| s.cpu_usage).collect();
            let rpms: Vec<u16> = group.iter().map(|s| s.fan_rpm).collect();
            let pwms: Vec<u8> = group.iter().map(|s| s.pwm_value).collect();

            let n = group.len() as f32;
            RollupRow {
                ts,
                temp_min: temps.iter().cloned().fold(f32::MAX, f32::min),
                temp_max: temps.iter().cloned().fold(f32::MIN, f32::max),
                temp_mean: temps.iter().sum::<f32>() / n,
                usage_min: usages.iter().cloned().fold(f32::MAX, f32::min),
                usage_max: usages.iter().cloned().fold(f32::MIN, f32::max),
                usage_mean: usages.iter().sum::<f32>() / n,
                rpm_mean: rpms.iter().map(|&r| r as f32).sum::<f32>() / n,
                pwm_mean: pwms.iter().map(|&p| p as f32).sum::<f32>() / n,
            }
        })
        .collect()
}

fn insert_row(conn: &rusqlite::Connection, row: &RollupRow) -> anyhow::Result<()> {
    conn.execute(
        "INSERT OR REPLACE INTO rollups
         (bucket_ts, temp_min, temp_max, temp_mean, usage_min, usage_max, usage_mean, rpm_mean, pwm_mean)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
        rusqlite::params![
            row.ts, row.temp_min, row.temp_max, row.temp_mean,
            row.usage_min, row.usage_max, row.usage_mean,
            row.rpm_mean, row.pwm_mean,
        ],
    )?;
    Ok(())
}
