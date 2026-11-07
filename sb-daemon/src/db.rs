use std::collections::BTreeMap;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use rusqlite::{params, Connection, OptionalExtension};
use tokio::sync::watch;
use tokio::task::JoinHandle;

use sb_protocol::types::{HistoricalResolution, HistoricalRow, MetricSample};

use crate::config::Config;
use crate::store::TelemetryStore;

const SCHEMA_VERSION: i64 = 1;
const BUSY_TIMEOUT: Duration = Duration::from_secs(5);

const TIER_10S: &str = "rollups_10s";
const TIER_1M: &str = "rollups_1m";
const TIER_10M: &str = "rollups_10m";
const TIER_1H: &str = "rollups_1h";

const TEN_SECONDS: i64 = 10;
const ONE_MINUTE: i64 = 60;
const TEN_MINUTES: i64 = 600;
const ONE_HOUR: i64 = 3600;

/// Metrics are stored scaled by 100 (0.01 °C / 0.01 % resolution); rpm and
/// pwm are stored as-is. Fixed-size integers keep rows around 45-50 bytes.
const SCALE: f64 = 100.0;

#[derive(Debug, Clone, PartialEq, Eq)]
struct RollupRow {
    ts: i64,
    temp_min: i64,
    temp_max: i64,
    temp_mean: i64,
    usage_min: i64,
    usage_max: i64,
    usage_mean: i64,
    rpm_mean: i64,
    pwm_mean: i64,
    sample_count: i64,
}

/// Open a database connection configured for concurrent readers and writers.
pub(crate) fn open(path: &str) -> anyhow::Result<Connection> {
    let conn = Connection::open(path)?;
    conn.busy_timeout(BUSY_TIMEOUT)?;
    conn.pragma_update(None, "journal_mode", "WAL")?;
    conn.pragma_update(None, "synchronous", "NORMAL")?;
    Ok(conn)
}

pub fn spawn(
    store: TelemetryStore,
    config: Config,
    mut shutdown: watch::Receiver<bool>,
) -> anyhow::Result<JoinHandle<anyhow::Result<()>>> {
    create_dir_if_needed(&config.db_path)?;
    init_db(&config.db_path)?;

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

/// Persist every complete bucket currently in `tier1`, roll the coarse tiers
/// forward, and prune expired fine history. Called on every flush interval
/// and once at shutdown.
async fn flush_once(store: &TelemetryStore, config: &Config) -> anyhow::Result<()> {
    let snapshot = store.tier1.snapshot();
    if snapshot.is_empty() {
        return Ok(());
    }

    let now = now_secs();
    let path = config.db_path.clone();
    let ten_second_days = config.retention.ten_second_days;
    let minute_days = config.retention.minute_days;

    tokio::task::spawn_blocking(move || {
        let mut conn = open(&path)?;
        let tx = conn.transaction()?;

        for row in complete_buckets(&snapshot, TEN_SECONDS) {
            insert_row(&tx, TIER_10S, &row)?;
        }
        for row in complete_buckets(&snapshot, ONE_MINUTE) {
            insert_row(&tx, TIER_1M, &row)?;
        }

        rollup_from(&tx, TIER_1M, TIER_10M, TEN_MINUTES, now)?;
        rollup_from(&tx, TIER_10M, TIER_1H, ONE_HOUR, now)?;

        prune(&tx, TIER_10S, now.saturating_sub(ten_second_days as i64 * 86400))?;
        prune(&tx, TIER_1M, now.saturating_sub(minute_days as i64 * 86400))?;

        tx.commit()?;
        Ok::<_, anyhow::Error>(())
    })
    .await
    .map_err(|err| anyhow::anyhow!("telemetry flush task failed: {err}"))??;

    Ok(())
}

fn create_dir_if_needed(path: &str) -> anyhow::Result<()> {
    if let Some(parent) = std::path::Path::new(path).parent() {
        std::fs::create_dir_all(parent)?;
    }
    Ok(())
}

fn init_db(path: &str) -> anyhow::Result<()> {
    let conn = open(path)?;
    create_schema(&conn)?;
    Ok(())
}

fn create_schema(conn: &Connection) -> anyhow::Result<()> {
    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS meta (key TEXT PRIMARY KEY, value TEXT NOT NULL);",
    )?;
    for table in [TIER_10S, TIER_1M, TIER_10M, TIER_1H] {
        conn.execute_batch(&table_ddl(table))?;
    }

    let version: Option<String> = conn
        .query_row(
            "SELECT value FROM meta WHERE key = 'schema_version'",
            [],
            |row| row.get(0),
        )
        .optional()?;
    match version {
        None => {
            conn.execute(
                "INSERT OR REPLACE INTO meta (key, value) VALUES ('schema_version', ?1)",
                params![SCHEMA_VERSION.to_string()],
            )?;
        }
        Some(raw) => {
            let version: i64 = raw.parse().unwrap_or(0);
            anyhow::ensure!(
                version <= SCHEMA_VERSION,
                "history database uses schema v{version}, but this build supports up to v{SCHEMA_VERSION}"
            );
        }
    }
    Ok(())
}

