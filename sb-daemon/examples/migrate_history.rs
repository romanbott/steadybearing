// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Roman Contreras

//! One-off migration from the old two-file history layout to the unified
//! `history.db` schema.
//!
//! The old `telemetry.db.rollups` table (minute, REAL values) is imported into
//! `rollups_1m`, and `archive.db.rollups` (hourly) into `rollups_1h`. Only
//! complete buckets are copied. The 10 s and 10 m tiers start empty; the
//! daemon backfills 10 m from 1 m on its first run.
//!
//! Run through `tools/migrate-history.sh`, or directly:
//!
//! ```text
//! cargo run --release -p sb-daemon --example migrate_history -- \
//!     --telemetry /var/log/sb/telemetry.db \
//!     --archive /var/log/sb/archive.db \
//!     --output /var/lib/sb/history.db
//! ```

use std::path::Path;
use std::time::{SystemTime, UNIX_EPOCH};

use anyhow::Context;
use rusqlite::{params, Connection, OptionalExtension};

const DATABASE_VERSION: i64 = 1;

const TABLES: [&str; 4] = ["rollups_10s", "rollups_1m", "rollups_10m", "rollups_1h"];

fn main() -> anyhow::Result<()> {
    let mut telemetry = "/var/log/sb/telemetry.db".to_string();
    let mut archive = "/var/log/sb/archive.db".to_string();
    let mut output = "/var/lib/sb/history.db".to_string();
    let mut force = false;

    let mut args = std::env::args().skip(1);
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--telemetry" => {
                telemetry = args.next().context("--telemetry requires a path")?;
            }
            "--archive" => {
                archive = args.next().context("--archive requires a path")?;
            }
            "--output" => {
                output = args.next().context("--output requires a path")?;
            }
            "--force" => force = true,
            "--help" | "-h" => {
                print_help();
                return Ok(());
            }
            other => anyhow::bail!("unknown argument: {other}"),
        }
    }

    for (label, path) in [("telemetry", &telemetry), ("archive", &archive)] {
        anyhow::ensure!(
            Path::new(path).exists(),
            "{label} database not found: {path}"
        );
    }
    if let Some(parent) = Path::new(&output).parent() {
        std::fs::create_dir_all(parent)
            .with_context(|| format!("creating {}", parent.display()))?;
    }

    let conn = Connection::open(&output)
        .with_context(|| format!("opening {output}"))?;
    create_schema(&conn)?;

    if !force {
        let existing_1m: i64 = conn.query_row(
            "SELECT COUNT(*) FROM rollups_1m",
            [],
            |row| row.get::<_, i64>(0),
        )?;
        let existing_1h: i64 = conn.query_row(
            "SELECT COUNT(*) FROM rollups_1h",
            [],
            |row| row.get::<_, i64>(0),
        )?;
        let existing = existing_1m + existing_1h;
        anyhow::ensure!(
            existing == 0,
            "{output} already contains history; pass --force to replace the imported tiers"
        );
    } else {
        conn.execute("DELETE FROM rollups_1m", [])?;
        conn.execute("DELETE FROM rollups_1h", [])?;
    }

    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs() as i64;

    conn.execute("ATTACH DATABASE ?1 AS old_telemetry", params![telemetry])?;
    conn.execute("ATTACH DATABASE ?1 AS old_archive", params![archive])?;

    let copied_1m = if has_rollups(&conn, "old_telemetry")? {
        conn.execute(
            "INSERT OR REPLACE INTO rollups_1m
             (bucket_ts, temp_min, temp_max, temp_mean,
              usage_min, usage_max, usage_mean, rpm_mean, pwm_mean, sample_count)
             SELECT bucket_ts,
                    CAST(ROUND(temp_min  * 100.0) AS INTEGER),
                    CAST(ROUND(temp_max  * 100.0) AS INTEGER),
                    CAST(ROUND(temp_mean * 100.0) AS INTEGER),
                    CAST(ROUND(usage_min  * 100.0) AS INTEGER),
                    CAST(ROUND(usage_max  * 100.0) AS INTEGER),
                    CAST(ROUND(usage_mean * 100.0) AS INTEGER),
                    CAST(ROUND(rpm_mean) AS INTEGER),
                    CAST(ROUND(pwm_mean) AS INTEGER),
                    60
             FROM old_telemetry.rollups
             WHERE bucket_ts + 60 <= ?1",
            params![now],
        )?
    } else {
        println!("telemetry database has no rollups table; skipping minute import");
        0
    };

    let copied_1h = if has_rollups(&conn, "old_archive")? {
        conn.execute(
            "INSERT OR REPLACE INTO rollups_1h
             (bucket_ts, temp_min, temp_max, temp_mean,
              usage_min, usage_max, usage_mean, rpm_mean, pwm_mean, sample_count)
             SELECT bucket_ts,
                    CAST(ROUND(temp_min  * 100.0) AS INTEGER),
                    CAST(ROUND(temp_max  * 100.0) AS INTEGER),
                    CAST(ROUND(temp_mean * 100.0) AS INTEGER),
                    CAST(ROUND(usage_min  * 100.0) AS INTEGER),
                    CAST(ROUND(usage_max  * 100.0) AS INTEGER),
                    CAST(ROUND(usage_mean * 100.0) AS INTEGER),
                    CAST(ROUND(rpm_mean) AS INTEGER),
                    CAST(ROUND(pwm_mean) AS INTEGER),
                    3600
             FROM old_archive.rollups
             WHERE bucket_ts + 3600 <= ?1",
            params![now],
        )?
    } else {
        println!("archive database has no rollups table; skipping hourly import");
        0
    };

    conn.execute("DETACH DATABASE old_telemetry", [])?;
    conn.execute("DETACH DATABASE old_archive", [])?;

    let total_1m: i64 = conn.query_row(
        "SELECT COUNT(*) FROM rollups_1m",
        [],
        |row| row.get::<_, i64>(0),
    )?;
    let total_1h: i64 = conn.query_row(
        "SELECT COUNT(*) FROM rollups_1h",
        [],
        |row| row.get::<_, i64>(0),
    )?;

    println!("imported {copied_1m} minute rows ({total_1m} total)");
    println!("imported {copied_1h} hourly rows ({total_1h} total)");
    println!("wrote {output}");
    println!("note: 10 m history is backfilled from 1 m on the next daemon start");
    Ok(())
}

