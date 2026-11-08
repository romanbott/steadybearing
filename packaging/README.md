# Packaging

This directory contains the pieces used to build the Debian package and the
portable release tarball.

## Prerequisites

- Rust toolchain (`cargo build --locked`)
- [nfpm](https://nfpm.goreleaser.com) for the `.deb`
- Debian bookworm (or a `debian:bookworm` container) for binaries compatible
  with the target glibc

> Build the release binaries in the oldest Debian/glibc environment you need
> to support. Building on a newer host (for example Fedora) can produce
> binaries that require a newer glibc than the target has.

## Build artifacts

```sh
make dist
```

produces:

```
dist/sb_<version>_amd64.deb
dist/sb-<version>-x86_64-linux.tar.gz
dist/SHA256SUMS
```

Both artifacts contain `sb-daemon`, `sb`, the systemd unit, the default
`sb.toml`, the sysusers file, man pages, a changelog/copyright file, and the
one-off `migrate-history` helper (`/usr/lib/sb/migrate-history` in the
package, `PREFIX/lib/sb/migrate-history` for the tarball). The package
installs the unit under `/usr/lib/systemd/system`; the tarball installer uses
`/etc/systemd/system`.

The project is licensed under GPL-3.0-or-later; see `../LICENSE`. The Debian
`copyright` file points at `/usr/share/common-licenses/GPL-3`, and `nfpm.yaml`
carries the same SPDX identifier.

## Debian

```sh
sudo apt install ./dist/sb_<version>_amd64.deb
sudo usermod -aG sb "$USER"
```

The package installs `/usr/bin/sb-daemon` and `/usr/bin/sb`, enables
`sb-daemon.service`, and installs `/etc/sb/sb.toml` as a conffile. The
maintainer scripts create the `sb` group, enable/restart the unit, and stop it
before removal.

## Portable tarball

```sh
tar -xzf dist/sb-<version>-x86_64-linux.tar.gz
sudo ./sb-<version>-x86_64-linux/install.sh
```

The script defaults to `/usr/local`, patches the unit's `ExecStart`, installs
the config only if it does not exist, creates the `sb` group, and enables the
service. Useful options: `--prefix`, `--bindir`, `--libdir`, `--no-start`,
`--uninstall`, `--purge`.

## Notes

- `make install` stages the binaries and runs the tarball installer from the
  repository. `INSTALL_ARGS` is passed through, e.g.
  `make install INSTALL_ARGS="--prefix /usr"`.
- `make uninstall` runs `packaging/install.sh --uninstall`; add
  `INSTALL_ARGS="--purge"` to remove `/etc/sb` and `/var/lib/sb`.
- The release build does **not** enable the `test-hwmon` feature; that is for
  the fake-sysfs harness only.
- For another architecture, build with an appropriate target/toolchain and set
  `ARCH`/`TAR_ARCH` (e.g. `ARCH=arm64 TAR_ARCH=aarch64` when producing arm64
  packages).
- Check a package with `dpkg-deb -I`, `dpkg-deb -c`, and `lintian` (when
  available), and install it on a disposable Debian VM before shipping.