fn table_ddl(table: &str) -> String {
    format!(
        "CREATE TABLE IF NOT EXISTS {table} (
            bucket_ts    INTEGER NOT NULL PRIMARY KEY,
            temp_min     INTEGER NOT NULL,
            temp_max     INTEGER NOT NULL,
            temp_mean    INTEGER NOT NULL,
            usage_min    INTEGER NOT NULL,
            usage_max    INTEGER NOT NULL,
            usage_mean   INTEGER NOT NULL,
            rpm_mean     INTEGER NOT NULL,
            pwm_mean     INTEGER NOT NULL,
            sample_count INTEGER NOT NULL
        );"
    )
}

/// Bucket samples, keeping only buckets that are fully covered by the
/// snapshot. The oldest bucket may be truncated by the ring buffer and the
/// newest one is still in progress.
fn complete_buckets(samples: &[MetricSample], interval: i64) -> Vec<RollupRow> {
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

fn bucket_by(samples: &[MetricSample], interval: i64) -> Vec<RollupRow> {
    let mut buckets: BTreeMap<i64, Vec<&MetricSample>> = BTreeMap::new();
    for sample in samples {
        let ts = sample.timestamp / interval * interval;
        buckets.entry(ts).or_default().push(sample);
    }

    buckets
        .into_iter()
        .map(|(ts, group)| aggregate_raw(ts, &group))
        .collect()
}

fn aggregate_raw(ts: i64, group: &[&MetricSample]) -> RollupRow {
    let count = group.len() as i64;
    let temps: Vec<i64> = group
        .iter()
        .map(|sample| scale(sample.cpu_temp))
        .collect();
    let usages: Vec<i64> = group
        .iter()
        .map(|sample| scale(sample.cpu_usage.clamp(0.0, 100.0)))
        .collect();
    let rpms: Vec<i64> = group.iter().map(|sample| sample.fan_rpm as i64).collect();
    let pwms: Vec<i64> = group.iter().map(|sample| sample.pwm_value as i64).collect();

    RollupRow {
        ts,
        temp_min: temps.iter().copied().min().unwrap_or(0),
        temp_max: temps.iter().copied().max().unwrap_or(0),
        temp_mean: mean_round(temps.iter().sum(), count),
        usage_min: usages.iter().copied().min().unwrap_or(0),
        usage_max: usages.iter().copied().max().unwrap_or(0),
        usage_mean: mean_round(usages.iter().sum(), count),
        rpm_mean: mean_round(rpms.iter().sum(), count),
        pwm_mean: mean_round(pwms.iter().sum(), count),
        sample_count: count,
    }
}

/// Aggregate complete source rows into `interval`-second buckets newer than
/// the last row already present in `target`.
fn rollup_from(
    conn: &Connection,
    source: &str,
    target: &str,
    interval: i64,
    now: i64,
) -> anyhow::Result<usize> {
    let watermark: Option<i64> = conn
        .query_row(&format!("SELECT MAX(bucket_ts) FROM {target}"), [], |row| {
            row.get::<_, Option<i64>>(0)
        })
        .optional()?
        .flatten();
    let start = watermark.map_or(0, |ts| ts + interval);

    let mut stmt = conn.prepare(&format!(
        "SELECT bucket_ts, temp_min, temp_max, temp_mean,
                usage_min, usage_max, usage_mean, rpm_mean, pwm_mean, sample_count
         FROM {source} WHERE bucket_ts >= ?1 ORDER BY bucket_ts"
    ))?;
    let rows: Vec<RollupRow> = stmt
        .query_map(params![start], read_row)?
        .filter_map(Result::ok)
        .collect();

    let mut groups: BTreeMap<i64, Vec<RollupRow>> = BTreeMap::new();
    for row in rows {
        let ts = row.ts / interval * interval;
        groups.entry(ts).or_default().push(row);
    }

    let mut inserted = 0;
    for (ts, group) in groups {
        if ts + interval > now {
            continue;
        }
        let row = aggregate_rows(ts, &group);
        insert_row(conn, target, &row)?;
        inserted += 1;
    }
    Ok(inserted)
}

fn aggregate_rows(ts: i64, group: &[RollupRow]) -> RollupRow {
    let count: i64 = group.iter().map(|row| row.sample_count).sum();
    let weighted = |get: fn(&RollupRow) -> i64| -> i64 {
        if count == 0 {
            return 0;
        }
        let sum: i64 = group
            .iter()
            .map(|row| get(row) * row.sample_count)
            .sum();
        mean_round(sum, count)
    };

    RollupRow {
        ts,
        temp_min: group.iter().map(|row| row.temp_min).min().unwrap_or(0),
        temp_max: group.iter().map(|row| row.temp_max).max().unwrap_or(0),
        temp_mean: weighted(|row| row.temp_mean),
        usage_min: group.iter().map(|row| row.usage_min).min().unwrap_or(0),
        usage_max: group.iter().map(|row| row.usage_max).max().unwrap_or(0),
        usage_mean: weighted(|row| row.usage_mean),
        rpm_mean: weighted(|row| row.rpm_mean),
        pwm_mean: weighted(|row| row.pwm_mean),
        sample_count: count,
    }
}

fn read_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<RollupRow> {
    Ok(RollupRow {
        ts: row.get(0)?,
        temp_min: row.get(1)?,
        temp_max: row.get(2)?,
        temp_mean: row.get(3)?,
        usage_min: row.get(4)?,
        usage_max: row.get(5)?,
        usage_mean: row.get(6)?,
        rpm_mean: row.get(7)?,
        pwm_mean: row.get(8)?,
        sample_count: row.get(9)?,
    })
}

fn insert_row(conn: &Connection, table: &str, row: &RollupRow) -> anyhow::Result<()> {
    conn.execute(
        &format!(
            "INSERT OR REPLACE INTO {table}
             (bucket_ts, temp_min, temp_max, temp_mean,
              usage_min, usage_max, usage_mean, rpm_mean, pwm_mean, sample_count)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)"
        ),
        params![
            row.ts,
            row.temp_min,
            row.temp_max,
            row.temp_mean,
            row.usage_min,
            row.usage_max,
            row.usage_mean,
            row.rpm_mean,
            row.pwm_mean,
            row.sample_count,
        ],
    )?;
    Ok(())
}

