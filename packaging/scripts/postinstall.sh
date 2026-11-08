#!/bin/sh
set -e

deb-systemd-helper unmask sb-daemon.service >/dev/null 2>&1 || true
deb-systemd-helper enable sb-daemon.service >/dev/null 2>&1 || true
deb-systemd-helper update-state sb-daemon.service >/dev/null 2>&1 || true
deb-systemd-invoke restart sb-daemon.service >/dev/null 2>&1 || true

echo "sb-daemon installed; add your user to the 'sb' group: sudo usermod -aG sb <user>"
exit 0
