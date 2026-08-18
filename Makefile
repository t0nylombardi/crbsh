.PHONY: all build release run test check lint fmt clean install uninstall ci help

BINARY := crbsh
PREFIX ?= /usr/local
BINDIR := $(PREFIX)/bin

all: build

## Build crbsh in debug mode
build:
	cargo build

## Build an optimized release binary
release:
	cargo build --release

## Run crbsh
run:
	cargo run

## Run all tests
test:
	cargo test

## Type-check the project
check:
	cargo check

## Run Clippy with warnings treated as errors
lint:
	cargo clippy --all-targets --all-features -- -D warnings

## Format the source
fmt:
	cargo fmt

## Run all CI checks
ci:
	cargo fmt --check
	cargo check
	cargo test
	cargo clippy --all-targets --all-features -- -D warnings

## Remove build artifacts
clean:
	cargo clean

## Build and install crbsh
install: release
	install -d $(DESTDIR)$(BINDIR)
	install -m 755 target/release/$(BINARY) $(DESTDIR)$(BINDIR)/$(BINARY)

## Uninstall crbsh
uninstall:
	rm -f $(DESTDIR)$(BINDIR)/$(BINARY)

## Show available targets
help:
	@echo "crbsh Makefile"
	@echo ""
	@echo "  make              Build debug version"
	@echo "  make build        Build debug version"
	@echo "  make release      Build optimized release"
	@echo "  make run          Run crbsh"
	@echo "  make test         Run tests"
	@echo "  make check        Run cargo check"
	@echo "  make lint         Run Clippy"
	@echo "  make fmt          Format source"
	@echo "  make ci           Run all CI checks"
	@echo "  make clean        Remove build artifacts"
	@echo "  make install      Install to PREFIX/bin"
	@echo "  make uninstall    Remove installed binary"
