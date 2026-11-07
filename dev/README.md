# Fake-sysfs harness

This runs the real `sb-daemon` against a fake `hwmon` tree and a fake
`/proc/stat`, with a small thermal plant driving the fake temperature. It
exercises discovery, sampling, the fan state machine, PWM/enable writes,
SIGTERM restore, IPC, the database, and the TUI — everything except the
kernel driver itself.

The daemon is built with the `test-hwmon` cargo feature, which compiles in
`SB_HWMON_ROOT` / `SB_PROC_STAT` overrides. Release builds do not include
them.

## On the host

```sh
./dev/harness.sh --tui   # daemon + plant + TUI
./dev/harness.sh         # headless; Ctrl-C to stop
```

The script creates a scratch directory under `$TMPDIR`, builds both binaries,
starts the daemon with temporary socket and database paths, and runs
`dev/plant.sh`.

The fake tree looks like:

```
hwmon/hwmon0/name          coretemp
hwmon/hwmon0/temp1_input   45000
hwmon/hwmon1/name          n95_ec
hwmon/hwmon1/pwm2          110
hwmon/hwmon1/pwm2_enable   2
hwmon/hwmon1/fan1_input    2000
proc_stat
```

The plant reads `pwm2` and writes a new `temp1_input` every second, so the
fan controller has feedback. It also advances the `/proc/stat` counters.

## Docker

```sh
docker build -f dev/Dockerfile -t sb-harness .
docker run --rm -it sb-harness sb   # TUI
docker run --rm sb-harness          # headless
```

The container creates the fake tree under `/tmp/sb-sim`, listens on
`/run/sb/sb.sock`, and starts the plant. No privileges or `/sys` bind mounts
are required.

## What still needs real hardware

The fake tree cannot tell you whether this machine's EC treats
`pwm2_enable = 2` as automatic control, whether it accepts PWM writes, or how
its sensors are scaled. Those need a short smoke test on the N95.
