use std::collections::BTreeMap;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use tokio::sync::watch;
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

const BUSY_TIMEOUT: Duration = Duration::from_secs(5);

/// Open a database connection configured for concurrent readers and writers.
pub(crate) fn open(path: &str) -> anyhow::Result<rusqlite::Connection> {
    let conn = rusqlite::Connection::open(path)?;
    conn.busy_timeout(BUSY_TIMEOUT)?;
    conn.pragma_update(None, "journal_mode", "WAL")?;
    conn.pragma_update(None, "synchronous", "NORMAL")?;
    Ok(conn)
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
    mut shutdown: watch::Receiver<bool>,
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
            tokio::select! {
                _ = interval.tick() => {
                    if let Err(err) = flush_once(&store, &config).await {
                        tracing::error!(?err, "telemetry flush failed");
                    }
                }
                changed = shutdown.changed() => {
                    if changed.is_err() || *shutdown.borrow() {
                        break;
                    }
                }
            }
        }

        if let Err(err) = flush_once(&store, &config).await {
            tracing::error!(?err, "final telemetry flush failed");
        }
        Ok(())
    }))
}

/// Write the complete minute buckets currently held in `tier1` and refresh
/// the hourly archive. Called on every flush interval and once at shutdown.
async fn flush_once(store: &TelemetryStore, config: &Config) -> anyhow::Result<()> {
    let snapshot = store.tier1.snapshot();
    if snapshot.is_empty() {
        return Ok(());
    }

    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();

    let minute_rows = complete_buckets(&snapshot, 60);

    let telemetry_path = config.db_path.clone();
    let retention_cutoff = now.saturating_sub(config.db_retention_days as u64 * 86400);

    tokio::task::spawn_blocking(move || {
        let conn = open(&telemetry_path)?;
        for row in &minute_rows {
            insert_row(&conn, row)?;
        }
        conn.execute(
            "DELETE FROM rollups WHERE bucket_ts < ?1",
            rusqlite::params![retention_cutoff as i64],
        )?;
        Ok::<_, anyhow::Error>(())
    })
    .await
    .map_err(|err| anyhow::anyhow!("telemetry flush task failed: {err}"))??;

    let telemetry_path = config.db_path.clone();
    let archive_path = config.db_archive_path.clone();
    let retention_days = config.db_retention_days;

    tokio::task::spawn_blocking(move || {
        let hour_rows = build_hourly_from_minute_db(&telemetry_path, retention_days);
        let conn = open(&archive_path)?;
        for row in &hour_rows {
            insert_row(&conn, row)?;
        }
        Ok::<_, anyhow::Error>(())
    })
    .await
    .map_err(|err| anyhow::anyhow!("archive rollup task failed: {err}"))??;

    Ok(())
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

    let conn = match open(telemetry_path) {
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
    let conn = open(path)?;
    conn.execute_batch(SCHEMA)?;
    Ok(())
}

/// Bucket samples, keeping only buckets that are fully covered by the
/// snapshot. The oldest bucket may be truncated by the ring buffer and the
/// newest one is still in progress; upserting either would overwrite a
/// complete rollup with partial min/max/mean values.
fn complete_buckets(
    samples: &[sb_protocol::types::MetricSample],
    interval: i64,
) -> Vec<RollupRow> {
    let (Some(oldest), Some(newest)) = (samples.first(), samples.last()) else {
        return Vec::new();
    };
    let oldest = oldest.timestamp;
    let newest = newest.timestamp;

    bucket_by(samples, interval)
        .into_iter()
        .filter(|row| row.ts >= oldest && row.ts + interval - 1 <= newest)
        .collect()
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

#[cfg(test)]
mod tests {
    use super::{complete_buckets, flush_once, init_db, open};
    use crate::config::Config;
    use crate::store::TelemetryStore;
    use sb_protocol::types::MetricSample;

    fn sample(timestamp: i64) -> MetricSample {
        MetricSample {
            timestamp,
            cpu_temp: 50.0,
            cpu_usage: 10.0,
            fan_rpm: 2000,
            pwm_value: 128,
        }
    }

    #[test]
    fn complete_buckets_drops_partial_edges() {
        // Second 30 of minute 0 through second 20 of minute 2.
        let samples: Vec<_> = (30..=140).map(sample).collect();
        let rows = complete_buckets(&samples, 60);
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].ts, 60);
    }

    #[test]
    fn complete_buckets_keeps_fully_covered_buckets() {
        let samples: Vec<_> = (60..=179).map(sample).collect();
        let rows = complete_buckets(&samples, 60);
        let timestamps: Vec<i64> = rows.iter().map(|row| row.ts).collect();
        assert_eq!(timestamps, vec![60, 120]);
    }

    #[test]
    fn complete_buckets_skips_the_in_progress_bucket() {
        let samples: Vec<_> = (0..=130).map(sample).collect();
        let rows = complete_buckets(&samples, 60);
        let timestamps: Vec<i64> = rows.iter().map(|row| row.ts).collect();
        assert_eq!(timestamps, vec![0, 60]);
    }

    #[test]
    fn complete_buckets_handles_empty_input() {
        assert!(complete_buckets(&[], 60).is_empty());
    }

    fn test_config(db_path: String, archive_path: String) -> Config {
        Config {
            socket_path: "/tmp/sb-test.sock".into(),
            db_path,
            db_archive_path: archive_path,
            db_flush_secs: 600.0,
            db_retention_days: 7,
            pwm_baseline: 110,
            fan_soft_min: 40.0,
            fan_soft_max: 60.0,
            fan_hard_max: 80.0,
            fan_dwell_secs: 10.0,
            fan_step: 5,
            fan_hard_ramp_secs: 10.0,
            fan_drift_temp: 50.0,
            fan_drift_secs: 30.0,
            verbose: false,
        }
    }

    #[tokio::test]
    async fn flush_once_writes_complete_minutes() {
        let base = std::env::temp_dir();
        let path = base.join(format!("sb-flush-test-{}.db", std::process::id()));
        let archive =
            base.join(format!("sb-flush-archive-test-{}.db", std::process::id()));
        let path_str = path.to_string_lossy().to_string();
        let archive_str = archive.to_string_lossy().to_string();

        for suffix in ["", "-wal", "-shm"] {
            let _ = std::fs::remove_file(format!("{path_str}{suffix}"));
            let _ = std::fs::remove_file(format!("{archive_str}{suffix}"));
        }

        let config = test_config(path_str.clone(), archive_str.clone());
        init_db(&config.db_path).unwrap();
        init_db(&config.db_archive_path).unwrap();

        let store = TelemetryStore::new();
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs() as i64;
        let minute_start = now / 60 * 60;
        for ts in (minute_start - 180)..minute_start {
            store.tier1.push(sample(ts));
        }

        flush_once(&store, &config).await.expect("flush");

        let conn = open(&path_str).unwrap();
        let mut stmt = conn
            .prepare("SELECT bucket_ts FROM rollups ORDER BY bucket_ts")
            .unwrap();
        let rows: Vec<i64> = stmt
            .query_map([], |row| row.get(0))
            .unwrap()
            .filter_map(Result::ok)
            .collect();
        assert_eq!(
            rows,
            vec![minute_start - 180, minute_start - 120, minute_start - 60]
        );
        drop(stmt);
        drop(conn);

        for suffix in ["", "-wal", "-shm"] {
            let _ = std::fs::remove_file(format!("{path_str}{suffix}"));
            let _ = std::fs::remove_file(format!("{archive_str}{suffix}"));
        }
    }

    #[tokio::test]
    async fn shutdown_triggers_a_final_flush() {
        let base = std::env::temp_dir();
        let path = base.join(format!("sb-shutdown-test-{}.db", std::process::id()));
        let archive =
            base.join(format!("sb-shutdown-archive-test-{}.db", std::process::id()));
        let path_str = path.to_string_lossy().to_string();
        let archive_str = archive.to_string_lossy().to_string();

        for suffix in ["", "-wal", "-shm"] {
            let _ = std::fs::remove_file(format!("{path_str}{suffix}"));
            let _ = std::fs::remove_file(format!("{archive_str}{suffix}"));
        }

        let config = test_config(path_str.clone(), archive_str.clone());
        let store = TelemetryStore::new();
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs() as i64;
        let minute_start = now / 60 * 60;
        for ts in (minute_start - 180)..minute_start {
            store.tier1.push(sample(ts));
        }

        let (shutdown_tx, shutdown_rx) = tokio::sync::watch::channel(false);
        let handle = super::spawn(store, config, shutdown_rx).unwrap();
        shutdown_tx.send(true).unwrap();

        let joined = tokio::time::timeout(
            std::time::Duration::from_secs(10),
            handle,
        )
        .await
        .expect("db task to stop");
        assert!(joined.is_ok(), "db task panicked");
        assert!(joined.unwrap().is_ok(), "final flush failed");

        let conn = open(&path_str).unwrap();
        let count: i64 = conn
            .query_row("SELECT COUNT(*) FROM rollups", [], |row| row.get(0))
            .unwrap();
        assert_eq!(count, 3);
        drop(conn);

        for suffix in ["", "-wal", "-shm"] {
            let _ = std::fs::remove_file(format!("{path_str}{suffix}"));
            let _ = std::fs::remove_file(format!("{archive_str}{suffix}"));
        }
    }

    #[test]
    fn open_enables_wal_and_busy_timeout() {
        let path = std::env::temp_dir().join(format!("sb-db-test-{}.db", std::process::id()));
        let path_str = path.to_string_lossy().to_string();
        let _ = std::fs::remove_file(&path);

        {
            let conn = open(&path_str).expect("open temp db");
            let mode: String = conn
                .query_row("PRAGMA journal_mode", [], |row| row.get(0))
                .expect("read journal_mode");
            assert_eq!(mode.to_lowercase(), "wal");

            let timeout: i64 = conn
                .query_row("PRAGMA busy_timeout", [], |row| row.get(0))
                .expect("read busy_timeout");
            assert_eq!(timeout, 5000);
        }

        let _ = std::fs::remove_file(&path);
        let _ = std::fs::remove_file(format!("{path_str}-wal"));
        let _ = std::fs::remove_file(format!("{path_str}-shm"));
    }
}
