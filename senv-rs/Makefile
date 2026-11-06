BIN := senv
DIST := dist

.PHONY: build install-local dist clean check

build:
	cargo build --release

install-local: build
	mkdir -p ~/.local/bin
	cp target/release/$(BIN) ~/.local/bin/$(BIN)

# Cross builds. Requires rustup targets (see README) with rustup shims
# first on PATH so cross std libs resolve.
dist:
	mkdir -p $(DIST)
	cargo build --release --target aarch64-apple-darwin
	cp target/aarch64-apple-darwin/release/$(BIN) $(DIST)/$(BIN)-darwin-arm64
	cargo build --release --target x86_64-apple-darwin
	cp target/x86_64-apple-darwin/release/$(BIN) $(DIST)/$(BIN)-darwin-amd64
	cargo build --release --target x86_64-unknown-linux-gnu
	cp target/x86_64-unknown-linux-gnu/release/$(BIN) $(DIST)/$(BIN)-linux-amd64
	cargo build --release --target aarch64-unknown-linux-gnu
	cp target/aarch64-unknown-linux-gnu/release/$(BIN) $(DIST)/$(BIN)-linux-arm64
	cargo build --release --target x86_64-pc-windows-msvc
	cp target/x86_64-pc-windows-msvc/release/$(BIN).exe $(DIST)/$(BIN)-windows-amd64.exe
	ls -la $(DIST)

check:
	cargo clippy -- -D warnings

clean:
	cargo clean
	rm -rf $(DIST)
