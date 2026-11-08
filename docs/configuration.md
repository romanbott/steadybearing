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
aux_days = 14
```

| Tier | Table | Default retention |
|---|---|---|
| 10 s | `rollups_10s` | 1 day |
| 1 m | `rollups_1m` | 7 days |
| 10 m | `rollups_10m` | forever |
| 1 h | `rollups_1h` | forever |
| aux 10 s | `aux_rollups_10s` | 14 days |

`db_flush_secs` controls how often the 10 s/1 m buckets are written and how
often the coarser tiers advance. A smaller value makes the 10 s view fresher
at the cost of more writes.

## Fan control

```toml
pwm_baseline = 110
fan_soft_min = 40.0
fan_soft_max = 60.0
fan_hard_max = 80.0
fan_hard_exit_temp = 70.0
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
| `fan_hard_exit_temp` | `70.0` | after a hard-max event, leave 100% below this temperature |
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
- after a hard-max event, hold 100% until the average falls below
  `fan_hard_exit_temp`, then a recovery state steps back down toward the
  baseline. Keeping `fan_hard_exit_temp` separate from `fan_soft_max` avoids
  holding 100% all the way down to the normal ramp threshold.

`fan_step` must be at least 1. The daemon validates the thresholds at startup
and refuses nonsensical configurations, including a `fan_drift_temp` outside
`fan_soft_min..fan_soft_max` or a `fan_hard_exit_temp` outside
`fan_soft_max..fan_hard_max`.

## Auxiliary thermal telemetry

The daemon also samples NVMe and IT8613 board temperatures without using them
for fan control. They are stored in the same database:

- `aux_sensors`: sensor metadata (`id`, kind, label, model, serial, PCI BDF,
  first/last seen)
- `aux_rollups_10s`: 10 s min/max/mean per sensor

NVMe sensors are discovered from `hwmon` devices named `nvme` and recorded per
drive and per temperature input (Composite, Sensor 1, ...). The IT8613 board
sensors `temp1` and `temp2` are recorded; the unconnected `temp3` is skipped.
Values outside −40…125 °C are ignored, which also filters the 127 °C
unconnected reading.

This data is intended for tuning. The TUI does not display it yet, and it does
not affect the fan curve. Set `[retention] aux_days` to control how long it is
kept (default 14 days). You can inspect it with SQLite, for example:

```sh
sqlite3 /var/lib/sb/history.db \
  'SELECT s.label, r.bucket_ts, r.temp_mean / 100.0
     FROM aux_rollups_10s r JOIN aux_sensors s ON s.id = r.sensor_id
    ORDER BY r.bucket_ts DESC LIMIT 20;'
```

`--check` also prints the discovered auxiliary sensors and their current
values, without changing anything.

## CLI

```text
sb-daemon [--socket-path PATH] [--db-path PATH] [--config-file PATH]
          [--verbose] [--check]
```

`--check` discovers the hwmon paths and prints the current `coretemp`, `pwm2`,
`pwm2_enable`, fan and auxiliary NVMe/board values, then exits without
changing the fan. Use it when setting up a new machine.

```text
sb [--socket-path PATH]
```

The TUI only needs the socket. Press `t` to cycle live → 10 s → 1 m → 10 m →
1 h, `m` to change the display statistic, `g` to toggle the grid, arrows to
move in history, and `q` to quit.