fn prune(conn: &Connection, table: &str, cutoff: i64) -> anyhow::Result<usize> {
    let removed = conn.execute(
        &format!("DELETE FROM {table} WHERE bucket_ts < ?1"),
        params![cutoff],
    )?;
    Ok(removed)
}

pub(crate) fn query_historical(
    path: &str,
    resolution: HistoricalResolution,
    count: usize,
) -> anyhow::Result<Vec<HistoricalRow>> {
    let table = match resolution {
        HistoricalResolution::TenSecond => TIER_10S,
        HistoricalResolution::Minute => TIER_1M,
        HistoricalResolution::TenMinute => TIER_10M,
        HistoricalResolution::Hourly => TIER_1H,
    };

    let conn = open(path)?;
    let mut stmt = conn.prepare(&format!(
        "SELECT bucket_ts, temp_min, temp_max, temp_mean,
                usage_min, usage_max, usage_mean, rpm_mean, pwm_mean
         FROM {table} ORDER BY bucket_ts DESC LIMIT ?1"
    ))?;
    let mut rows: Vec<HistoricalRow> = stmt
        .query_map(params![count as i64], |row| {
            Ok(HistoricalRow {
                timestamp: row.get(0)?,
                temp_min: unscale(row.get(1)?),
                temp_max: unscale(row.get(2)?),
                temp_mean: unscale(row.get(3)?),
                usage_min: unscale(row.get(4)?),
                usage_max: unscale(row.get(5)?),
                usage_mean: unscale(row.get(6)?),
                rpm_mean: row.get::<_, i64>(7)? as f32,
                pwm_mean: row.get::<_, i64>(8)? as f32,
            })
        })?
        .filter_map(Result::ok)
        .collect();
    rows.reverse();
    Ok(rows)
}

fn scale(value: f32) -> i64 {
    (value as f64 * SCALE).round() as i64
}

fn unscale(value: i64) -> f32 {
    (value as f64 / SCALE) as f32
}

fn mean_round(sum: i64, count: i64) -> i64 {
    if count == 0 {
        0
    } else {
        (sum as f64 / count as f64).round() as i64
    }
}

fn now_secs() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs() as i64
}

