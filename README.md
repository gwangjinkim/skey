# senv — OS-keyring env vault (Go + Rust)

Two interchangeable implementations sharing one OS-native store:

- [`senv/`](senv/) — Go implementation (`go install`); canonical docs:
  [`senv/README.md`](senv/README.md), [`senv/QUICKSTART.md`](senv/QUICKSTART.md),
  [`senv/RUN_THREAT_MODEL.md`](senv/RUN_THREAT_MODEL.md)
- [`senv-rs/`](senv-rs/) — Rust port (`cargo install`), same entries/index

Secrets live only in the OS keyring (macOS Keychain, Linux Secret Service,
Windows Credential Manager). The local index holds entry names only.

Start here: [`senv/README.md`](senv/README.md) — friendly guide with copy-paste
recipes for pi, opencode, claude code, codex, aider, gemini CLI, and more.

> Note: this repo was initialized from a working directory that also contains
> unrelated local folders; only `senv/`, `senv-rs/`, and these top-level
> meta files are tracked.
