.PHONY: build test lint fmt app setup uninstall probe ci clean help

build:            ## Build the daemon and CLI (release)
	cargo build --release --locked

test:             ## Rust tests + Swift protocol checks
	cargo test --locked
	cd app && swift run -c debug KitChecks

lint:             ## Formatting and lints
	cargo fmt --all --check
	cargo clippy --locked --all-targets -- -D warnings

fmt:
	cargo fmt --all

app:              ## Build build/MacDetour.app
	scripts/build-app.sh

setup:            ## Build, install and authorize: make setup SSH=<alias>
	scripts/setup.sh $(SSH)

uninstall:        ## Remove the daemon (routes are removed first)
	sudo scripts/uninstall.sh

probe:            ## Show which listed addresses are blocked on this network (changes nothing)
	cargo run --release --example probe

ci: lint test app ## Everything a change must pass

clean:
	cargo clean
	rm -rf build app/.build

help:
	@grep -E '^[a-z-]+:.*## ' $(MAKEFILE_LIST) | awk -F':.*## ' '{printf "  %-12s %s\n", $$1, $$2}'
