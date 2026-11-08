#!/bin/sh
set -e

deb-systemd-invoke stop sb-daemon.service >/dev/null 2>&1 || true

exit 0