#[cfg(test)]
mod tests {
    use super::{
        bucket_by, complete_buckets, init_db, insert_row, open, prune, query_historical,
        rollup_from, RollupRow, TIER_10S, TIER_10M, TIER_1M,
    };
    use crate::config::{Config, RetentionConfig};
    use crate::store::TelemetryStore;
    use sb_protocol::types::{HistoricalResolution, MetricSample};
    use std::path::PathBuf;

    fn sample(timestamp: i64) -> MetricSample {
        metric(timestamp, 50.0, 10.0, 2000, 128)
    }

    fn metric(
        timestamp: i64,
        cpu_temp: f32,
        cpu_usage: f32,
        fan_rpm: u16,
        pwm_value: u8,
    ) -> MetricSample {
        MetricSample {
            timestamp,
            cpu_temp,
            cpu_usage,
            fan_rpm,
            pwm_value,
        }
    }

    fn test_config(db_path: String) -> Config {
        Config {
            socket_path: "/tmp/sb-test.sock".into(),
            socket_gid: None,
            db_path,
            db_flush_secs: 60.0,
            retention: RetentionConfig {
                ten_second_days: 1,
                minute_days: 7,
            },
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

    fn temp_db(name: &str) -> PathBuf {
        let base = std::env::temp_dir().join(format!(
            "sb-history-test-{}-{name}.db",
            std::process::id()
        ));
        cleanup(&base.to_string_lossy());
        base
    }

    fn cleanup(path: &str) {
        for suffix in ["", "-wal", "-shm"] {
            let _ = std::fs::remove_file(format!("{path}{suffix}"));
        }
    }

    fn row(ts: i64, mean: i64, count: i64) -> RollupRow {
        RollupRow {
            ts,
            temp_min: mean - 100,
            temp_max: mean + 100,
            temp_mean: mean,
            usage_min: 500,
            usage_max: 1500,
            usage_mean: 1000,
            rpm_mean: 2000,
            pwm_mean: 128,
            sample_count: count,
        }
    }

    #[test]
    fn raw_buckets_are_scaled_and_weighted() {
        let samples = [
            metric(0, 50.0, 10.0, 2000, 128),
            metric(1, 52.0, 20.0, 2200, 130),
        ];
        let rows = bucket_by(&samples, 60);
        assert_eq!(rows.len(), 1);
        let row = &rows[0];
        assert_eq!(row.temp_min, 5000);
        assert_eq!(row.temp_max, 5200);
        assert_eq!(row.temp_mean, 5100);
        assert_eq!(row.usage_min, 1000);
        assert_eq!(row.usage_max, 2000);
        assert_eq!(row.usage_mean, 1500);
        assert_eq!(row.rpm_mean, 2100);
        assert_eq!(row.pwm_mean, 129);
        assert_eq!(row.sample_count, 2);
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

    #[test]
    fn rollup_uses_sample_count_weights() {
        let path = temp_db("rollup");
        let path_str = path.to_string_lossy().to_string();
        init_db(&path_str).unwrap();
        let conn = open(&path_str).unwrap();

        // Nine full minutes at 5000 and one sparse minute at 6000.
        for minute in 0..9 {
            insert_row(&conn, TIER_1M, &row(600 + minute * 60, 5000, 60)).unwrap();
        }
        insert_row(&conn, TIER_1M, &row(600 + 9 * 60, 6000, 30)).unwrap();

        let inserted = rollup_from(&conn, TIER_1M, TIER_10M, 600, 2000).unwrap();
        assert_eq!(inserted, 1);

        let (ts, mean, count): (i64, i64, i64) = conn
            .query_row(
                "SELECT bucket_ts, temp_mean, sample_count FROM rollups_10m",
                [],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
            )
            .unwrap();
        assert_eq!(ts, 600);
        assert_eq!(count, 570);
        // (9*60*5000 + 30*6000) / 570 = 5053
        assert_eq!(mean, 5053);

        // A second pass has no new source rows.
        assert_eq!(rollup_from(&conn, TIER_1M, TIER_10M, 600, 2000).unwrap(), 0);

        drop(conn);
        cleanup(&path_str);
    }

    #[test]
    fn prune_removes_rows_before_the_cutoff() {
        let path = temp_db("prune");
        let path_str = path.to_string_lossy().to_string();
        init_db(&path_str).unwrap();
        let conn = open(&path_str).unwrap();

        insert_row(&conn, TIER_10S, &row(100, 5000, 10)).unwrap();
        insert_row(&conn, TIER_10S, &row(1000, 5000, 10)).unwrap();

        assert_eq!(prune(&conn, TIER_10S, 500).unwrap(), 1);
        let remaining: i64 = conn
            .query_row("SELECT COUNT(*) FROM rollups_10s", [], |row| row.get(0))
            .unwrap();
        assert_eq!(remaining, 1);

        drop(conn);
        cleanup(&path_str);
    }

    #[tokio::test]
    async fn flush_once_writes_fine_tiers() {
        let path = temp_db("flush");
        let path_str = path.to_string_lossy().to_string();
        let config = test_config(path_str.clone());
        init_db(&config.db_path).unwrap();

        let store = TelemetryStore::new();
        let now = super::now_secs();
        let minute_start = now / 60 * 60;
        for ts in (minute_start - 180)..minute_start {
            store.tier1.push(sample(ts));
        }

        super::flush_once(&store, &config).await.expect("flush");

        let conn = open(&path_str).unwrap();
        let count_1m: i64 = conn
            .query_row("SELECT COUNT(*) FROM rollups_1m", [], |row| row.get(0))
            .unwrap();
        let count_10s: i64 = conn
            .query_row("SELECT COUNT(*) FROM rollups_10s", [], |row| row.get(0))
            .unwrap();
        assert_eq!(count_1m, 3);
        assert_eq!(count_10s, 18);

        let mean: i64 = conn
            .query_row("SELECT temp_mean FROM rollups_1m LIMIT 1", [], |row| {
                row.get(0)
            })
            .unwrap();
        assert_eq!(mean, 5000);

        drop(conn);
        cleanup(&path_str);
    }

    #[tokio::test]
    async fn shutdown_triggers_a_final_flush() {
        let path = temp_db("shutdown");
        let path_str = path.to_string_lossy().to_string();
        let config = test_config(path_str.clone());

        let store = TelemetryStore::new();
        let now = super::now_secs();
        let minute_start = now / 60 * 60;
        for ts in (minute_start - 180)..minute_start {
            store.tier1.push(sample(ts));
        }

        let (shutdown_tx, shutdown_rx) = tokio::sync::watch::channel(false);
        let handle = super::spawn(store, config, shutdown_rx).unwrap();
        shutdown_tx.send(true).unwrap();

        let joined = tokio::time::timeout(std::time::Duration::from_secs(10), handle)
            .await
            .expect("db task to stop");
        assert!(joined.is_ok(), "db task panicked");
        assert!(joined.unwrap().is_ok(), "final flush failed");

        let conn = open(&path_str).unwrap();
        let count: i64 = conn
            .query_row("SELECT COUNT(*) FROM rollups_1m", [], |row| row.get(0))
            .unwrap();
        assert_eq!(count, 3);
        drop(conn);

        cleanup(&path_str);
    }

    #[test]
    fn query_historical_returns_scaled_values_in_order() {
        let path = temp_db("query");
        let path_str = path.to_string_lossy().to_string();
        init_db(&path_str).unwrap();
        let conn = open(&path_str).unwrap();
        insert_row(&conn, TIER_1M, &row(60, 5000, 60)).unwrap();
        insert_row(&conn, TIER_1M, &row(120, 6000, 60)).unwrap();
        insert_row(&conn, TIER_10S, &row(10, 4100, 10)).unwrap();
        insert_row(&conn, TIER_10M, &row(600, 5100, 600)).unwrap();
        drop(conn);

        let rows = query_historical(&path_str, HistoricalResolution::Minute, 10).unwrap();
        assert_eq!(rows.len(), 2);
        assert_eq!(rows[0].timestamp, 60);
        assert_eq!(rows[0].temp_mean, 50.0);
        assert_eq!(rows[0].rpm_mean, 2000.0);
        assert_eq!(rows[1].timestamp, 120);

        let rows = query_historical(&path_str, HistoricalResolution::TenSecond, 10).unwrap();
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].temp_mean, 41.0);

        let rows = query_historical(&path_str, HistoricalResolution::TenMinute, 10).unwrap();
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].temp_mean, 51.0);

        cleanup(&path_str);
    }

    #[test]
    fn schema_rejects_newer_versions() {
        let path = temp_db("version");
        let path_str = path.to_string_lossy().to_string();
        init_db(&path_str).unwrap();

        let conn = open(&path_str).unwrap();
        conn.execute(
            "UPDATE meta SET value = '999' WHERE key = 'schema_version'",
            [],
        )
        .unwrap();
        drop(conn);

        assert!(init_db(&path_str).is_err());
        cleanup(&path_str);
    }

    #[test]
    fn open_enables_wal_and_busy_timeout() {
        let path = temp_db("open");
        let path_str = path.to_string_lossy().to_string();
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
        cleanup(&path_str);
    }
}
