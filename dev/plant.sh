#!/usr/bin/env bash
# Drives the fake hardware tree used by the fake-sysfs harness.
#
# Usage: plant.sh <hwmon-root> <proc-stat-path>
#
# The model is intentionally simple: temperature rises with a slow load cycle
# and falls as the daemon raises the PWM duty cycle. It is enough to exercise
# cooling, drifting, hard-max and recovery without any real hardware.
set -euo pipefail

hwmon=${1:?usage: plant.sh <hwmon-root> <proc-stat-path>}
proc_stat=${2:?usage: plant.sh <hwmon-root> <proc-stat-path>}

temp_file="$hwmon/hwmon0/temp1_input"
pwm_file="$hwmon/hwmon1/pwm2"
tick=0

while :; do
    pwm=$(cat "$pwm_file" 2>/dev/null || echo 110)
    case "$pwm" in
        ''|*[!0-9]*) pwm=110 ;;
    esac

    load=$(( (tick % 120) * 100 ))
    temp=$(( 78000 + load - pwm * 120 ))
    if [ "$temp" -lt 0 ]; then
        temp=0
    fi
    printf '%s\n' "$temp" > "$temp_file"

    # Move the aggregate /proc/stat counters so usage is non-zero.
    idle=$(( 800 + tick * 8 ))
    busy=$(( 100 + tick * 2 ))
    printf 'cpu  %d 0 %d %d 0 0 0 0 0 0\n' "$busy" "$busy" "$idle" > "$proc_stat"

    tick=$(( tick + 1 ))
    sleep 1
done
