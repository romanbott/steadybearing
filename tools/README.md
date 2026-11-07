# Tools

## migrate-history.sh

One-off import of the old `telemetry.db` / `archive.db` history into the unified
`/var/lib/sb/history.db` schema.

```sh
sudo systemctl stop sb-daemon
sudo ./tools/migrate-history.sh
sudo systemctl start sb-daemon
```

Use `--telemetry`, `--archive`, `--output` to point at custom paths, and
`--force` if the destination already has history. The migration is idempotent
and only copies complete buckets.

Notes:

- Minute rows are imported into `rollups_1m` with `sample_count = 60`; hourly
  rows into `rollups_1h` with `sample_count = 3600`. The old data did not store
  exact counts, so these are approximations for future weighted means.
- The 10 m tier is backfilled from the imported minute rows on the next daemon
  run. The 10 s tier starts empty.
- Back up the old files first; the migration only reads them.
