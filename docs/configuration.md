# Configuration

The daemon reads `/etc/sb/sb.toml` (or the path passed to `--config-file`).
The packaged file is a conffile, so upgrades do not overwrite local changes.
CLI flags override the file, which overrides the built-in defaults.

Top-level keys are daemon and fan settings. Tables such as `[retention]` must
come after them; fan keys written inside `[retention]` are ignored. The daemon
logs a warning if it finds fan keys under `[retention]`. Keep the `[retention]`
table at the bottom of the file.

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
# Keep this table after all top-level fan/path keys.
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

## Fan-control goals

The default curve is deliberately conservative and slow:

- Prefer infrequent, small changes over tight temperature tracking. The 10 s
  moving average, dwell time and 5-unit steps keep the fan from hunting.
- Hold the CPU in a mid band. The default hold band is 50–60 °C; the fan only
  ramps above 60 °C and treats 80 °C as a safety threshold, not an operating
  point.
- Keep a continuous baseline airflow. `pwm_baseline = 110` is the EC's own
  start value (~43%, ~1380 RPM on the ME Mini) and provides airflow over the
  NVMe drives, VRM and board.
- Deprioritize noise. A silent profile would lower the baseline and raise the
  thresholds; this one does the opposite.
- Keep the CPU fan's wear low while protecting the more expensive components.
  The curve avoids stop/start cycles and rapid changes.
- The curve is CPU-only today. NVMe and IT8613 temperatures are logged for
  tuning; they do not raise the fan. With several NVMe drives, the baseline is
  the only current lever.

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

## What each knob is for

| Knob | Goal | Trade-off |
|---|---|---|
| `pwm_baseline` | continuous airflow floor | lower = less fan wear/noise, warmer components; 110 matches the EC and helps the NVMe/VRM |
| `fan_soft_min` | lower edge of the hold band | lower = slows sooner; higher = keeps speed longer |
| `fan_drift_temp` | start of the slow cool-down | higher = sheds speed sooner, but can cause more changes |
| `fan_soft_max` | normal ramp threshold | lower = cooler, more fan speed and wear; higher = warmer, less wear |
| `fan_hard_exit_temp` | release 100% after a hard-max | lower = longer 100%, cooler; higher = shorter 100%, warmer |
| `fan_hard_max` | safety ramp threshold | lower = earlier 100%, cooler; higher = later |
| `fan_dwell_secs` | how long a condition must hold | higher = slower and fewer changes |
| `fan_step` | PWM per step | higher = faster and larger changes |
| `fan_hard_ramp_secs` | time to reach 255 | higher = gentler but slower |
| `fan_drift_secs` | seconds per 1-unit cool-down | higher = keeps elevated speed much longer |

`fan_drift_secs = 30` is very slow: if a load leaves the fan at PWM 200 and the
CPU settles at 45 °C, it takes about 45 minutes to return to 110. Lower it to
10–15 s if you want less unnecessary fan wear without making the curve twitchy.

## Example profiles

These are starting points, not guarantees. Edit `/etc/sb/sb.toml`, restart the
daemon, and monitor `sensors`, `sb` history and the NVMe SMART data. The
packaged file is a conffile, so upgrades do not overwrite your edits.

### Default / EC-aligned

The shipped defaults. Matches the ME Mini's own 40 °C/60 °C EC points and keeps
a 50–60 °C hold band.

```toml
pwm_baseline = 110
fan_soft_min = 40.0
fan_drift_temp = 50.0
fan_soft_max = 60.0
fan_hard_exit_temp = 70.0
fan_hard_max = 80.0
fan_dwell_secs = 10.0
fan_step = 5
fan_hard_ramp_secs = 10.0
fan_drift_secs = 30.0
```

### Slow and steady

Smaller steps and a longer dwell, with a slightly earlier ramp and a shorter
100% hold. Good when fan-life and smoothness matter more than response time.

```toml
pwm_baseline = 110
fan_soft_min = 40.0
fan_drift_temp = 48.0
fan_soft_max = 58.0
fan_hard_exit_temp = 70.0
fan_hard_max = 78.0
fan_dwell_secs = 15.0
fan_step = 4
fan_hard_ramp_secs = 15.0
fan_drift_secs = 12.0
```

### Component-first / cooler

More baseline airflow and lower thresholds. This keeps the CPU, board and NVMe
drives cooler at the cost of more fan speed and wear. It is the closest match
to “noise does not matter, protect the components”.

```toml
pwm_baseline = 130
fan_soft_min = 38.0
fan_drift_temp = 45.0
fan_soft_max = 54.0
fan_hard_exit_temp = 68.0
fan_hard_max = 74.0
fan_dwell_secs = 10.0
fan_step = 5
fan_hard_ramp_secs = 10.0
fan_drift_secs = 10.0
```

### Quiet / low-wear

Lower baseline and higher thresholds. **Not recommended for a ME Mini with
several NVMe drives without monitoring**: the CPU can stay cool while the
drives and board get hot. It is useful on boards where noise matters and the
NVMe/board temperatures are known to be low.

```toml
pwm_baseline = 80
fan_soft_min = 45.0
fan_drift_temp = 55.0
fan_soft_max = 68.0
fan_hard_exit_temp = 76.0
fan_hard_max = 86.0
fan_dwell_secs = 20.0
fan_step = 3
fan_hard_ramp_secs = 20.0
fan_drift_secs = 20.0
```

### Six-drive NAS / experimental

A speculative profile for a fully populated ME Mini. The CPU fan is the only
current airflow lever, so this raises the floor and ramps earlier. Watch NVMe
and board temperatures before trusting it.

```toml
pwm_baseline = 140
fan_soft_min = 38.0
fan_drift_temp = 45.0
fan_soft_max = 52.0
fan_hard_exit_temp = 66.0
fan_hard_max = 72.0
fan_dwell_secs = 10.0
fan_step = 5
fan_hard_ramp_secs = 10.0
fan_drift_secs = 10.0
```

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

## Tuning from data

The auxiliary telemetry and `daemon_runs` table exist to make tuning
data-driven. Start from the default profile, change one thing at a time, and
give it hours or days.

The useful data is in:

- `rollups_10s` / `rollups_1m` / `rollups_10m` / `rollups_1h` — CPU temperature,
  CPU usage, fan RPM and PWM at each resolution
- `aux_rollups_10s` — NVMe and board temperatures at 10 s resolution
- `daemon_runs` — one row per daemon start with the full config snapshot

For example:

```sh
sqlite3 /var/lib/sb/history.db \
  'SELECT version, datetime(started_at, "unixepoch"), config_json
     FROM daemon_runs ORDER BY id DESC LIMIT 5;'
```

When comparing profiles, keep the load similar and check:

- time spent above the soft/hard thresholds;
- how often the fan changes and how long it stays at 100%;
- whether NVMe/board temperatures rise while the CPU stays cool;
- how quickly the fan returns to the baseline after a load.

One change at a time avoids mixing effects.

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
