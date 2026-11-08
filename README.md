# SteadyBearing

[![CI](https://github.com/romanbott/steadybearing/actions/workflows/ci.yml/badge.svg)](https://github.com/romanbott/steadybearing/actions/workflows/ci.yml)
[![Release](https://img.shields.io/github/v/release/romanbott/steadybearing)](https://github.com/romanbott/steadybearing/releases)
[![License: GPL-3.0-or-later](https://img.shields.io/badge/license-GPL--3.0--or--later-blue.svg)](LICENSE)

A small fan-control daemon and terminal dashboard for Linux machines whose
embedded controller exposes a fan through `hwmon` (`pwm2`). It was written for
the Beelink ME Mini (Intel N95/N150, ITE IT8613E), but works on any IT87-class
board with a controllable `pwm2` channel.

- `sb-daemon` samples CPU temperature, CPU usage, fan RPM and PWM, runs a
  hysteresis fan curve with a hard-max ramp and recovery, and stores tiered
  history in SQLite.
- `sb` is a Ratatui TUI that streams live samples and browses 10 s / 1 m /
  10 m / 1 h history.
- The daemon restores automatic fan control on shutdown or SIGTERM.

See [hardware](docs/hardware.md) and [it87 driver](docs/it87.md) for the
ME Mini setup. Fan control is inherently risky — read the safety notes before
running it on a new machine.

## Features

- 1 Hz sampling of package temperature, CPU usage, fan RPM and PWM.
- Hysteresis curve with dwell times, baseline floor, hard-max ramp and
  controlled recovery.
- Tiered history in one SQLite database:
  - 10 s for 1 day,
  - 1 m for 7 days,
  - 10 m forever,
  - 1 h forever.
- Auxiliary NVMe and IT8613 board temperature telemetry for tuning (stored in
  the same database; not shown in the TUI yet).
- Live TUI with raw/MA/min/max views, grids, and historical navigation.
- Local Unix socket (`0660`, group `sb`) with a protocol version handshake.
- systemd unit with `StateDirectory`/`RuntimeDirectory` and conservative
  hardening.
- Debian package and portable tarball.

## Verified hardware

| | |
|---|---|
| Machine | Beelink ME Mini (`AZW` / `ME mini`), BIOS `N5X12002` |
| CPU | Intel N95 / N150 class |
| Super I/O | ITE IT8613E, revision 12 |
| Linux | Debian 13 (trixie), kernel 6.12.115 |
| Driver | out-of-tree `it87` (frankcrawford/it87) via DKMS |

The CPU fan is `fan2_input`, controlled by `pwm2`. See
[hardware](docs/hardware.md) for the channel map and observed values.

## Requirements

- Linux with systemd.
- `/sys/class/hwmon` with:
  - a `coretemp` device providing `temp1_input`;
  - an `it87`-class device providing `pwm2` and `pwm2_enable`.
- On the ME Mini, the out-of-tree `it87` driver — see
  [docs/it87.md](docs/it87.md).
- Debian 13+, or Debian 12+ using a release built on bookworm. Releases are
  built against bookworm (glibc 2.36) wherever possible.

## Install

### Debian / Ubuntu

Download the `.deb` from the [releases page](https://github.com/romanbott/steadybearing/releases)
and install it:

```sh
sudo apt install ./sb_<version>_amd64.deb
```

### Other systemd distributions

```sh
tar -xzf sb-<version>-x86_64-linux.tar.gz
sudo ./sb-<version>-x86_64-linux/install.sh
```

Both install `sb-daemon`, `sb`, the systemd unit, the default configuration,
and man pages.

Add your user to the socket group, then start a new login session:

```sh
sudo usermod -aG sb "$USER"
```

### The it87 driver

The ME Mini needs the out-of-tree `it87` module to expose `pwm2`. Install it
before starting the daemon and verify the chip appears:

```sh
dmesg | grep -i it87     # Found IT8613E chip at 0xa30, revision 12
sensors                  # it8613-isa-* with fan2 / pwm2
```

Full instructions, module options, Secure Boot notes and removal steps are in
[docs/it87.md](docs/it87.md).

## Quick start

```sh
sudo systemctl status sb-daemon
sudo systemctl restart sb-daemon
sudo journalctl -u sb-daemon -f
sb
```

`sb-daemon --check` discovers the hwmon paths and prints the current
`temp1_input`, `pwm2`, `pwm2_enable`, fan and auxiliary NVMe/board values
without touching the fan.

## Configuration

Edit `/etc/sb/sb.toml`. The packaged file is a conffile and is never
overwritten by upgrades.

The most important settings are `pwm_baseline`, `fan_soft_min`,
`fan_soft_max`, `fan_hard_max`, `fan_step`, and the history retention under
`[retention]`. See [docs/configuration.md](docs/configuration.md) for every
field and the fan curve.

## TUI keys

| Key | Action |
|---|---|
| `t` | Cycle live → 10 s → 1 m → 10 m → 1 h |
| `m` | Cycle raw / 10 s moving average / 10 s min / 10 s max (live), or mean/min/max (history) |
| `g` | Toggle the chart grid |
| `←` / `→` | Move through historical data |
| `q`, `Esc` | Quit |

## History and backup

Telemetry lives in `/var/lib/sb/history.db`. The 10 s and 1 m tiers are pruned
automatically; 10 m and 1 h are kept forever. Auxiliary NVMe/board temperatures
are stored in `aux_rollups_10s` and kept for `[retention] aux_days` (default
14 days). See [configuration](docs/configuration.md) for the schema and a
query example.

For a consistent backup, stop the service and copy the file:

```sh
sudo systemctl stop sb-daemon
sudo cp /var/lib/sb/history.db /var/backups/history-$(date +%F).db
sudo systemctl start sb-daemon
```

## Troubleshooting

Common issues (`pwm2 hwmon not found`, socket permissions, glibc mismatch,
old TUI/new daemon, empty history, fan not changing) are covered in
[docs/troubleshooting.md](docs/troubleshooting.md).

For ME Mini / IT8613E reports, run the read-only collector and attach its
output:

```sh
sudo bash tools/collect-me-mini.sh > /tmp/me-mini-report.txt 2>&1
```

It does not write to sysfs or change fan settings.

## Development

```sh
cargo test --workspace
./dev/harness.sh --tui     # fake sysfs + simulated thermal plant
make dist                  # .deb, tarball and checksums (needs nfpm)
```

The fake-sysfs harness builds the daemon with the dev-only `test-hwmon`
feature; release builds do not include those overrides.

Release builds are made in a `debian:bookworm` environment so the binaries
run on Debian 12/13 and Ubuntu 24.04. The package/CI details are in
[packaging/README.md](packaging/README.md).

## License

SteadyBearing is free software: you can redistribute it and/or modify it under
the terms of the GNU General Public License as published by the Free Software
Foundation, either version 3 of the License, or (at your option) any later
version.

See [LICENSE](LICENSE) for the full text. If you distribute compiled binaries,
the GPL requires you to provide the corresponding source code.

## Contributing

Issues, hardware reports and patches are welcome. See
[CONTRIBUTING.md](CONTRIBUTING.md). Security-sensitive reports should follow
[SECURITY.md](SECURITY.md).
