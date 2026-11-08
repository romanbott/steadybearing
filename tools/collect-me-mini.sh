#!/usr/bin/env bash
# Read-only diagnostics for Beelink ME Mini / IT8613E fan control.
#
# Usage:
#   sudo bash tools/collect-me-mini.sh > /tmp/me-mini-report.txt 2>&1
#
# This script only reads files and runs read-only commands. It does not write
# to sysfs, load or unload modules, or change fan settings.
set -u

section() {
    printf '\n========== %s ==========\n' "$*"
}

run() {
    printf '\n$'
    printf ' %q' "$@"
    printf '\n'
    "$@" 2>&1 || true
}

run_sh() {
    printf '\n$ sh -c %q\n' "$1"
    sh -c "$1" 2>&1 || true
}

if [ "$(id -u)" -ne 0 ]; then
    echo "warning: run as root (sudo) for complete dmesg, dkms and service output" >&2
fi

section "system"
run uname -a
run cat /etc/os-release
run cat /proc/cmdline
run date

section "hardware identity"
for f in sys_vendor product_name product_version board_vendor board_name board_version bios_version bios_date; do
    if [ -r "/sys/class/dmi/id/$f" ]; then
        printf '%s: %s\n' "$f" "$(cat "/sys/class/dmi/id/$f")"
    fi
done

section "modules"
run lsmod
printf '\n--- it87 / coretemp / hwmon ---\n'
lsmod 2>/dev/null | grep -E '^(it87|coretemp|hwmon)' || true

if command -v modinfo >/dev/null 2>&1; then
    run modinfo it87
fi

for p in /sys/module/it87/parameters/*; do
    if [ -e "$p" ]; then
        printf '%s = %s\n' "$(basename "$p")" "$(cat "$p" 2>/dev/null)"
    fi
done

section "dkms and driver sources"
if command -v dkms >/dev/null 2>&1; then
    run dkms status
fi
run ls -la /usr/src
run ls -la /var/lib/dkms
run find /lib/modules/"$(uname -r)" -name 'it87.ko*' -ls
run ls -d /root/it87 /home/*/it87 2>/dev/null

section "module configuration"
for d in /etc/modprobe.d /etc/modules-load.d; do
    if [ -d "$d" ]; then
        run_sh "grep -RIn --include='*.conf' -E 'it87|acpi_enforce_resources|ignore_resource_conflict' '$d' 2>/dev/null"
    fi
done

section "secure boot"
if command -v mokutil >/dev/null 2>&1; then
    run mokutil --sb-state
else
    echo "mokutil not installed"
fi

section "hwmon inventory"
run ls -l /sys/class/hwmon
for h in /sys/class/hwmon/hwmon*; do
    [ -d "$h" ] || continue
    name=$(cat "$h/name" 2>/dev/null || echo '?')
    printf '\n--- %s (name=%s) ---\n' "$h" "$name"
    if command -v readlink >/dev/null 2>&1; then
        readlink -f "$h/device" 2>/dev/null || true
    fi
    for f in "$h"/*; do
        [ -f "$f" ] || continue
        base=$(basename "$f")
        case "$base" in
            temp*_input|temp*_label|fan*_input|fan*_label|pwm|pwm[0-9]*|pwm*_enable|pwm*_freq)
                printf '%s = %s\n' "$base" "$(cat "$f" 2>/dev/null)"
                ;;
        esac
    done
done

section "sensors"
if command -v sensors >/dev/null 2>&1; then
    run sensors
    run sensors -j
else
    echo "lm-sensors not installed"
fi

section "kernel log (it87 / ACPI resource conflicts)"
run_sh "dmesg 2>/dev/null | grep -iE 'it87|it8613|it8625|resource conflict|acpi.*conflict' | tail -80"
if command -v journalctl >/dev/null 2>&1; then
    run_sh "journalctl -k -b --no-pager 2>/dev/null | grep -iE 'it87|it8613|it8625|resource conflict|acpi.*conflict' | tail -80"
fi

section "packages"
if command -v dpkg >/dev/null 2>&1; then
    run_sh "dpkg -l | grep -E 'dkms|linux-headers|linux-image|lm-sensors|it87'"
fi

section "shell history hints"
if [ -r /root/.bash_history ]; then
    run_sh "grep -iE 'it87|dkms|modprobe|pwm2|acpi_enforce' /root/.bash_history | tail -60"
else
    echo "no readable /root/.bash_history"
fi

section "current sb daemon"
if command -v systemctl >/dev/null 2>&1; then
    run systemctl cat sb-daemon
    run_sh "systemctl status sb-daemon --no-pager 2>/dev/null | head -40"
    run_sh "journalctl -u sb-daemon -n 50 --no-pager 2>/dev/null"
fi
run ls -l /etc/sb/sb.toml /var/log/sb /var/lib/sb

printf '\nDone. No changes were made.\n'
