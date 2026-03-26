.PHONY: build test lint clean install

build:
	cargo build --release

test:
	cargo test

lint:
	cargo clippy -- -D warnings
	cargo fmt -- --check

clean:
	cargo clean

install: build
	sudo cp target/release/pm /usr/local/bin/pm-v3

dev:
	cargo build

check: lint test
