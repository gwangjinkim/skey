// skey — minimal cross-platform secure env-value vault CLI.
//
// OS-native storage only (macOS Keychain / Linux Secret Service /
// Windows Credential Manager) via github.com/zalando/go-keyring.
// This repo never stores secret plaintext; the local index holds
// only entry/variant *names*.
//
// UX:
//
//	skey set <name> [variant]      # secure no-echo prompt, nothing in ps/history
//	skey get <name> [variant|N] [-n]  # bare value, no trailing newline by default
//	skey list [name] | skey ls [name]
//	skey del <name> [variant|N]
package main

import (
	"bufio"
	"fmt"
	"os"
	"sort"
	"strings"

	"github.com/zalando/go-keyring"
	"golang.org/x/term"
)

const service = "skey"
const defaultVariant = "default"

func account(name, variant string) string { return name + "/" + variant }

func usage() {
	fmt.Fprint(os.Stderr, `skey - tiny OS-keyring env vault
usage:
  skey set <name> [variant]        store value (secure prompt, no echo)
  skey get <name> [variant|N] [-n] print value, no newline unless -n/--newline
                                   get always asks for confirmation (no --yes bypass)
  skey run <name> [variant] --env VAR -- <cmd> [args...]
                                   inject secret as $VAR into child only, redact output
  skey list [name]  (alias: ls)    list names / variants, never secrets
  skey del <name> [variant|N]      delete variant or whole name
  skey stats                       per-entry get/run counts + last caller
  skey log [-n N] [--json]         recent audit trail (who ran what, exact argv)
variant: label (prod, dev, ...) or 1-based index; omitted+multiple => picker
examples:
  skey set mistral prod
  MISTRAL_API_KEY=$(skey get mistral prod) pi
`)
}

func cmdSet(args []string) int {
	if len(args) < 1 || len(args) > 2 {
		fmt.Fprintln(os.Stderr, "usage: skey set <name> [variant]")
		return 2
	}
	name, variant := args[0], defaultVariant
	if len(args) == 2 {
		variant = args[1]
	}
	if !valid(name) || !valid(variant) {
		fmt.Fprintln(os.Stderr, "error: name/variant must match [A-Za-z0-9._-]{1,64}")
		return 2
	}
	fmt.Fprintf(os.Stderr, "value for %s/%s (hidden): ", name, variant)
	pw, err := term.ReadPassword(int(os.Stdin.Fd()))
	fmt.Fprintln(os.Stderr)
	if err != nil {
		// Non-tty stdin (pipe): read one line so tests/scripts can feed
		// values; interactive ttys always use hidden ReadPassword.
		fmt.Fprintln(os.Stderr, "(no tty - reading piped line)")
		var sb strings.Builder
		b := make([]byte, 1)
		for {
			n, rerr := os.Stdin.Read(b)
			if n > 0 && b[0] != '\n' {
				sb.WriteByte(b[0])
			} else if n > 0 || rerr != nil {
				break
			}
		}
		pw = []byte(sb.String())
	}
	raw := strings.TrimRight(string(pw), "\r\n")
	val, trimmed := sanitizeSecret(raw)
	if trimmed {
		fmt.Fprintln(os.Stderr, "note: removed surrounding whitespace/quotes before storing")
	}
	if val == "" {
		fmt.Fprintln(os.Stderr, "error: empty value, aborted")
		return 1
	}
	if err := keyring.Set(service, account(name, variant), val); err != nil {
		fmt.Fprintln(os.Stderr, "error: store failed:", err)
		return 1
	}
	ix, err := loadIndex()
	if err != nil {
		fmt.Fprintln(os.Stderr, "warning: stored but index update failed:", err)
		return 0
	}
	addVariant(ix, name, variant)
	if err := saveIndex(ix); err != nil {
		fmt.Fprintln(os.Stderr, "warning: stored but index save failed:", err)
	}
	fmt.Fprintf(os.Stderr, "stored %s/%s\n", name, variant)
	return 0
}

// confirmAccess prompts on /dev/tty (stderr, never stdout) before get prints.
// Returns true only on explicit y/yes. No secret is ever printed here.
func confirmAccess(name, variant string) bool {
	fmt.Fprintf(os.Stderr, "Allow access to %s/%s? [y/N]: ", name, variant)
	line, ok := readConfirmLine()
	if !ok {
		fmt.Fprintln(os.Stderr, "denied (no interactive terminal; automation must use `skey run`)")
		return false
	}
	switch strings.ToLower(strings.TrimSpace(line)) {
	case "y", "yes":
		return true
	}
	fmt.Fprintln(os.Stderr, "denied")
	return false
}

func readConfirmLine() (string, bool) {
	if f, err := os.OpenFile("/dev/tty", os.O_RDONLY, 0); err == nil {
		defer f.Close()
		line, err := bufio.NewReader(f).ReadString('\n')
		if err != nil && len(line) == 0 {
			return "", false
		}
		return line, true
	}
	// Fallback: stdin only when it is a real tty (never pipes).
	if !term.IsTerminal(int(os.Stdin.Fd())) {
		return "", false
	}
	line, err := bufio.NewReader(os.Stdin).ReadString('\n')
	if err != nil && len(line) == 0 {
		return "", false
	}
	return line, true
}

