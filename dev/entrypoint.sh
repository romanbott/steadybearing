#!/usr/bin/env bash
# Container entrypoint for the fake-sysfs harness.
#
# With no arguments it runs headless (daemon + plant) until stopped. With
# arguments it runs them with the given command, e.g.:
#   docker run --rm -it sb-harness sb
set -euo pipefail

work=${SB_WORK:-/tmp/sb-sim}
hwmon="$work/hwmon"
proc_stat="$work/proc_stat"
socket="/run/sb/sb.sock"
db="$work/telemetry.db"
archive="$work/archive.db"

mkdir -p "$hwmon/hwmon0" "$hwmon/hwmon1" /run/sb "$work"
printf 'coretemp\n' > "$hwmon/hwmon0/name"
printf '45000\n' > "$hwmon/hwmon0/temp1_input"
printf 'n95_ec\n' > "$hwmon/hwmon1/name"
printf '110\n' > "$hwmon/hwmon1/pwm2"
printf '2\n' > "$hwmon/hwmon1/pwm2_enable"
printf '2000\n' > "$hwmon/hwmon1/fan1_input"
printf 'cpu  100 0 100 800 0 0 0 0 0 0\n' > "$proc_stat"

cat > "$work/harness.toml" <<EOF
socket_path = "$socket"
db_path = "$db"
db_archive_path = "$archive"
db_flush_secs = 10
EOF

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
}
trap cleanup EXIT INT TERM

cd "$work"
SB_HWMON_ROOT="$hwmon" SB_PROC_STAT="$proc_stat" \
    sb-daemon \
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

plant.sh "$hwmon" "$proc_stat" &
plant_pid=$!

echo "fake hwmon: $hwmon"
echo "socket:     $socket"

if [[ $# -eq 0 ]]; then
    wait "$daemon_pid"
else
    "$@"
fi
