package main

// In-program call counter + audit trail.
//
// Every secret-touching call (`get`, `run`) records, from INSIDE the senv
// process (not a wrapper script, so a model cannot bypass it by calling the
// binary directly):
//   - timestamp (UTC RFC3339), pid, ppid + parent process name + short
//     parent chain (best effort), user, cwd, executable path, tty hint
//   - the EXACT argv senv was invoked with (os.Args, quoted)
//   - subcommand, entry name/variant (NEVER the secret value)
//   - decision (allowed/denied + reason), plus run extras (env var name,
//     child basename, redact flag, child exit code)
//
// Storage (next to index.json, see senvDir):
//   audit.log  - append-only JSON Lines, 0600, one object per get/run call.
//                Pre-existing legacy text lines ("<ts> run ...") are kept
//                and still shown by `senv log`.
//   counts.json - {"<sub> <name>/<variant>": {count,last_ts,last_caller,
//                last_argv,last_decision}}, 0600. Incremented on every
//                get/run call, allowed or denied.
//
// `senv stats` prints per-entry get/run counts + last caller.
// `senv log [-n N|--tail N] [--json]` prints the last N audit lines
// (default 20) as a short human summary, or raw JSON with --json.
// Secrets are never written to either file.

import (
	"encoding/json"
	"fmt"
	"os"
	"os/exec"
	"os/user"
	"path/filepath"
	"sort"
	"strconv"
	"strings"
	"time"
)

// skeyDir returns the config dir, honouring SKEY_CONFIG_DIR or SENV_CONFIG_DIR for backwards compatibility/tests.
func skeyDir() (string, error) {
	if d := os.Getenv("SKEY_CONFIG_DIR"); d != "" {
		return d, nil
	}
	if d := os.Getenv("SENV_CONFIG_DIR"); d != "" {
		return d, nil
	}
	base, err := os.UserConfigDir()
	if err != nil {
		return "", err
	}
	return filepath.Join(base, "skey"), nil
}

func auditPath() (string, error) {
	d, err := skeyDir()
	if err != nil {
		return "", err
	}
	return filepath.Join(d, "audit.log"), nil
}

func countsPath() (string, error) {
	d, err := skeyDir()
	if err != nil {
		return "", err
	}
	return filepath.Join(d, "counts.json"), nil
}

// parentComm returns the short process name for pid (best effort, "" on failure).
func parentComm(pid int) string {
	if pid <= 0 {
		return ""
	}
	// Linux fast path.
	if b, err := os.ReadFile(fmt.Sprintf("/proc/%d/comm", pid)); err == nil {
		return strings.TrimSpace(string(b))
	}
	// macOS/Linux fallback: ps. (Windows: best effort, likely empty.)
	out, err := exec.Command("ps", "-p", strconv.Itoa(pid), "-o", "comm=").Output()
	if err != nil {
		return ""
	}
	return strings.TrimSpace(string(out))
}

// parentPpid returns the parent pid of pid (best effort, 0 on failure).
func parentPpid(pid int) int {
	if pid <= 0 {
		return 0
	}
	if b, err := os.ReadFile(fmt.Sprintf("/proc/%d/stat", pid)); err == nil {
		// comm may contain spaces/parens; ppid follows the last ')'.
		s := string(b)
		if i := strings.LastIndex(s, ")"); i >= 0 {
			f := strings.Fields(s[i+1:])
			if len(f) >= 2 {
				if n, err := strconv.Atoi(f[1]); err == nil {
					return n
				}
			}
		}
	}
	out, err := exec.Command("ps", "-p", strconv.Itoa(pid), "-o", "ppid=").Output()
	if err != nil {
		return 0
	}
	n, _ := strconv.Atoi(strings.TrimSpace(string(out)))
	return n
}

// callerChain walks up to 4 ancestors: ["name(pid)", ...]. Best effort.
func callerChain(ppid int) []string {
	var chain []string
	pid := ppid
	for i := 0; i < 4 && pid > 0; i++ {
		name := parentComm(pid)
		if name == "" {
			chain = append(chain, fmt.Sprintf("pid=%d", pid))
		} else {
			chain = append(chain, fmt.Sprintf("%s(%d)", name, pid))
		}
		pid = parentPpid(pid)
	}
	return chain
}

