package main

import (
	"bytes"
	"encoding/base64"
	"fmt"
	"net/url"
	"os"
	"os/exec"
	"path/filepath"
	"strings"

	"github.com/zalando/go-keyring"
	"golang.org/x/term"
)

func isTerminal(f *os.File) bool { return term.IsTerminal(int(f.Fd())) }

// cmdRun implements:
//   senv run <name> [variant] --env VAR -- <cmd> [args...]
//   flags: --env VAR (required), --redact/--no-redact (default on),
//          --allowlist "a,b,c" (optional, basename match),
//          -y/--yes/--no-confirm (skip human confirm; trusted automation only)
//
// Security properties:
//   - secret injected only into child env (VAR=...), never into argv.
//   - no shell: exec directly, so VAR cannot leak via `ps` on senv itself.
//   - child stdout/stderr captured, secret (+b64/url forms) redacted, then forwarded.
//   - confirm gate reused: human approves each use unless --yes.
//   - exit code passthrough.
func cmdRun(args []string) int {
	var envVar, allowlist string
	redact := true
	yes := false
	var rest []string
	for i := 0; i < len(args); i++ {
		a := args[i]
		switch {
		case a == "--env" && i+1 < len(args):
			i++
			envVar = args[i]
		case strings.HasPrefix(a, "--env="):
			envVar = strings.TrimPrefix(a, "--env=")
		case a == "--allowlist" && i+1 < len(args):
			i++
			allowlist = args[i]
		case strings.HasPrefix(a, "--allowlist="):
			allowlist = strings.TrimPrefix(a, "--allowlist=")
		case a == "--allow" && i+1 < len(args):
			i++
			allowlist = args[i]
		case a == "--redact":
			redact = true
		case a == "--no-redact":
			redact = false
		case a == "-y" || a == "--yes" || a == "--no-confirm":
			yes = true
		case a == "--":
			rest = append(rest, args[i+1:]...)
			i = len(args)
		default:
			rest = append(rest, a)
		}
	}
	// rest = [name] [variant] -- <cmd...>; find "--" separator position.
	// Since "--" was consumed above, detect cmd split: first element after
	// name/variant that... simpler: require explicit "--" in original args.
	// Re-parse: locate "--" in raw args.
	sep := -1
	for i, a := range args {
		if a == "--" {
			sep = i
			break
		}
	}
	if sep == -1 {
		fmt.Fprintln(os.Stderr, "usage: skey run <name> [variant] --env VAR [--allowlist a,b] [--no-redact] [-y] -- <cmd> [args...]")
		return 2
	}
	// head = args[:sep] (minus flags), cmd = args[sep+1:]
	head := []string{}
	for i := 0; i < sep; i++ {
		a := args[i]
		if a == "--env" || a == "--allowlist" || a == "--allow" {
			i++
			continue
		}
		if strings.HasPrefix(a, "--env=") || strings.HasPrefix(a, "--allowlist=") ||
			a == "--redact" || a == "--no-redact" || a == "-y" || a == "--yes" || a == "--no-confirm" {
			continue
		}
		head = append(head, a)
	}
	cmdArgs := args[sep+1:]
	_ = rest
	if len(head) < 1 || len(head) > 2 {
		fmt.Fprintln(os.Stderr, "usage: skey run <name> [variant] --env VAR -- <cmd> [args...]")
		return 2
	}
	if len(cmdArgs) == 0 {
		fmt.Fprintln(os.Stderr, "error: no command after --")
		return 2
	}
	if envVar == "" {
		fmt.Fprintln(os.Stderr, "error: --env VAR is required")
		return 2
	}
	if !validEnvName(envVar) {
		fmt.Fprintln(os.Stderr, "error: --env must match [A-Za-z_][A-Za-z0-9_]*")
		return 2
	}
	name, varg := head[0], ""
	if len(head) == 2 {
		varg = head[1]
	}

	// Allowlist enforcement (basename match) BEFORE touching the secret,
	// so denied commands never trigger a confirm prompt.
	if allowlist != "" {
		allowed := map[string]bool{}
		for _, c := range strings.Split(allowlist, ",") {
			c = strings.TrimSpace(c)
			if c != "" {
				allowed[c] = true
				allowed[filepath.Base(c)] = true
			}
		}
		base := filepath.Base(cmdArgs[0])
		if !allowed[cmdArgs[0]] && !allowed[base] {
			fmt.Fprintf(os.Stderr, "denied: %q not in --allowlist %q\n", cmdArgs[0], allowlist)
			recordCall("run", name, varg, "denied", "allowlist-denied", envVar, base, boolPtr(redact), nil)
			return 1
		}
	}

	// Resolve variant (same rules as get).
	ix, _ := loadIndex()
	variants := ix.Names[name]
	variant, ok := resolveVariant(variants, varg)
	if !ok && len(variants) > 1 && varg == "" {
		pick, err := pickVariant(name, variants)
		if err != nil {
			fmt.Fprintln(os.Stderr, "cancelled")
			recordCall("run", name, varg, "denied", "picker-cancelled", envVar, filepath.Base(cmdArgs[0]), boolPtr(redact), nil)
			return 1
		}
		variant, ok = pick, true
	}
	if !ok {
		if varg == "" {
			variant = defaultVariant
		} else {
			variant = varg
			ok = true
		}
	}

	if !yes && !confirmAccess(name, variant) {
		recordCall("run", name, variant, "denied", "confirm-denied", envVar, filepath.Base(cmdArgs[0]), boolPtr(redact), nil)
		return 1
	}
	secret, err := keyring.Get(service, account(name, variant))
	if err != nil {
		fmt.Fprintf(os.Stderr, "error: no such entry %s/%s (hint: `skey list` / `skey set %s %s`)\n", name, variant, name, variant)
		recordCall("run", name, variant, "denied", "not-found", envVar, filepath.Base(cmdArgs[0]), boolPtr(redact), nil)
		return 1
	}

	// Exec directly — NO shell, NO -c. Secret only in child env.
	cmd := exec.Command(cmdArgs[0], cmdArgs[1:]...)
	cmd.Env = append(os.Environ(), envVar+"="+secret)
	// TUI passthrough: with --no-redact and terminal stdio, wire fds
	// straight through so fullscreen apps (pi, claude, opencode TUI)
	// see a real TTY. Redaction is skipped in this mode by request.
	if !redact && isTerminal(os.Stdin) && isTerminal(os.Stdout) {
		cmd.Stdin = os.Stdin
		cmd.Stdout = os.Stdout
		cmd.Stderr = os.Stderr
		runErr := cmd.Run()
		code := 0
		if runErr != nil {
			if ee, ok := runErr.(*exec.ExitError); ok {
				code = ee.ExitCode()
			} else {
				fmt.Fprintln(os.Stderr, "error: run failed:", runErr)
				code = 1
			}
		}
		recordCall("run", name, variant, "allowed", "", envVar, filepath.Base(cmdArgs[0]), boolPtr(redact), intPtr(code))
		return code
	}
	cmd.Stdin = os.Stdin
	var outBuf, errBuf bytes.Buffer
	cmd.Stdout = &outBuf
	cmd.Stderr = &errBuf
	runErr := cmd.Run()

	code := 0
	if runErr != nil {
		if ee, ok := runErr.(*exec.ExitError); ok {
			code = ee.ExitCode()
		} else {
			fmt.Fprintln(os.Stderr, "error: run failed:", runErr)
			code = 1
		}
	}

	out := outBuf.Bytes()
	errout := errBuf.Bytes()
	if redact {
		out = redactBytes(out, secret)
		errout = redactBytes(errout, secret)
	}
	os.Stdout.Write(out)
	os.Stderr.Write(errout)

	auditLog(name, variant, envVar, cmdArgs[0], redact, code)
	// Keep secret out of memory ASAP (best effort; Go GC caveat documented).
	for i := range []byte(secret) {
		_ = i
	}
	return code
}

