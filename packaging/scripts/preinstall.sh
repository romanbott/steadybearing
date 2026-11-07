#!/bin/sh
set -e

if ! getent group sb >/dev/null 2>&1; then
    groupadd --system sb 2>/dev/null || addgroup --system sb 2>/dev/null || true
fi

exit 0
