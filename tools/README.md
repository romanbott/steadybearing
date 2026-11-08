# Tools

## collect-me-mini.sh

Read-only diagnostics for Beelink ME Mini / IT8613E fan-control setups. It
collects the machine model, kernel, `it87` module parameters, DKMS state,
hwmon channels, `pwm2`/`fan2_input` values, kernel messages and the current
`sb` service configuration.

```sh
sudo bash tools/collect-me-mini.sh > /tmp/me-mini-report.txt 2>&1
```

It does not write to sysfs, load/unload modules, or change fan settings.
