SHELL := /bin/bash

VERSION ?= $(shell sed -n 's/^version = "\(.*\)"/\1/p' Cargo.toml | head -n1)
ARCH ?= amd64
TAR_ARCH ?= x86_64
DIST := dist
STAGE := $(DIST)/sb-$(VERSION)-$(TAR_ARCH)-linux
NFPM ?= nfpm

ifeq ($(strip $(VERSION)),)
$(error could not determine VERSION from Cargo.toml)
endif

.PHONY: all build test deb stage tarball checksums dist release install uninstall clean

all: build

build:
	cargo build --release --locked --workspace
	cargo build --release --locked -p sb-daemon --example migrate_history

test:
	cargo test --workspace --locked

deb: build
	@command -v $(NFPM) >/dev/null 2>&1 || { \
		echo "nfpm not found; install it or run 'make NFPM=/path/to/nfpm deb'" >&2; \
		exit 1; \
	}
	mkdir -p $(DIST)
	VERSION=$(VERSION) ARCH=$(ARCH) $(NFPM) package -f nfpm.yaml -p deb -t $(DIST)/

stage: build
	rm -rf $(STAGE)
	mkdir -p $(STAGE)/sysusers.d
	install -m755 target/release/sb-daemon $(STAGE)/sb-daemon
	install -m755 target/release/sb $(STAGE)/sb
	install -m755 target/release/examples/migrate_history $(STAGE)/migrate-history
	install -m644 sb-daemon.service $(STAGE)/sb-daemon.service
	install -m644 sb.toml $(STAGE)/sb.toml
	install -m644 README.md $(STAGE)/README.md
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

clean:
	rm -rf $(DIST)
