#!/bin/sh
set -e

if [ "$1" = "remove" ]; then
    deb-systemd-helper mask sb-daemon.service >/dev/null 2>&1 || true
fi

if [ "$1" = "purge" ]; then
    deb-systemd-helper purge sb-daemon.service >/dev/null 2>&1 || true
    deb-systemd-helper unmask sb-daemon.service >/dev/null 2>&1 || true
fi

exit 0
