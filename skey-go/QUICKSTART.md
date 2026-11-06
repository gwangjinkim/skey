# skey — 30-second quickstart

OS-keychain vault. Secrets never touch history, dotfiles, or `list`.

## Install (either binary, same store)

```sh
cd skey-go && go install .            # Go -> `skey` on PATH
# or: cd skey && cargo install --path .  # Rust, identical UX (or: cargo install skey)
```

## Use (copy-paste)

```sh
skey set mistral prod              # hidden prompt, stored in OS keychain
skey set mistral dev               # second variant, same name
skey get mistral prod              # always asks `Allow access to mistral/prod? [y/N]`
                                     # (confirm on /dev/tty; stdout stays pure; no -y bypass)
skey get mistral prod -n            # same, with trailing newline (-n/--newline)
skey get mistral 2                 # by 1-based index (see `list mistral`)
skey list                          # mistral: dev, prod (never secrets)
skey list mistral                  # 1) mistral/dev  2) mistral/prod
skey get mistral                   # picker on stderr (safe in $(...))
skey run mistral prod --env MISTRAL_API_KEY -- curl args...  # use-without-see (confirm gate, redacted)
skey stats                          # per-entry get/run counts + last caller (also: skey log [-n N] [--json])
```

## Agent Harness Quickstart Table

| Coding Agent / Harness | Clean Exit (if active) | Start / Resume with `skey` |
| :--- | :--- | :--- |
| **`pi`** | `/quit` | `skey run mistral prod --env MISTRAL_API_KEY -- pi --continue` |
| **`claude`** (Claude Code) | `/exit` or `Ctrl+C` | `skey run anthropic prod --env ANTHROPIC_API_KEY -- claude --resume` |
| **`aider`** | `/exit` or `/quit` | `skey run openai prod --env OPENAI_API_KEY -- aider --restore-chat-history` |
| **`opencode`** | `Ctrl+C` / `/exit` | `skey run anthropic prod --env ANTHROPIC_API_KEY -- opencode --continue` |
| **`codex`** / **`omp`** | `Ctrl+D` / `exit` | `skey run openai prod --env OPENAI_API_KEY -- codex --resume` |
| **`cursor`** / **`windsurf`** | Restart IDE window | `skey run openai prod --env OPENAI_API_KEY -- cursor .` |

Do NOT put `!skey get ...` in `auth.json` (no tty to confirm on, and there
is intentionally no bypass). Wrap the agent spawn in `skey run`.

## Cleanup

```sh
skey del mistral dev               # one variant (label or index; rm = alias)
skey del mistral                   # whole name (picker if ambiguous)
```

Missing key -> exit 1 + hint. Index holds names only (`~/.config/skey/`).
No `--raw` flag: `get` is already raw; `-n` only adds `\n`.
