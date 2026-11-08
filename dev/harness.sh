#!/usr/bin/env bash
# Run sb-daemon against a fake sysfs tree, together with a fake thermal plant.
#
# Usage:
#   ./dev/harness.sh                 # headless (Ctrl-C to stop)
#   ./dev/harness.sh --tui           # open the TUI against the simulated daemon
#   ./dev/harness.sh --db PATH --static --tui
#                                    # use a copy of a real history DB; static
#                                    # live values seeded from its last sample
#
# The daemon is built with the `test-hwmon` feature so it can be pointed at the
# fake tree via SB_HWMON_ROOT / SB_PROC_STAT. Production builds do not contain
# those overrides.
set -euo pipefail

root=$(cd "$(dirname "$0")/.." && pwd)

use_tui=false
use_plant=true
source_db=""

usage() {
    cat <<'EOF'
usage: dev/harness.sh [options]

  --tui          open the TUI against the simulated daemon
  --db PATH      copy PATH to a temporary history DB and use it
  --static       do not run the simulated plant; seed the fake hwmon from the
                 last DB sample (or defaults without --db)
  -h, --help     show this help
EOF
}

while [[ $# -gt 0 ]]; do
    case "$1" in
        --tui)
            use_tui=true
            shift
            ;;
        --db)
            source_db=${2:?--db needs a path}
            shift 2
            ;;
        --static)
            use_plant=false
            shift
            ;;
        -h|--help)
            usage
            exit 0
            ;;
        *)
            echo "unknown option: $1" >&2
            usage >&2
            exit 1
            ;;
    esac
done

if [[ -n "$source_db" && ! -f "$source_db" ]]; then
    echo "history DB not found: $source_db" >&2
    exit 1
fi
if [[ -n "$source_db" ]]; then
    # A static live view is the only coherent mode when replaying history.
    use_plant=false
fi

cargo build --manifest-path "$root/Cargo.toml" -p sb-daemon --features test-hwmon
cargo build --manifest-path "$root/Cargo.toml" -p sb

work=$(mktemp -d "${TMPDIR:-/tmp}/sb-harness.XXXXXX")
hwmon="$work/hwmon"
proc_stat="$work/proc_stat"
socket="$work/sb.sock"
db="$work/history.db"
daemon_log="$work/daemon.log"
plant_log="$work/plant.log"

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

temp_milli=45000
fan_rpm=2000
pwm_value=110
flush_secs=10

if [[ -n "$source_db" ]]; then
    cp "$source_db" "$db"
    for suffix in -wal -shm; do
        if [[ -f "$source_db$suffix" ]]; then
            cp "$source_db$suffix" "$db$suffix"
        fi
    done

    if command -v sqlite3 >/dev/null 2>&1; then
        row=$(sqlite3 "$db" \
            "SELECT temp_mean || '|' || rpm_mean || '|' || pwm_mean
               FROM rollups_10s ORDER BY bucket_ts DESC LIMIT 1;" \
            2>/dev/null || true)
        if [[ -n "$row" ]]; then
            IFS='|' read -r temp_centi fan_rpm pwm_value <<< "$row"
            if [[ "$temp_centi" =~ ^[0-9]+$ ]]; then
                temp_milli=$((temp_centi * 10))
            fi
            if ! [[ "$fan_rpm" =~ ^[0-9]+$ ]]; then
                fan_rpm=2000
            fi
            if ! [[ "$pwm_value" =~ ^[0-9]+$ ]]; then
                pwm_value=110
            fi
        fi
    else
        echo "note: sqlite3 not found; using default live values" >&2
    fi

    # Keep the daemon from appending simulated 10 s/1 m rows to the copy while
    # the demo is running. The final flush still writes to the temporary copy,
    # never to the source DB.
    flush_secs=3600
fi

mkdir -p "$hwmon/hwmon0" "$hwmon/hwmon1"
printf 'coretemp\n' > "$hwmon/hwmon0/name"
printf '%s\n' "$temp_milli" > "$hwmon/hwmon0/temp1_input"
printf 'n95_ec\n' > "$hwmon/hwmon1/name"
printf '%s\n' "$pwm_value" > "$hwmon/hwmon1/pwm2"
printf '2\n' > "$hwmon/hwmon1/pwm2_enable"
printf '%s\n' "$fan_rpm" > "$hwmon/hwmon1/fan2_input"
printf 'cpu  100 0 100 800 0 0 0 0 0 0\n' > "$proc_stat"

cat > "$work/harness.toml" <<EOF
socket_path = "$socket"
db_path = "$db"
db_flush_secs = $flush_secs
EOF

if [[ -n "$source_db" ]]; then
    echo "history DB: $source_db (using a temporary copy)"
    echo "live values: ${temp_milli} mC, ${fan_rpm} RPM, PWM ${pwm_value}"
fi

cd "$work"

SB_HWMON_ROOT="$hwmon" SB_PROC_STAT="$proc_stat" \
    "$root/target/debug/sb-daemon" \
        --config-file "$work/harness.toml" \
        -v >"$daemon_log" 2>&1 &
daemon_pid=$!

for _ in $(seq 1 50); do
    if [[ -S "$socket" ]]; then
        break
    fi
    sleep 0.1
done
if [[ ! -S "$socket" ]]; then
    echo "daemon did not create $socket" >&2
    cat "$daemon_log" >&2
    exit 1
fi

if $use_plant; then
    "$root/dev/plant.sh" "$hwmon" "$proc_stat" >"$plant_log" 2>&1 &
    plant_pid=$!
fi

echo "fake sysfs:  $hwmon"
echo "socket:      $socket"
echo "database:    $db"
echo "daemon log:  $daemon_log"

if $use_tui; then
    "$root/target/debug/sb" --socket-path "$socket"
else
    echo "headless mode; press Ctrl-C to stop"
    wait "$daemon_pid"
fi
