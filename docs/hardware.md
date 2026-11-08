# Hardware support

## Verified machine

| | |
|---|---|
| Model | Beelink ME Mini (`AZW` / `ME mini`) |
| CPU | Intel N95 / N150 class |
| Super I/O | ITE IT8613E, revision 12 |
| BIOS | `N5X12002` (2025-12-24) |
| OS | Debian 13 (trixie) |
| Kernel | `6.12.115+deb13.1-amd64` |
| Driver | out-of-tree `it87` via DKMS, version `20f2f2f.20260416` |

The IT8613E is not always handled by the in-tree `it87` module. See
[it87 driver](it87.md) for the DKMS setup used on this machine.

## Channel mapping

hwmon numbers are not stable across boots. Look the chip up by name:

```sh
for h in /sys/class/hwmon/hwmon*; do
    echo "$h: $(cat "$h/name" 2>/dev/null)"
done
```

On the tested ME Mini:

| Purpose | hwmon name | File |
|---|---|---|
| CPU package temperature | `coretemp` | `temp1_input` (`Package id 0`) |
| CPU fan speed | `it8613` | `fan2_input` |
| CPU fan PWM | `it8613` | `pwm2` |
| Manual/auto mode | `it8613` | `pwm2_enable` (`1` manual, `2` automatic) |

Observed values while `sb-daemon` was running:

```
coretemp  temp1_input = 45000   (45 °C package)
it8613    fan2_input  = 1383    (RPM)
it8613    pwm2        = 110     (~43%)
it8613    pwm2_enable = 1       (manual, set by sb-daemon)
it8613    pwm2_freq   = 23437   (Hz)
```

The EC's own automatic curve on this board was exposed as:

```
pwm2_auto_start = 110
pwm2_auto_point1_temp = 25000   (25 °C)
pwm2_auto_point2_temp = 40000   (40 °C)
pwm2_auto_point3_temp = 60000   (60 °C)
pwm2_auto_slope = 64
```

That is why the default `pwm_baseline = 110` feels natural on this machine:
it matches the EC's own starting duty cycle.

## Channels that are not the CPU fan

The same IT8613E exposes `pwm3` and `pwm5`, plus an unconnected `temp3`
reading (often `127 °C` with an alarm). On the tested unit, only
`fan2_input`/`pwm2` corresponds to the running CPU fan. Do not assume
`pwm3` or `pwm5` control it; verify with `sensors` and by changing one
channel at a time while watching `fan2_input`.

## How `sb-daemon` selects its paths

`hwmon::HwmonLocator::discover()` picks:

1. the first `coretemp` hwmon that has `temp1_input`;
2. the first hwmon that has both `pwm2` and `pwm2_enable`;
3. `fan2_input` if present, otherwise `fan1_input`.

Use the diagnostics script before trusting the mapping on a different board:

```sh
sudo bash tools/collect-me-mini.sh > /tmp/me-mini-report.txt 2>&1
sudo sb-daemon --check
```

`--check` prints the selected paths and current values and exits without
touching the fan.

## Other IT87 boards

The daemon is deliberately generic: any hwmon with `pwm2` works. The ME Mini
is the only board validated so far. Other IT8613E/IT8625E mini PCs may have
different fan/PWM routing or require different `it87` options. Check the
driver documentation for your board and confirm the channel mapping first.

## Safety

- The fan is under manual control while `sb-daemon` runs. The service restores
  automatic mode on SIGTERM or a clean stop.
- If the daemon is killed with SIGKILL or the machine loses power, the EC may
  remain in manual mode until the next service start or reboot.
- Start with conservative temperatures and monitor `sensors` for a while.
- `pwm2 = 0` can stop the fan. The default baseline is 110 (~43%); do not lower
  it without checking temperatures under load.
