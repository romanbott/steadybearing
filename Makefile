SHELL := /bin/bash

VERSION ?= $(shell sed -n 's/^version = "\(.*\)"/\1/p' Cargo.toml | head -n1)
ARCH ?= amd64
TAR_ARCH ?= x86_64
DIST := dist
STAGE := $(DIST)/sb-$(VERSION)-$(TAR_ARCH)-linux
MAN := $(DIST)/man
CHANGELOG := $(DIST)/changelog.gz
NFPM ?= nfpm

ifeq ($(strip $(VERSION)),)
$(error could not determine VERSION from Cargo.toml)
endif

.PHONY: all build test man changelog deb stage tarball checksums dist release install uninstall clean demo

all: build

build:
	cargo build --release --locked --workspace

test:
	cargo test --workspace --locked

man:
	mkdir -p $(MAN)
	gzip -9 -n -c packaging/man/sb.1 > $(MAN)/sb.1.gz
	gzip -9 -n -c packaging/man/sb-daemon.1 > $(MAN)/sb-daemon.1.gz

changelog:
	mkdir -p $(DIST)
	gzip -9 -n -c packaging/changelog > $(CHANGELOG)

deb: build man changelog
	@command -v $(NFPM) >/dev/null 2>&1 || { \
		echo "nfpm not found; install it or run 'make NFPM=/path/to/nfpm deb'" >&2; \
		exit 1; \
	}
	mkdir -p $(DIST)
	VERSION=$(VERSION) ARCH=$(ARCH) $(NFPM) package -f nfpm.yaml -p deb -t $(DIST)/

stage: build man changelog
	rm -rf $(STAGE)
	mkdir -p $(STAGE)/sysusers.d $(STAGE)/man $(STAGE)/assets
	install -m755 target/release/sb-daemon $(STAGE)/sb-daemon
	install -m755 target/release/sb $(STAGE)/sb
	install -m644 sb-daemon.service $(STAGE)/sb-daemon.service
	install -m644 sb.toml $(STAGE)/sb.toml
	install -m644 README.md $(STAGE)/README.md
	install -m644 assets/logo-mark.svg $(STAGE)/assets/logo-mark.svg
	install -m644 LICENSE $(STAGE)/LICENSE
	install -m644 packaging/copyright $(STAGE)/copyright
	install -m644 $(CHANGELOG) $(STAGE)/changelog.gz
	install -m644 $(MAN)/sb.1.gz $(STAGE)/man/sb.1.gz
	install -m644 $(MAN)/sb-daemon.1.gz $(STAGE)/man/sb-daemon.1.gz
	install -m755 packaging/install.sh $(STAGE)/install.sh
	install -m644 packaging/sysusers.d/sb.conf $(STAGE)/sysusers.d/sb.conf

tarball: stage
	tar -C $(DIST) -czf $(DIST)/sb-$(VERSION)-$(TAR_ARCH)-linux.tar.gz \
		sb-$(VERSION)-$(TAR_ARCH)-linux

checksums: deb tarball
	cd $(DIST) && sha256sum \
		sb_$(VERSION)_$(ARCH).deb \
		sb-$(VERSION)-$(TAR_ARCH)-linux.tar.gz > SHA256SUMS

dist: checksums

release: test dist

install: stage
	sudo "$(STAGE)/install.sh" $(INSTALL_ARGS)

uninstall:
	sudo packaging/install.sh --uninstall $(INSTALL_ARGS)

demo:
	vhs dev/demo.tape

clean:
	rm -rf $(DIST)
