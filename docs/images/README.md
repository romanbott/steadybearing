# Demo assets

| File | Purpose |
|---|---|
| `sb-history.gif` | README hero; 10 s / 1 m / 10 m / 1 h views from a real ME Mini history DB |
| `sb-history.png` | Static fallback; the 1 m history view |

The history views are real data from a Beelink ME Mini. The live view is not
shown in the tape because the demo uses a static fake hwmon seeded from the
last real sample; showing history avoids implying that the live values are
recorded from the machine.

The history DB is **not** committed. `dev/demo.tape` reads its path from
`SB_DEMO_DB`.

## Prerequisites

VHS is not in Debian bookworm apt, and `ttyd` is not either. On Debian:

```sh
sudo apt install ffmpeg fonts-jetbrains-mono gifsicle chromium

# ttyd static binary
curl -fsSL -o /usr/local/bin/ttyd \
  https://github.com/tsl0922/ttyd/releases/download/1.7.7/ttyd.x86_64
chmod +x /usr/local/bin/ttyd

# VHS release package, see
# https://github.com/charmbracelet/vhs/releases/latest
curl -fsSLO https://github.com/charmbracelet/vhs/releases/download/v0.12.1/vhs_0.12.1_amd64.deb
sudo dpkg -i vhs_0.12.1_amd64.deb

fc-cache -f
```

VHS uses a headless Chromium. In a root/LXC container it may need:

```sh
export VHS_NO_SANDBOX=1
```

The Debian `chromium` package provides the shared libraries VHS's bundled
browser needs.

## Regenerate

```sh
export SB_DEMO_DB=/path/to/history.db
vhs dev/demo.tape

# optional: shrink the GIF further
gifsicle -O3 --lossy=80 docs/images/sb-history.gif \
  -o docs/images/sb-history.gif
```

The tape hides the build and harness setup, then records the 10 s, 1 m, 10 m
and 1 h history views. It takes a PNG screenshot at the 1 m view.

The harness copies the DB to a temporary directory and sets a long
`db_flush_secs` so the demo does not append simulated rows to the source DB.
The source file is never modified.
