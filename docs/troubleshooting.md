# Troubleshooting

## `pwm2 hwmon not found` / daemon exits on startup

The daemon needs an `it87` hwmon with `pwm2` and `pwm2_enable`.

```sh
uname -r
lsmod | grep -E 'it87|coretemp'
dkms status
dmesg | grep -i it87
ls /sys/class/hwmon
for h in /sys/class/hwmon/hwmon*; do echo "$h $(cat "$h/name" 2>/dev/null)"; done
```

- `it87` not loaded: see [it87 driver](it87.md).
- `Found IT8613E chip` missing from `dmesg`: the module did not bind. Check
  headers, DKMS, and the resource-conflict option.
- `pwm2` missing: the board may expose a different channel; run
  [`tools/collect-me-mini.sh`](../tools/collect-me-mini.sh) and inspect the
  full hwmon inventory.
- `modinfo -n it87` should point at `.../updates/dkms/it87.ko*`; if it points
  at the in-tree module, the DKMS module is not installed for the running
  kernel.

`sensors` should show `it8613-isa-*`, `fan2`, and `pwm2`.

## it87 missing after a kernel update

DKMS needs the headers for the new kernel. If `it87` or `pwm2` disappears after
an update:

```sh
uname -r
dkms status
dmesg | grep -i it87
```

Install the headers meta-package so future kernel updates keep the matching
headers:

```sh
sudo apt install linux-headers-amd64
sudo dkms autoinstall
sudo modprobe it87
```

Then check again:

```sh
lsmod | grep it87
dmesg | grep -i it87
```

Installing `linux-headers-$(uname -r)` once only covers the current kernel;
`linux-headers-amd64` is the meta-package that follows kernel updates. See
[it87 driver](it87.md#kernel-updates-and-headers).

## `sb: cannot connect to sb-daemon`

```sh
systemctl status sb-daemon
journalctl -u sb-daemon -n 50
ls -l /run/sb/sb.sock
```

The socket is `0660 root:sb`. Your user must be in the `sb` group:

```sh
sudo usermod -aG sb "$USER"
# log out and back in, or use `newgrp sb` in the current shell
groups
```

If `/run/sb/sb.sock` is missing, the daemon is not running.

## `GLIBC_2.41 not found` or similar

The binary was built on a newer Debian than the target. Build the release on
the oldest Debian/glibc you need to support (bookworm for Debian 12/13 and
Ubuntu 24.04), or use the tarball built for that baseline.

## The TUI reports a protocol mismatch

The daemon and TUI must be from the same release. Install both from the same
`.deb`/tarball; an old `sb` left in `/usr/local/bin` can shadow the packaged
`/usr/bin/sb`. Check:

```sh
type -a sb
```

## History is empty

- The history database is `/var/lib/sb/history.db`. Check it exists.
- The first flush is after `db_flush_secs` (default 60 s); the 10 m tier needs
  a complete 10-minute bucket, and the 1 h tier a complete hour.
- Check that `db_path` in `/etc/sb/sb.toml` points at the file you expect.

## The fan does not change speed

Confirm the mapping before changing anything:

```sh
H=$(grep -l it8613 /sys/class/hwmon/*/name | xargs dirname)
cat "$H/pwm2_enable"   # 1 manual, 2 automatic
cat "$H/pwm2"
cat "$H/fan2_input"
```

- `pwm2_enable = 2`: the EC is in control; `sb-daemon` is not running or did
  not take over.
- `pwm2` changes but `fan2_input` does not: you may be looking at the wrong
  channel. On the ME Mini only `fan2`/`pwm2` represents the CPU fan; `pwm3`
  and `pwm5` are separate channels.
- Some BIOS/EC versions override manual PWM. Check the BIOS fan mode and the
  `it87` module options.

## `temp3 = 127 °C` with an alarm

On the tested ME Mini, `it8613` `temp3_input` is unconnected and reads
127 °C with an alarm. It is not a real temperature. The daemon uses the
`coretemp` package sensor (`temp1_input`), not the it87 temperatures.

## Fan is stuck after a crash or SIGKILL

The daemon restores `pwm2_enable = 2` on a clean stop. If it was killed
abruptly, restore automatic mode manually:

```sh
H=$(grep -l it8613 /sys/class/hwmon/*/name | xargs dirname)
echo 2 | sudo tee "$H/pwm2_enable"
```

Then start the service again. A reboot also returns the EC to its firmware
default.

## Collecting a report

For ME Mini / IT8613E issues, run the read-only collector and attach its
output:

```sh
sudo bash tools/collect-me-mini.sh > /tmp/me-mini-report.txt 2>&1
```

It includes the model, kernel, `it87` parameters, DKMS state, hwmon channels,
kernel messages and the current service configuration. It does not write to
sysfs or change fan settings.
