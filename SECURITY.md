# Security policy

## Reporting

Please report security issues privately rather than opening a public issue.
Use the maintainer email listed in `Cargo.toml` / `nfpm.yaml`
(`roman.contreras@ciencias.unam.mx`) with a description and, if possible, a
reproduction.

## Scope

SteadyBearing runs as root and writes to hwmon sysfs files. Areas worth
reporting include:

- unsafe or unsound code;
- IPC parsing/deserialization issues or denial of service over the local
  socket;
- socket ownership or group-permission mistakes;
- systemd unit hardening gaps;
- anything that could leave the fan stopped or in manual mode unexpectedly.

The daemon uses an AF_UNIX socket at `/run/sb/sb.sock` with mode `0660` and no
network listener. The TUI and daemon must be from the same release; the
protocol version handshake rejects mismatches.

## Supported versions

Security fixes target the latest release. Please mention the version, Debian
version, kernel, and `it87` driver version in the report.