func ttyHint() string {
	if t := os.Getenv("SSH_TTY"); t != "" {
		return t
	}
	// /dev/tty presence tells us a controlling terminal exists.
	if f, err := os.OpenFile("/dev/tty", os.O_RDONLY, 0); err == nil {
		f.Close()
		return "dev-tty"
	}
	return "no-tty"
}

func userName() string {
	if u := os.Getenv("USER"); u != "" {
		return u
	}
	if u, err := user.Current(); err == nil {
		return u.Username
	}
	return ""
}

// quoteArgv renders the exact argv for the audit log (shell-style quoting).
func quoteArgv(argv []string) string {
	var q []string
	for _, a := range argv {
		if a == "" {
			q = append(q, "''")
		} else if strings.ContainsAny(a, " \t\n\"'\\$`!*?()[]{}<>|&;") {
			q = append(q, "'"+strings.ReplaceAll(a, "'", "'\\''")+"'")
		} else {
			q = append(q, a)
		}
	}
	return strings.Join(q, " ")
}

type auditEntry struct {
	Ts       string   `json:"ts"`
	Pid      int      `json:"pid"`
	Ppid     int      `json:"ppid"`
	Pname    string   `json:"pname,omitempty"`
	Chain    []string `json:"chain,omitempty"`
	User     string   `json:"user,omitempty"`
	Cwd      string   `json:"cwd,omitempty"`
	Exe      string   `json:"exe,omitempty"`
	Tty      string   `json:"tty,omitempty"`
	Argv     []string `json:"argv"`
	ArgvStr  string   `json:"argv_str"`
	Sub      string   `json:"sub"`
	Name     string   `json:"name,omitempty"`
	Variant  string   `json:"variant,omitempty"`
	Env      string   `json:"env,omitempty"`
	Cmd      string   `json:"cmd,omitempty"`
	Decision string   `json:"decision"` // allowed | denied
	Reason   string   `json:"reason,omitempty"`
	Redact   *bool    `json:"redact,omitempty"`
	Exit     *int     `json:"exit,omitempty"`
}

// recordCall appends one JSON audit line and bumps the persistent counter.
// It never logs secret values; callers pass only metadata.
func recordCall(sub, name, variant, decision, reason string, envVar, cmdBase string, redact *bool, exitCode *int) {
	pid := os.Getpid()
	ppid := os.Getppid()
	cwd, _ := os.Getwd()
	exe, _ := os.Executable()
	e := auditEntry{
		Ts: time.Now().UTC().Format(time.RFC3339), Pid: pid, Ppid: ppid,
		Pname: parentComm(ppid), Chain: callerChain(ppid),
		User: userName(), Cwd: cwd, Exe: exe, Tty: ttyHint(),
		Argv: os.Args, ArgvStr: quoteArgv(os.Args),
		Sub: sub, Name: name, Variant: variant,
		Env: envVar, Cmd: cmdBase, Decision: decision, Reason: reason,
		Redact: redact, Exit: exitCode,
	}
	dir, err := skeyDir()
	if err != nil {
		return
	}
	_ = os.MkdirAll(dir, 0o700)
	p, err := auditPath()
	if err != nil {
		return
	}
	f, err := os.OpenFile(p, os.O_APPEND|os.O_CREATE|os.O_WRONLY, 0o600)
	if err != nil {
		return
	}
	b, _ := json.Marshal(e)
	b = append(b, '\n')
	_, _ = f.Write(b)
	_ = f.Close()
	bumpCount(sub, name, variant, e)
}

type countRec struct {
	Count        int    `json:"count"`
	LastTs       string `json:"last_ts"`
	LastCaller   string `json:"last_caller"`
	LastArgv     string `json:"last_argv"`
	LastDecision string `json:"last_decision"`
}

