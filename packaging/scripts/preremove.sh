#!/bin/sh
set -e

if command -v deb-systemd-invoke >/dev/null 2>&1; then
    deb-systemd-invoke stop sb-daemon.service >/dev/null 2>&1 || true
else
    systemctl stop sb-daemon.service >/dev/null 2>&1 || true
fi

exit 0
