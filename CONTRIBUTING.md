# Contributing

Thanks for taking a look at SteadyBearing.

## Development

```sh
cargo test --workspace --locked
cargo clippy --workspace --all-targets -- -D warnings
```

The fake-sysfs harness runs the real daemon against a temporary hwmon tree and
a simulated thermal plant:

```sh
./dev/harness.sh --tui
```

It builds the daemon with the dev-only `test-hwmon` feature. Release builds do
not include that feature.

## Demo GIF

The README demo is generated with VHS from a copy of a real history DB. The DB
is never committed; the harness copies it to a temporary directory.

```sh
export SB_DEMO_DB=/path/to/history.db
vhs dev/demo.tape
```

Prerequisites, the LXC/root workaround (`VHS_NO_SANDBOX=1`) and the asset
details are in [docs/images/README.md](docs/images/README.md).

Packaging:

```sh
make dist
```

This needs `nfpm`; the resulting `.deb` and tarball are written to `dist/`.
See [packaging/README.md](packaging/README.md).

## Hardware reports

Hardware-specific fixes are much easier with real data. On a new machine, run:

```sh
sudo bash tools/collect-me-mini.sh > /tmp/me-mini-report.txt 2>&1
sudo sb-daemon --check
```

Include the model, BIOS, IT87 chip, driver version and the hwmon
`pwm*`/`fan*` mapping in the issue. Review the report before posting it; it
contains DMI strings and kernel logs.

## Commits and pull requests

- Keep commits focused and describe the change in a short semantic subject,
  for example `fix: restore terminal on early exit paths`.
- Add tests for pure logic where practical.
- Do not include generated artifacts, local databases, or diagnostics reports.
- Keep `cargo test` and `cargo clippy -- -D warnings` green.

## License

The project is licensed under GPL-3.0-or-later. By contributing, you agree
that your changes are licensed under the same terms.
