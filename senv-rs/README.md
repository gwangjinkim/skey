# senv (Rust port)

Tiny OS-keyring env vault CLI — same UX, same entries, same index file as the
Go version in `../senv/`. Either binary reads what the other wrote.

## Interop design (read this before swapping crates)

The store backends deliberately mirror **zalando/go-keyring** 1:1 instead of
using a generic Rust `keyring` crate (tried `keyring` v3 + v4 on macOS:
entries it writes are invisible to `security`/Go — verified Oct 2025):

| OS | Backend | Entry shape (identical to Go) |
|---|---|---|
| macOS | `/usr/bin/security` (`add -U` / `find -wa` / `delete`), password piped via `security -i` (never in `ps`) | `svce=senv`, `acct=<name>/<variant>`, secret `go-keyring-base64:<b64>` |
| Linux | `secret-tool` (libsecret, login collection) | attrs `service=senv username=<name>/<variant>`, raw secret |
| Windows | raw `advapi32` FFI (`CredReadW/WriteW/DeleteW`, no deps) | `TargetName=senv:<name>/<variant>`, blob = UTF-8 bytes |

Index: same path and JSON (`{"names": {name: [variants]}}`) as Go —
`~/Library/Application Support/senv/index.json` (mac),
`~/.config/senv/index.json` (linux), `%APPDATA%\senv\index.json` (win).

## Install

```sh
cd senv-rs
cargo install --path .        # puts `senv` on PATH (~/.cargo/bin)
# or just build: cargo build --release  # ./target/release/senv
```

Requires: Rust stable, plus the OS helper (`/usr/bin/security` on macOS,
`secret-tool` from libsecret on Linux — nothing extra on Windows).

Cross builds (needs the rustup targets + PATH with rustup shims first):

```sh
export PATH="$HOME/.cargo/bin:$PATH"
rustup target add x86_64-apple-darwin x86_64-unknown-linux-gnu \
  x86_64-pc-windows-msvc aarch64-unknown-linux-gnu
make dist   # binaries land in dist/
```

Or simply build natively on each machine (`cargo install --path .`).

## UX (identical to Go)

```
senv set <name> [variant]         # secure no-echo prompt
senv get <name> [variant|N] [-n]  # bare value, NO newline unless -n;
                                    # always confirms (no -y/--yes/--no-confirm)
senv list [name]   (alias: ls)    # names + variants, never secrets
senv del <name> [variant|N]       # delete variant or whole name (aliases: rm)
senv run <name> [variant] --env VAR [-y] -- <cmd> [args...]  # use-without-see:
                                    # confirm gate, no shell, output redacted, exit passthrough
                                    # (--allowlist curl,python3  --no-redact opt-out  -y trusted auto only)
```

Names/variants must match `[A-Za-z0-9._-]{1,64}`. Default variant `default`.
Omitted variant + multiple entries → numbered picker on stderr (reads
`/dev/tty`, falls back to stdin — safe inside `$(...)`).

## Audit (parity with Go: `stats` / `log`, shared files)

Every `get`/`run` — allowed or denied — appends a JSON line to
`senv/audit.log` (0600) with timestamp, pid/ppid/parent name + chain, user,
cwd, exe, tty hint, exact argv, name/variant, decision/reason (+env/cmd-basename/
redact/exit for `run`), and bumps `senv/counts.json`. Secrets are never logged.

```sh
senv stats          # per-entry get/run counts + last caller + exact last command
senv log -n 30     # recent audit entries (--json for raw JSON Lines)
```

Why/motivation, threat model, and honest limits live in the canonical doc:
[`../senv/README.md`](../senv/README.md) (+ [`../senv/RUN_THREAT_MODEL.md`](../senv/RUN_THREAT_MODEL.md)).

## pi integration

```sh
senv set mistral prod                 # hidden prompt, stored in OS keychain
senv get mistral prod                 # always asks confirmation (interactive only)
senv run mistral prod --env MISTRAL_API_KEY -- pi args...   # automation path: use-without-see
```

`get` always confirms on /dev/tty (terminal gate, no bypass — `get --yes`
is rejected). `-y`/`--yes`/`--no-confirm` exists only on `run` (trusted
automation, output still redacted).

Do NOT use `!senv get ...` in `pi` auth.json (no bypass by design). Wrap
the agent in `senv run ... -- pi` or have a human pre-approve via an
interactive `get`.

## Deps

`serde`/`serde_json` (index), `dirs` (config dir), `rpassword` (hidden
prompt), `base64` (go-keyring envelope on macOS). No keyring crate, no clap
(arg parsing is manual, like the Go version). ~750 lines total, of which the
Windows FFI backend (~100) only compiles on Windows.
