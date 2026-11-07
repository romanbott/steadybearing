#!/usr/bin/env bash
# Run sb-daemon against a fake sysfs tree, together with a fake thermal plant.
#
# Usage:
#   ./dev/harness.sh          # headless (Ctrl-C to stop)
#   ./dev/harness.sh --tui    # open the TUI against the simulated daemon
#
# The daemon is built with the `test-hwmon` feature so it can be pointed at
# the fake tree via SB_HWMON_ROOT / SB_PROC_STAT. Production builds do not
# contain those overrides.
set -euo pipefail

root=$(cd "$(dirname "$0")/.." && pwd)

use_tui=false
if [[ "${1:-}" == "--tui" ]]; then
    use_tui=true
fi

cargo build --manifest-path "$root/Cargo.toml" -p sb-daemon --features test-hwmon
cargo build --manifest-path "$root/Cargo.toml" -p sb

work=$(mktemp -d "${TMPDIR:-/tmp}/sb-harness.XXXXXX")
hwmon="$work/hwmon"
proc_stat="$work/proc_stat"
socket="$work/sb.sock"
db="$work/history.db"

daemon_pid=""
plant_pid=""

cleanup() {
    trap - EXIT INT TERM
    if [[ -n "$plant_pid" ]]; then
        kill "$plant_pid" 2>/dev/null || true
    fi
    if [[ -n "$daemon_pid" ]]; then
        kill -TERM "$daemon_pid" 2>/dev/null || true
    fi
    wait 2>/dev/null || true
    if [[ "${SB_KEEP:-0}" == "1" ]]; then
        echo "kept harness state in $work"
    else
        rm -rf "$work"
    fi
}
trap cleanup EXIT INT TERM

mkdir -p "$hwmon/hwmon0" "$hwmon/hwmon1"
printf 'coretemp\n' > "$hwmon/hwmon0/name"
printf '45000\n' > "$hwmon/hwmon0/temp1_input"
printf 'n95_ec\n' > "$hwmon/hwmon1/name"
printf '110\n' > "$hwmon/hwmon1/pwm2"
printf '2\n' > "$hwmon/hwmon1/pwm2_enable"
printf '2000\n' > "$hwmon/hwmon1/fan1_input"
printf 'cpu  100 0 100 800 0 0 0 0 0 0\n' > "$proc_stat"

# Use an explicit config file so a stray sb.toml in the repository or under
# /etc/sb cannot take precedence over the harness paths.
cat > "$work/harness.toml" <<EOF
socket_path = "$socket"
db_path = "$db"
db_flush_secs = 10
EOF

cd "$work"

SB_HWMON_ROOT="$hwmon" SB_PROC_STAT="$proc_stat" \
    "$root/target/debug/sb-daemon" \
        --config-file "$work/harness.toml" \
        -v &
daemon_pid=$!

for _ in $(seq 1 50); do
    if [[ -S "$socket" ]]; then
        break
    fi
    sleep 0.1
done
if [[ ! -S "$socket" ]]; then
    echo "daemon did not create $socket" >&2
    exit 1
fi

"$root/dev/plant.sh" "$hwmon" "$proc_stat" &
plant_pid=$!

echo "fake sysfs: $hwmon"
echo "socket:     $socket"
echo "database:   $db"

if $use_tui; then
    "$root/target/debug/sb" --socket-path "$socket"
else
    echo "headless mode; press Ctrl-C to stop"
    wait "$daemon_pid"
fi