fn create_schema(conn: &Connection) -> anyhow::Result<()> {
    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS meta (key TEXT PRIMARY KEY, value TEXT NOT NULL);",
    )?;
    for table in TABLES {
        conn.execute_batch(&format!(
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
        ))?;
    }

    let version: Option<String> = conn
        .query_row(
            "SELECT value FROM meta WHERE key = 'schema_version'",
            [],
            |row| row.get::<_, String>(0),
        )
        .optional()?;
    match version {
        None => {
            conn.execute(
                "INSERT OR REPLACE INTO meta (key, value) VALUES ('schema_version', ?1)",
                params![DATABASE_VERSION.to_string()],
            )?;
        }
        Some(raw) => {
            let version: i64 = raw.parse().unwrap_or(0);
            anyhow::ensure!(
                version <= DATABASE_VERSION,
                "history database uses schema v{version}, newer than v{DATABASE_VERSION}"
            );
        }
    }
    Ok(())
}

fn has_rollups(conn: &Connection, schema: &str) -> anyhow::Result<bool> {
    let count: i64 = conn.query_row(
        &format!(
            "SELECT COUNT(*) FROM {schema}.sqlite_master
             WHERE type = 'table' AND name = 'rollups'"
        ),
        [],
        |row| row.get::<_, i64>(0),
    )?;
    Ok(count > 0)
}

fn print_help() {
    println!(
        "usage: migrate_history [--telemetry PATH] [--archive PATH] [--output PATH] [--force]"
    );
    println!();
    println!("defaults:");
    println!("  --telemetry /var/log/sb/telemetry.db");
    println!("  --archive   /var/log/sb/archive.db");
    println!("  --output    /var/lib/sb/history.db");
}
