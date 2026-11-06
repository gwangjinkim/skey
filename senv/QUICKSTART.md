# senv — 30-second quickstart

OS-keychain vault. Secrets never touch history, dotfiles, or `list`.

## Install (either binary, same store)

```sh
cd senv && go install .            # Go -> `senv` on PATH
# or: cd senv-rs && cargo install --path .  # Rust, identical UX
```

## Use (copy-paste)

```sh
senv set mistral prod              # hidden prompt, stored in OS keychain
senv set mistral dev               # second variant, same name
senv get mistral prod              # always asks `Allow access to mistral/prod? [y/N]`
                                     # (confirm on /dev/tty; stdout stays pure; no -y bypass)
senv get mistral prod -n            # same, with trailing newline (-n/--newline)
senv get mistral 2                 # by 1-based index (see `list mistral`)
senv list                          # mistral: dev, prod (never secrets)
senv list mistral                  # 1) mistral/dev  2) mistral/prod
senv get mistral                   # picker on stderr (safe in $(...))
senv run mistral prod --env MISTRAL_API_KEY -- curl args...  # use-without-see (confirm gate, redacted)
senv stats                          # per-entry get/run counts + last caller (also: senv log [-n N] [--json])
```

## pi (keys stay out of env files/history)

`get` is interactive-only (always confirms; `get --yes` is rejected). For
agents/automation use the redacted path so the model never reads plaintext:

```sh
senv run mistral prod --env MISTRAL_API_KEY -- pi   # human confirms once; model uses without seeing the key
senv run mistral prod --env MISTRAL_API_KEY -y -- pi args...  # -y only here (trusted automation, output still redacted)
```

Do NOT use `!senv get ...` in `auth.json` (no tty to confirm on, and there
is intentionally no bypass). Either have a human run the interactive `get`
first (explicit pre-approval), or wrap the agent spawn in `senv run`.

## Cleanup

```sh
senv del mistral dev               # one variant (label or index; rm = alias)
senv del mistral                   # whole name (picker if ambiguous)
```

Missing key -> exit 1 + hint. Index holds names only (`~/.config/senv/`).
No `--raw` flag: `get` is already raw; `-n` only adds `\n`.
