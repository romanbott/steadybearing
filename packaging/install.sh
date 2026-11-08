#!/usr/bin/env bash
# Install prebuilt sb-daemon / sb from a release tarball.
#
#   sudo ./install.sh
#   sudo ./install.sh --prefix /usr
#   sudo ./install.sh --uninstall
#
# The script expects the release layout:
#   sb-daemon  sb  sb-daemon.service  sb.toml  sysusers.d/sb.conf
set -euo pipefail

PREFIX=/usr/local
BINDIR=""
LIBDIR=""
MANDIR=""
DOCDIR=""
UNIT_DIR=/etc/systemd/system
CONFIG_DIR=/etc/sb
STATE_DIR=/var/lib/sb
SYSUSERS_DIR=/usr/lib/sysusers.d
DO_START=1
UNINSTALL=0
PURGE=0

usage() {
    cat <<'EOF'
usage: install.sh [options]

  --prefix DIR       installation prefix (default /usr/local)
  --bindir DIR       binary directory (default PREFIX/bin)
  --libdir DIR       helper binary directory (default PREFIX/lib/sb)
  --mandir DIR       man page directory (default PREFIX/share/man/man1)
  --docdir DIR       documentation directory (default PREFIX/share/doc/sb)
  --unit-dir DIR     systemd unit directory (default /etc/systemd/system)
  --config-dir DIR   configuration directory (default /etc/sb)
  --no-start         install but do not start/restart the service
  --uninstall        remove binaries and the unit
  --purge            with --uninstall, also remove config and state
  -h, --help         show this help
EOF
}

while [ $# -gt 0 ]; do
    case "$1" in
        --prefix)
            PREFIX=${2:?--prefix needs a value}
            shift 2
            ;;
        --bindir)
            BINDIR=${2:?--bindir needs a value}
            shift 2
            ;;
        --libdir)
            LIBDIR=${2:?--libdir needs a value}
            shift 2
            ;;
        --mandir)
            MANDIR=${2:?--mandir needs a value}
            shift 2
            ;;
        --docdir)
            DOCDIR=${2:?--docdir needs a value}
            shift 2
            ;;
        --unit-dir)
            UNIT_DIR=${2:?--unit-dir needs a value}
            shift 2
            ;;
        --config-dir)
            CONFIG_DIR=${2:?--config-dir needs a value}
            shift 2
            ;;
        --no-start)
            DO_START=0
            shift
            ;;
        --uninstall)
            UNINSTALL=1
            shift
            ;;
        --purge)
            PURGE=1
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

if [ -z "$BINDIR" ]; then
    BINDIR="$PREFIX/bin"
fi
if [ -z "$LIBDIR" ]; then
    LIBDIR="$PREFIX/lib/sb"
fi
if [ -z "$MANDIR" ]; then
    MANDIR="$PREFIX/share/man/man1"
fi
if [ -z "$DOCDIR" ]; then
    DOCDIR="$PREFIX/share/doc/sb"
fi

if [ "$(id -u)" -ne 0 ]; then
    echo "run as root, for example: sudo $0 $*" >&2
    exit 1
fi

src=$(cd "$(dirname "$0")" && pwd)
have_systemd=0
if command -v systemctl >/dev/null 2>&1; then
    have_systemd=1
fi

if [ "$UNINSTALL" = 1 ]; then
    if [ "$have_systemd" = 1 ]; then
        systemctl disable --now sb-daemon.service >/dev/null 2>&1 || true
        systemctl unmask sb-daemon.service >/dev/null 2>&1 || true
    fi
    rm -f "$BINDIR/sb-daemon" "$BINDIR/sb" "$UNIT_DIR/sb-daemon.service"
    rm -f "$LIBDIR/migrate-history"
    rmdir "$LIBDIR" 2>/dev/null || true
    rm -f "$SYSUSERS_DIR/sb.conf"
    rm -f "$MANDIR/sb.1.gz" "$MANDIR/sb-daemon.1.gz"
    rm -f "$DOCDIR/README.md" "$DOCDIR/LICENSE" "$DOCDIR/copyright" "$DOCDIR/changelog.gz"
    rmdir "$DOCDIR" 2>/dev/null || true
    if [ "$have_systemd" = 1 ]; then
        systemctl daemon-reload >/dev/null 2>&1 || true
    fi
    if [ "$PURGE" = 1 ]; then
        rm -rf "$CONFIG_DIR" "$STATE_DIR"
        echo "removed configuration and history"
    else
        echo "kept $CONFIG_DIR and $STATE_DIR (use --purge to remove them)"
    fi
    echo "the 'sb' group was left in place"
    exit 0
