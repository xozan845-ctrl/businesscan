VERSION ?= $(shell sed -n 's/^version = "\(.*\)"/\1/p' Cargo.toml | head -1)
BINARY  := businesscan
HOST    := $(shell rustc -vV | sed -n 's/^host: //p')
DIST    := dist

.PHONY: build release test fmt clippy check dist termux clean

build:
	cargo build

release:
	cargo build --release

test:
	cargo test --all-targets

fmt:
	cargo fmt

clippy:
	cargo clippy --all-targets -- -D warnings

check: fmt clippy test

dist: release
	mkdir -p $(DIST)
	cp target/release/$(BINARY) $(DIST)/$(BINARY)-$(HOST)
	@echo "Binario: $(DIST)/$(BINARY)-$(HOST)"

termux: release
	mkdir -p $(PREFIX)/bin
	install -m755 target/release/$(BINARY) $(PREFIX)/bin/$(BINARY)
	@echo "Instalado en $(PREFIX)/bin/$(BINARY)"

clean:
	cargo clean
	rm -rf $(DIST)