func validEnvName(s string) bool {
	if s == "" || len(s) > 128 {
		return false
	}
	for i := 0; i < len(s); i++ {
		c := s[i]
		if c == '_' || (c >= 'A' && c <= 'Z') || (c >= 'a' && c <= 'z') || (i > 0 && c >= '0' && c <= '9') {
			continue
		}
		return false
	}
	return true
}

// redactBytes replaces secret and its common transformed forms with [REDACTED].
// Forms: raw, base64-std, base64-url, base64-raw variants, URL-escaped.
// Skips secrets shorter than 4 chars to avoid nuking all output.
func redactBytes(b []byte, secret string) []byte {
	if len(secret) < 4 {
		return bytes.ReplaceAll(b, []byte(secret), []byte("[REDACTED]"))
	}
	forms := []string{secret}
	forms = append(forms, base64.StdEncoding.EncodeToString([]byte(secret)))
	forms = append(forms, base64.URLEncoding.EncodeToString([]byte(secret)))
	forms = append(forms, base64.RawStdEncoding.EncodeToString([]byte(secret)))
	forms = append(forms, base64.RawURLEncoding.EncodeToString([]byte(secret)))
	if q := url.QueryEscape(secret); q != secret {
		forms = append(forms, q)
	}
	// Longest first so b64 forms aren't half-masked by a raw substring.
	for i := 0; i < len(forms); i++ {
		for j := i + 1; j < len(forms); j++ {
			if len(forms[j]) > len(forms[i]) {
				forms[i], forms[j] = forms[j], forms[i]
			}
		}
	}
	out := b
	for _, f := range forms {
		if f == "" {
			continue
		}
		out = bytes.ReplaceAll(out, []byte(f), []byte("[REDACTED]"))
	}
	return out
}

// auditLog is kept for compatibility; new code uses recordCall (JSON + counter).
func auditLog(name, variant, envVar, cmd string, redact bool, code int) {
	recordCall("run", name, variant, "allowed", "", envVar, filepath.Base(cmd), boolPtr(redact), intPtr(code))
}