func bumpCount(sub, name, variant string, e auditEntry) {
	p, err := countsPath()
	if err != nil {
		return
	}
	m := map[string]*countRec{}
	if b, err := os.ReadFile(p); err == nil && len(b) > 0 {
		_ = json.Unmarshal(b, &m)
	}
	key := sub + " " + name + "/" + variant
	r := m[key]
	if r == nil {
		r = &countRec{}
		m[key] = r
	}
	r.Count++
	r.LastTs = e.Ts
	if e.Pname != "" {
		r.LastCaller = fmt.Sprintf("%s by %s(%d)", e.User, e.Pname, e.Ppid)
	} else {
		r.LastCaller = fmt.Sprintf("%s pid=%d ppid=%d", e.User, e.Pid, e.Ppid)
	}
	r.LastArgv = e.ArgvStr
	r.LastDecision = e.Decision
	b, _ := json.MarshalIndent(m, "", "  ")
	tmp := p + ".tmp"
	_ = os.WriteFile(tmp, b, 0o600)
	_ = os.Rename(tmp, p)
}

func boolPtr(b bool) *bool { return &b }
func intPtr(i int) *int    { return &i }

func cmdStats(args []string) int {
	if len(args) > 0 {
		fmt.Fprintln(os.Stderr, "usage: skey stats")
		return 2
	}
	p, err := countsPath()
	if err != nil {
		fmt.Fprintln(os.Stderr, "error:", err)
		return 1
	}
	b, err := os.ReadFile(p)
	if err != nil || len(b) == 0 {
		fmt.Fprintln(os.Stdout, "(no recorded get/run calls yet)")
		return 0
	}
	m := map[string]*countRec{}
	if err := json.Unmarshal(b, &m); err != nil {
		fmt.Fprintln(os.Stderr, "error: corrupt counts file:", err)
		return 1
	}
	keys := make([]string, 0, len(m))
	for k := range m {
		keys = append(keys, k)
	}
	sort.Strings(keys)
	for _, k := range keys {
		r := m[k]
		fmt.Fprintf(os.Stdout, "%s: %d (last %s %s; %s)\n  $ %s\n",
			k, r.Count, r.LastTs, r.LastDecision, r.LastCaller, r.LastArgv)
	}
	return 0
}

func cmdLog(args []string) int {
	n := 20
	jsonOut := false
	for i := 0; i < len(args); i++ {
		a := args[i]
		switch {
		case a == "--json":
			jsonOut = true
		case (a == "-n" || a == "--tail") && i+1 < len(args):
			i++
			v, err := strconv.Atoi(args[i])
			if err != nil || v < 0 {
				fmt.Fprintln(os.Stderr, "error: -n needs a non-negative number")
				return 2
			}
			n = v
		case strings.HasPrefix(a, "-n="):
			v, err := strconv.Atoi(strings.TrimPrefix(a, "-n="))
			if err != nil || v < 0 {
				fmt.Fprintln(os.Stderr, "error: -n needs a non-negative number")
				return 2
			}
			n = v
		default:
			fmt.Fprintln(os.Stderr, "usage: skey log [-n N|--tail N] [--json]")
			return 2
		}
	}
	p, err := auditPath()
	if err != nil {
		fmt.Fprintln(os.Stderr, "error:", err)
		return 1
	}
	b, err := os.ReadFile(p)
	if err != nil {
		fmt.Fprintln(os.Stderr, "(empty audit log)")
		return 0
	}
	lines := strings.Split(strings.TrimRight(string(b), "\n"), "\n")
	if len(lines) == 1 && lines[0] == "" {
		fmt.Fprintln(os.Stderr, "(empty audit log)")
		return 0
	}
	if n < len(lines) {
		lines = lines[len(lines)-n:]
	}
	for _, ln := range lines {
		if jsonOut {
			fmt.Fprintln(os.Stdout, ln)
			continue
		}
		var e auditEntry
		if err := json.Unmarshal([]byte(ln), &e); err != nil {
			// Legacy pre-JSON text line: print as-is.
			fmt.Fprintln(os.Stdout, ln)
			continue
		}
		who := e.User
		if e.Pname != "" {
			who += " via " + e.Pname + "(" + strconv.Itoa(e.Ppid) + ")"
		}
		what := e.Sub + " " + e.Name + "/" + e.Variant
		extra := ""
		if e.Cmd != "" {
			extra = " cmd=" + e.Cmd
		}
		fmt.Fprintf(os.Stdout, "%s %s %s%s by %s\n  $ %s\n",
			e.Ts, e.Decision, what, extra, who, e.ArgvStr)
	}
	return 0
}