fi

for file in sb-daemon sb sb-daemon.service sb.toml README.md; do
    if [ ! -e "$src/$file" ]; then
        echo "missing $src/$file; run this from an extracted release tarball" >&2
        exit 1
    fi
done

install -Dm755 "$src/sb-daemon" "$BINDIR/sb-daemon"
install -Dm755 "$src/sb" "$BINDIR/sb"
if [ -e "$src/migrate-history" ]; then
    install -Dm755 "$src/migrate-history" "$LIBDIR/migrate-history"
fi
install -Dm644 "$src/README.md" "$DOCDIR/README.md"
if [ -e "$src/LICENSE" ]; then
    install -Dm644 "$src/LICENSE" "$DOCDIR/LICENSE"
fi
if [ -e "$src/copyright" ]; then
    install -Dm644 "$src/copyright" "$DOCDIR/copyright"
fi
if [ -e "$src/changelog.gz" ]; then
    install -Dm644 "$src/changelog.gz" "$DOCDIR/changelog.gz"
fi
if [ -e "$src/man/sb.1.gz" ]; then
    install -Dm644 "$src/man/sb.1.gz" "$MANDIR/sb.1.gz"
fi
if [ -e "$src/man/sb-daemon.1.gz" ]; then
    install -Dm644 "$src/man/sb-daemon.1.gz" "$MANDIR/sb-daemon.1.gz"
fi

if ! getent group sb >/dev/null 2>&1; then
    groupadd --system sb 2>/dev/null || addgroup --system sb
fi

if [ -e "$src/sysusers.d/sb.conf" ]; then
    install -Dm644 "$src/sysusers.d/sb.conf" "$SYSUSERS_DIR/sb.conf"
fi

install -d "$CONFIG_DIR"
if [ -e "$CONFIG_DIR/sb.toml" ]; then
    echo "keeping existing $CONFIG_DIR/sb.toml"
else
    install -m644 "$src/sb.toml" "$CONFIG_DIR/sb.toml"
fi

install -d "$UNIT_DIR"
# The package masks the unit on removal; replace any mask link instead of
# writing through it.
rm -f "$UNIT_DIR/sb-daemon.service"
sed -e "s|^ExecStart=.*|ExecStart=$BINDIR/sb-daemon -v|" \
    -e "s|^Documentation=.*|Documentation=file:$DOCDIR/README.md|" \
    "$src/sb-daemon.service" > "$UNIT_DIR/sb-daemon.service"
chmod 0644 "$UNIT_DIR/sb-daemon.service"

if [ "$have_systemd" = 1 ]; then
    systemctl unmask sb-daemon.service >/dev/null 2>&1 || true
    systemctl daemon-reload
    systemctl enable sb-daemon.service >/dev/null 2>&1 || true
    if [ "$DO_START" = 1 ]; then
        if ! systemctl restart sb-daemon.service; then
            echo "warning: could not start sb-daemon; check 'systemctl status sb-daemon'" >&2
        fi
    fi
else
    echo "systemd not found; binaries installed but the service was not started" >&2
fi

echo
echo "installed sb-daemon and sb"
if [ -n "${SUDO_USER:-}" ]; then
    echo "add $SUDO_USER to the socket group: sudo usermod -aG sb $SUDO_USER"
else
    echo "add your user to the socket group: sudo usermod -aG sb <user>"
fi
