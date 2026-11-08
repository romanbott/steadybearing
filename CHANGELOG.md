# Changelog

All notable changes to this project are documented here. The format is based
on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/), and the project
follows [Semantic Versioning](https://semver.org/).

## [0.1.1] - 2026-11-07

### Removed

- The one-off `migrate-history` helper for the pre-release two-database
  layout, along with the installer's `--libdir` option.

### Fixed

- Man pages and `--help` now document `--check`, `--version`,
  `--config-file` and `-v`.

## [0.1.0] - 2026-11-07

### Added

- `sb-daemon` fan controller with a 10 s moving average, hysteresis dwell
  times, a baseline PWM floor, a hard-max ramp and a controlled recovery.
- `sb` Ratatui dashboard with live and historical views.
- Tiered SQLite history: 10 s (1 day), 1 m (7 days), 10 m (forever), 1 h
  (forever).
- Local Unix socket with group ownership and a protocol version handshake.
- systemd unit with `StateDirectory`/`RuntimeDirectory` and hardening.
- Debian package, portable tarball, man pages and a one-off migration tool.
- Fake-sysfs harness and a read-only ME Mini diagnostics collector.
- Me Mini / IT8613E hardware and it87 driver documentation.

[Unreleased]: https://github.com/romanbott/steadybearing/compare/v0.1.1...HEAD
[0.1.1]: https://github.com/romanbott/steadybearing/compare/v0.1.0...v0.1.1
[0.1.0]: https://github.com/romanbott/steadybearing/releases/tag/v0.1.0
