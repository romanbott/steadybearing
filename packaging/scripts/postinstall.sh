#!/bin/sh
set -e

if command -v deb-systemd-helper >/dev/null 2>&1; then
    deb-systemd-helper unmask sb-daemon.service >/dev/null 2>&1 || true
    deb-systemd-helper enable sb-daemon.service >/dev/null 2>&1 || true
    deb-systemd-helper update-state sb-daemon.service >/dev/null 2>&1 || true
else
    systemctl enable sb-daemon.service >/dev/null 2>&1 || true
fi

systemctl daemon-reload >/dev/null 2>&1 || true
systemctl restart sb-daemon.service >/dev/null 2>&1 || true

echo "sb-daemon installed; add your user to the 'sb' group: sudo usermod -aG sb <user>"
exit 0
