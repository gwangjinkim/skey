package main

import (
	"encoding/json"
	"fmt"
	"os"
	"path/filepath"
	"regexp"
	"sort"
	"strconv"
)

// index stores only names+variant labels. Secrets live in OS keyring only.
type index struct {
	Names map[string][]string `json:"names"`
}

var validRe = regexp.MustCompile(`^[A-Za-z0-9._-]{1,64}$`)

func valid(s string) bool { return validRe.MatchString(s) }

func indexPath() (string, error) {
	dir, err := senvDir()
	if err != nil {
		return "", err
	}
	return filepath.Join(dir, "index.json"), nil
}

func loadIndex() (*index, error) {
	ix := &index{Names: map[string][]string{}}
	p, err := indexPath()
	if err != nil {
		return nil, err
	}
	b, err := os.ReadFile(p)
	if os.IsNotExist(err) {
		return ix, nil
	}
	if err != nil {
		return nil, err
	}
	if len(b) == 0 {
		return ix, nil
	}
	if err := json.Unmarshal(b, ix); err != nil {
		return nil, fmt.Errorf("corrupt index %s: %w", p, err)
	}
	if ix.Names == nil {
		ix.Names = map[string][]string{}
	}
	return ix, nil
}

func saveIndex(ix *index) error {
	p, err := indexPath()
	if err != nil {
		return err
	}
	if err := os.MkdirAll(filepath.Dir(p), 0o700); err != nil {
		return err
	}
	tmp := p + ".tmp"
	b, _ := json.MarshalIndent(ix, "", "  ")
	if err := os.WriteFile(tmp, b, 0o600); err != nil {
		return err
	}
	return os.Rename(tmp, p)
}

func addVariant(ix *index, name, variant string) {
	for _, v := range ix.Names[name] {
		if v == variant {
			return
		}
	}
	ix.Names[name] = append(ix.Names[name], variant)
	sort.Strings(ix.Names[name])
}

func removeVariant(ix *index, name, variant string) {
	if variant == "" { // whole name
		delete(ix.Names, name)
		return
	}
	kept := ix.Names[name][:0:0]
	for _, v := range ix.Names[name] {
		if v != variant {
			kept = append(kept, v)
		}
	}
	if len(kept) == 0 {
		delete(ix.Names, name)
	} else {
		ix.Names[name] = kept
	}
}

// resolveVariant maps "" | name | 1-based index -> concrete variant.
// Returns ("", false) when ambiguous (caller should pick interactively).
func resolveVariant(variants []string, arg string) (string, bool) {
	if arg == "" {
		if len(variants) == 1 {
			return variants[0], true
		}
		return "", false
	}
	if n, err := strconv.Atoi(arg); err == nil && n >= 1 && n <= len(variants) {
		return variants[n-1], true
	}
	for _, v := range variants {
		if v == arg {
			return v, true
		}
	}
	return "", false
}

// pickVariant prints a numbered list to stderr, reads one line from tty/stdin.
func pickVariant(name string, variants []string) (string, error) {
	sorted := append([]string(nil), variants...)
	sort.Strings(sorted)
	fmt.Fprintf(os.Stderr, "multiple variants for %q:\n", name)
	for i, v := range sorted {
		fmt.Fprintf(os.Stderr, "  %d) %s\n", i+1, v)
	}
	fmt.Fprintf(os.Stderr, "choose [1-%d]: ", len(sorted))
	var line string
	// Prefer /dev/tty so `$(senv get x)` pipelines keep working.
	var in *os.File
	if tty, err := os.Open("/dev/tty"); err == nil {
		defer tty.Close()
		in = tty
	} else {
		in = os.Stdin
	}
	var buf [64]byte
	n, _ := in.Read(buf[:])
	line = string(buf[:n])
	line = trimSpace(line)
	idx, err := strconv.Atoi(line)
	if err != nil || idx < 1 || idx > len(sorted) {
		return "", fmt.Errorf("cancelled")
	}
	return sorted[idx-1], nil
}

func trimSpace(s string) string {
	for len(s) > 0 && (s[0] == ' ' || s[0] == '\t' || s[0] == '\n' || s[0] == '\r') {
		s = s[1:]
	}
	for len(s) > 0 && (s[len(s)-1] == ' ' || s[len(s)-1] == '\t' || s[len(s)-1] == '\n' || s[len(s)-1] == '\r') {
		s = s[:len(s)-1]
	}
	return s
}