func cmdGet(args []string) int {
	newline := false
	var rest []string
	for _, a := range args {
		switch a {
		case "-n", "--newline":
			newline = true
		case "-y", "--yes", "--no-confirm":
			fmt.Fprintln(os.Stderr, "error: skey get has no --yes flag (always confirms; automation must use `skey run`)")
			return 2
		default:
			rest = append(rest, a)
		}
	}
	args = rest
	if len(args) < 1 || len(args) > 2 {
		fmt.Fprintln(os.Stderr, "usage: skey get <name> [variant|N] [-n]")
		return 2
	}
	name, varg := args[0], ""
	if len(args) == 2 {
		varg = args[1]
	}
	ix, _ := loadIndex()
	variants := ix.Names[name]
	variant, ok := resolveVariant(variants, varg)
	if !ok && len(variants) > 1 && varg == "" {
		pick, err := pickVariant(name, variants)
		if err != nil {
			fmt.Fprintln(os.Stderr, "cancelled")
			return 1
		}
		variant, ok = pick, true
	}
	if !ok {
		if varg == "" {
			variant = defaultVariant // single/unknown: try default (covers pre-index stores)
		} else {
			variant = varg // explicit label: try verbatim
			ok = true
		}
	}
	if !confirmAccess(name, variant) {
		recordCall("get", name, variant, "denied", "confirm-denied", "", "", nil, nil)
		return 1
	}
	val, err := keyring.Get(service, account(name, variant))
	if err != nil {
		fmt.Fprintf(os.Stderr, "error: no such entry %s/%s (hint: `skey list` / `skey set %s %s`)\n", name, variant, name, variant)
		recordCall("get", name, variant, "denied", "not-found", "", "", nil, nil)
		return 1
	}
	recordCall("get", name, variant, "allowed", "", "", "", nil, nil)
	fmt.Fprint(os.Stdout, val)
	if newline {
		fmt.Fprintln(os.Stdout)
	}
	return 0
}

func cmdList(args []string) int {
	ix, err := loadIndex()
	if err != nil {
		fmt.Fprintln(os.Stderr, "error:", err)
		return 1
	}
	if len(args) == 1 {
		name := args[0]
		vs := ix.Names[name]
		if len(vs) == 0 {
			fmt.Fprintf(os.Stderr, "no such entry %q\n", name)
			return 1
		}
		sorted := append([]string(nil), vs...)
		sort.Strings(sorted)
		for i, v := range sorted {
			fmt.Fprintf(os.Stdout, "%d) %s/%s\n", i+1, name, v)
		}
		return 0
	}
	if len(args) > 1 {
		fmt.Fprintln(os.Stderr, "usage: skey list [name]")
		return 2
	}
	names := make([]string, 0, len(ix.Names))
	for n := range ix.Names {
		names = append(names, n)
	}
	sort.Strings(names)
	if len(names) == 0 {
		fmt.Fprintln(os.Stderr, "(empty — `skey set <name> [variant]` to add)")
		return 0
	}
	for _, n := range names {
		vs := append([]string(nil), ix.Names[n]...)
		sort.Strings(vs)
		fmt.Fprintf(os.Stdout, "%s: %s\n", n, strings.Join(vs, ", "))
	}
	return 0
}

func cmdDel(args []string) int {
	if len(args) < 1 || len(args) > 2 {
		fmt.Fprintln(os.Stderr, "usage: skey del <name> [variant|N]")
		return 2
	}
	name, varg := args[0], ""
	if len(args) == 2 {
		varg = args[1]
	}
	ix, err := loadIndex()
	if err != nil {
		fmt.Fprintln(os.Stderr, "error:", err)
		return 1
	}
	variants := ix.Names[name]
	if varg == "" && len(variants) > 1 {
		pick, err := pickVariant(name, variants)
		if err != nil {
			fmt.Fprintln(os.Stderr, "cancelled")
			return 1
		}
		varg = pick
	}
	if varg == "" {
		// delete whole name: all known variants, else try default
		targets := variants
		if len(targets) == 0 {
			targets = []string{defaultVariant}
		}
		for _, v := range targets {
			_ = keyring.Delete(service, account(name, v))
		}
		removeVariant(ix, name, "")
		_ = saveIndex(ix)
		fmt.Fprintf(os.Stderr, "deleted %s\n", name)
		return 0
	}
	variant, ok := resolveVariant(variants, varg)
	if !ok {
		variant = varg // explicit label: try verbatim
	}
	if err := keyring.Delete(service, account(name, variant)); err != nil {
		fmt.Fprintf(os.Stderr, "error: no such entry %s/%s\n", name, variant)
		return 1
	}
	removeVariant(ix, name, variant)
	_ = saveIndex(ix)
	fmt.Fprintf(os.Stderr, "deleted %s/%s\n", name, variant)
	return 0
}

func main() {
	if len(os.Args) < 2 {
		usage()
		os.Exit(2)
	}
	var code int
	switch os.Args[1] {
	case "set":
		code = cmdSet(os.Args[2:])
	case "get":
		code = cmdGet(os.Args[2:])
	case "list", "ls":
		code = cmdList(os.Args[2:])
	case "run":
		code = cmdRun(os.Args[2:])
	case "stats":
		code = cmdStats(os.Args[2:])
	case "log":
		code = cmdLog(os.Args[2:])
	case "del", "rm", "delete":
		code = cmdDel(os.Args[2:])
	case "-h", "--help", "help":
		usage()
	default:
		fmt.Fprintf(os.Stderr, "unknown command %q\n", os.Args[1])
		usage()
		code = 2
	}
	os.Exit(code)
}
