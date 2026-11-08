# Configuration

The daemon reads `/etc/sb/sb.toml` (or the path passed to `--config-file`).
The packaged file is a conffile, so upgrades do not overwrite local changes.
CLI flags override the file, which overrides the built-in defaults.

## Paths and socket

```toml
socket_path = "/run/sb/sb.sock"
# socket_gid = 1234
db_path = "/var/lib/sb/history.db"
db_flush_secs = 60
```

| Field | Default | Meaning |
|---|---|---|
| `socket_path` | `/run/sb/sb.sock` | Unix socket used by the TUI |
| `socket_gid` | unset | optional numeric group to own the socket; otherwise the daemon's effective group is used |
| `db_path` | `/var/lib/sb/history.db` | unified history database |
| `db_flush_secs` | `60` | how often complete buckets are written (1–3600) |

The socket is created with mode `0660`. Users who run `sb` must be members of
the owning group (normally `sb`). The systemd unit uses `Group=sb`, and the
package/installer creates that group.

## History retention

```toml
[retention]
ten_second_days = 1
minute_days = 7
```

| Tier | Table | Default retention |
|---|---|---|
| 10 s | `rollups_10s` | 1 day |
| 1 m | `rollups_1m` | 7 days |
| 10 m | `rollups_10m` | forever |
| 1 h | `rollups_1h` | forever |

`db_flush_secs` controls how often the 10 s/1 m buckets are written and how
often the coarser tiers advance. A smaller value makes the 10 s view fresher
at the cost of more writes.

## Fan control

```toml
pwm_baseline = 110
fan_soft_min = 40.0
fan_soft_max = 60.0
fan_hard_max = 80.0
fan_dwell_secs = 10.0
fan_step = 5
fan_hard_ramp_secs = 10.0
fan_drift_temp = 50.0
fan_drift_secs = 30.0
```

| Field | Default | Meaning |
|---|---|---|
| `pwm_baseline` | `110` | lowest PWM the controller will use while idling |
| `fan_soft_min` | `40.0` | below this, step down toward the baseline |
| `fan_soft_max` | `60.0` | above this, step up by `fan_step` |
| `fan_hard_max` | `80.0` | ramp to full speed over `fan_hard_ramp_secs` |
| `fan_dwell_secs` | `10.0` | seconds a condition must hold before a step |
| `fan_step` | `5` | PWM units per step |
| `fan_hard_ramp_secs` | `10.0` | ramp time to 255 at `fan_hard_max` |
| `fan_drift_temp` | `50.0` | below this, slowly drift back toward baseline |
| `fan_drift_secs` | `30.0` | seconds per 1-unit drift step |

The controller uses a 10-second moving average of the CPU package temperature:

- **above `fan_soft_max`**: after `fan_dwell_secs`, increase PWM by `fan_step`;
- **below `fan_soft_min`**: after `fan_dwell_secs`, decrease PWM by `fan_step`,
  never below `pwm_baseline`;
- **between `fan_drift_temp` and `fan_soft_max`**: hold (hysteresis band);
- **below `fan_drift_temp`**: drift down by 1 PWM unit every `fan_drift_secs`;
- **at or above `fan_hard_max`**: ramp linearly from the current value to 255
  over `fan_hard_ramp_secs`;
- after a hard-max event, a recovery state steps back down toward the baseline
  instead of holding 100%.

`fan_step` must be at least 1. The daemon validates the thresholds at startup
and refuses nonsensical configurations.

## CLI

```text
sb-daemon [--socket-path PATH] [--db-path PATH] [--config-file PATH]
          [--verbose] [--check]
```

`--check` discovers the hwmon paths and prints the current `coretemp`, `pwm2`,
`pwm2_enable` and fan values, then exits without changing the fan. Use it when
setting up a new machine.

```text
sb [--socket-path PATH]
```

The TUI only needs the socket. Press `t` to cycle live → 10 s → 1 m → 10 m →
1 h, `m` to change the display statistic, `g` to toggle the grid, arrows to
move in history, and `q` to quit.
