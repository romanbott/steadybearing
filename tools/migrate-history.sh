#!/usr/bin/env bash
# One-off migration from the old two-file history layout to /var/lib/sb/history.db.
#
# Stop sb-daemon first, then run:
#   ./tools/migrate-history.sh
#
# Defaults:
#   --telemetry /var/log/sb/telemetry.db
#   --archive   /var/log/sb/archive.db
#   --output    /var/lib/sb/history.db
#
# The migration only copies complete 1 m / 1 h buckets and approximates
# sample_count. The 10 m tier is backfilled by the daemon on its next start;
# 10 s history starts fresh.
set -euo pipefail

root=$(cd "$(dirname "$0")/.." && pwd)
exec cargo run --release --manifest-path "$root/Cargo.toml" \
    -p sb-daemon --example migrate_history -- "$@"
