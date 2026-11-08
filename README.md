# SteadyBearing

A small fan-control daemon and terminal dashboard for Linux machines whose
EC exposes a fan through `hwmon` (`pwm2`), such as Intel N95 mini PCs.

- `sb-daemon` samples CPU temperature, CPU usage, fan RPM and PWM, runs a
  hysteresis fan curve, and stores historical telemetry in SQLite.
- `sb` is a Ratatui TUI that streams live samples and browses the stored
  10 s / 1 m / 10 m / 1 h history.
- The daemon restores automatic fan control on shutdown or SIGTERM.

## Requirements

- Linux with systemd
- `/sys/class/hwmon` with a `coretemp` device and an EC `pwm2` channel
- Debian 13+ (or another systemd distribution for the tarball install).
  For Debian 12 (bookworm), build the release artifacts in a bookworm
  container; see `packaging/README.md`.

## Install

### Debian / Ubuntu

```sh
sudo apt install ./dist/sb_<version>_amd64.deb
```

### Other systemd distributions

```sh
tar -xzf dist/sb-<version>-x86_64-linux.tar.gz
sudo ./sb-<version>-x86_64-linux/install.sh
```

Both install `sb-daemon`, `sb`, the systemd unit, and the default
configuration. Add your user to the socket group, then start a new login
session so the group membership applies:

```sh
sudo usermod -aG sb "$USER"
```

## Configuration

`/etc/sb/sb.toml` (installed as a conffile, never overwritten by upgrades).
See the comments in that file for the fan thresholds, retention windows, and
paths.

The daemon listens on `/run/sb/sb.sock` (mode `0660`, group `sb`).

## Service

```sh
systemctl status sb-daemon
journalctl -u sb-daemon -f
sudo systemctl restart sb-daemon
```

## History

Telemetry lives in `/var/lib/sb/history.db`:

| Resolution | Retention |
|---|---|
| 10 s | 1 day |
| 1 m | 7 days |
| 10 m | forever |
| 1 h | forever |

If you have data from the older `telemetry.db` / `archive.db` layout, run the
one-off importer before starting the daemon:

```sh
sudo /usr/lib/sb/migrate-history --help          # package install
sudo /usr/local/lib/sb/migrate-history --help    # tarball install
```

## Development

```sh
cargo test --workspace
./dev/harness.sh --tui   # fake sysfs + simulated thermal plant
make dist                # .deb, tarball and checksums (requires nfpm)
```

The fake-sysfs harness builds the daemon with the dev-only `test-hwmon`
feature; release builds do not include those overrides.
