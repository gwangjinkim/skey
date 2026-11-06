// skey (Rust) — minimal cross-platform secure env-value vault CLI.
//
// Interop-first design: instead of a generic keyring crate, each OS backend
// mirrors what zalando/go-keyring (the Go skey's store) does, so both tools
// read/write the SAME entries (service `skey`, account `<name>/<variant>`):
//   macOS:   /usr/bin/security add/find/delete-generic-password, values stored
//            as `go-keyring-base64:<b64>` (exactly like go-keyring).
//   Linux:   Secret Service via `secret-tool` with attributes
//            service=<service> username=<account> on the login collection.
//   Windows: Credential Manager, TargetName `<service>:<account>`, blob = the
//            raw UTF-8 password bytes (like danieljoos/wincred use in Go).
// Secrets never touch the index: it holds only entry/variant *names* in the
// same file/format as the Go version (configdir/skey/index.json).
//
// UX (mirrors Go skey):
//   skey set <name> [variant]            # secure no-echo prompt
//   skey get <name> [variant|N] [-n]     # bare value, no newline unless -n
//   skey list [name]  (alias: ls)        # names/variants, never secrets
//   skey del <name> [variant|N]          # delete variant or whole name
//     (aliases: rm, delete)

use std::collections::HashMap;
use std::io::{self, BufRead, IsTerminal, Write};

use serde::{Deserialize, Serialize};

const SERVICE: &str = "skey";
const DEFAULT_VARIANT: &str = "default";

// ---------------- secret store (per-OS, go-keyring compatible) ----------------

#[derive(Debug)]
enum StoreError {
    NotFound,
    Other(String),
}

impl std::fmt::Display for StoreError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            StoreError::NotFound => write!(f, "entry not found"),
            StoreError::Other(m) => write!(f, "{m}"),
        }
    }
}

/// macOS: drive /usr/bin/security exactly like zalando/go-keyring does.
/// Values are stored as `go-keyring-base64:<base64>`.
#[cfg(target_os = "macos")]
mod store {
    use super::StoreError;
    use base64::{Engine as _, engine::general_purpose::STANDARD};
    use std::io::Write;
    use std::process::{Command, Stdio};

    const B64_PREFIX: &str = "go-keyring-base64:";
    const HEX_PREFIX: &str = "go-keyring-encoded:";
    const SECURITY: &str = "/usr/bin/security";

    /// Single-quote for `security -i` (same idea as go's internal/shellescape).
    fn sh_quote(s: &str) -> String {
        format!("'{}'", s.replace('\'', "'\\''"))
    }

    fn run_interactive(cmd_line: &str) -> Result<(), StoreError> {
        let mut child = Command::new(SECURITY)
            .arg("-i")
            .stdin(Stdio::piped())
            .stdout(Stdio::null())
            .stderr(Stdio::piped())
            .spawn()
            .map_err(|e| StoreError::Other(format!("security: {e}")))?;
        child
            .stdin
            .take()
            .ok_or_else(|| StoreError::Other("security: no stdin".into()))?
            .write_all(cmd_line.as_bytes())
            .map_err(|e| StoreError::Other(format!("security: {e}")))?;
        let out = child
            .wait_with_output()
            .map_err(|e| StoreError::Other(format!("security: {e}")))?;
        if out.status.success() {
            return Ok(());
        }
        let msg = String::from_utf8_lossy(&out.stderr).into_owned();
        if msg.contains("could not be found") {
            return Err(StoreError::NotFound);
        }
        Err(StoreError::Other(msg.trim().to_string()))
    }

    pub fn set(service: &str, account: &str, password: &str) -> Result<(), StoreError> {
        // -U: update in place; password via stdin (never in ps argv), like Go.
        let enc = format!("{B64_PREFIX}{}", STANDARD.encode(password.as_bytes()));
        let cmd = format!(
            "add-generic-password -U -s {} -a {} -w {}\n",
            sh_quote(service),
            sh_quote(account),
            sh_quote(&enc)
        );
        run_interactive(&cmd)
    }

    /// Mirror go-keyring Get: trim whitespace, then decode well-known prefixes.
    fn decode(raw: &str) -> Result<String, StoreError> {
        let t = raw.trim();
        if let Some(b64) = t.strip_prefix(B64_PREFIX) {
            return STANDARD
                .decode(b64.trim())
                .map_err(|e| StoreError::Other(format!("decode: {e}")))
                .and_then(|b| {
                    String::from_utf8(b).map_err(|e| StoreError::Other(format!("decode: {e}")))
                });
        }
        if let Some(hex) = t.strip_prefix(HEX_PREFIX) {
            return decode_hex(hex.trim());
        }
        Ok(t.to_string())
    }

    fn decode_hex(s: &str) -> Result<String, StoreError> {
        if !s.len().is_multiple_of(2) {
            return Err(StoreError::Other("decode: odd hex len".into()));
        }
        let b: Option<Vec<u8>> = (0..s.len())
            .step_by(2)
            .map(|i| u8::from_str_radix(&s[i..i + 2], 16).ok())
            .collect();
        let b = b.ok_or_else(|| StoreError::Other("decode: bad hex".into()))?;
        String::from_utf8(b).map_err(|e| StoreError::Other(format!("decode: {e}")))
    }

    pub fn get(service: &str, account: &str) -> Result<String, StoreError> {
        let out = Command::new(SECURITY)
            .args(["find-generic-password", "-s", service, "-wa", account])
            .output()
            .map_err(|e| StoreError::Other(format!("security: {e}")))?;
        if !out.status.success() {
            let msg = String::from_utf8_lossy(&out.stderr).into_owned();
            if msg.contains("could not be found")
                || String::from_utf8_lossy(&out.stdout).contains("could not be found")
            {
                return Err(StoreError::NotFound);
            }
            return Err(StoreError::Other(msg.trim().to_string()));
        }
        decode(&String::from_utf8_lossy(&out.stdout))
    }

    pub fn delete(service: &str, account: &str) -> Result<(), StoreError> {
        let out = Command::new(SECURITY)
            .args(["delete-generic-password", "-s", service, "-a", account])
            .output()
            .map_err(|e| StoreError::Other(format!("security: {e}")))?;
        if out.status.success() {
            return Ok(());
        }
        let msg = String::from_utf8_lossy(&out.stderr).into_owned();
        if msg.contains("could not be found") {
            return Err(StoreError::NotFound);
        }
        Err(StoreError::Other(msg.trim().to_string()))
    }
}

/// Linux: Secret Service via `secret-tool` (libsecret), same attributes as
/// go-keyring: service=<service> username=<account> on the login collection.
#[cfg(target_os = "linux")]
mod store {
    use super::StoreError;
    use std::io::Write;
    use std::process::{Command, Stdio};

    fn attrs(service: &str, account: &str) -> [String; 4] {
        [
            "service".into(),
            service.into(),
            "username".into(),
            account.into(),
        ]
    }

    pub fn set(service: &str, account: &str, password: &str) -> Result<(), StoreError> {
        let a = attrs(service, account);
        let label = format!("Password for '{account}' on '{service}'");
        let mut child = Command::new("secret-tool")
            .args(["store", "--label", &label, &a[0], &a[1], &a[2], &a[3]])
            .stdin(Stdio::piped())
            .stdout(Stdio::null())
            .stderr(Stdio::piped())
            .spawn()
            .map_err(|e| StoreError::Other(format!("secret-tool store: {e} (is libsecret installed?)")))?;
        child
            .stdin
            .take()
            .ok_or_else(|| StoreError::Other("secret-tool: no stdin".into()))?
            .write_all(password.as_bytes())
            .map_err(|e| StoreError::Other(format!("secret-tool: {e}")))?;
        let out = child
            .wait_with_output()
            .map_err(|e| StoreError::Other(format!("secret-tool: {e}")))?;
        if out.status.success() {
            Ok(())
        } else {
            Err(StoreError::Other(
                String::from_utf8_lossy(&out.stderr).trim().to_string(),
            ))
        }
    }

    pub fn get(service: &str, account: &str) -> Result<String, StoreError> {
        let a = attrs(service, account);
        let out = Command::new("secret-tool")
            .args(["lookup", &a[0], &a[1], &a[2], &a[3]])
            .output()
            .map_err(|e| StoreError::Other(format!("secret-tool lookup: {e}")))?;
        if !out.status.success() || out.stdout.is_empty() {
            return Err(StoreError::NotFound);
        }
        // `lookup` appends a trailing newline; strip one (secrets are raw bytes).
        let mut s = String::from_utf8_lossy(&out.stdout).into_owned();
        if s.ends_with('\n') {
            s.pop();
            if s.ends_with('\r') {
                s.pop();
            }
        }
        Ok(s)
    }

    pub fn delete(service: &str, account: &str) -> Result<(), StoreError> {
        get(service, account)?; // NotFound when absent, like go-keyring Delete
        let a = attrs(service, account);
        let out = Command::new("secret-tool")
            .args(["clear", &a[0], &a[1], &a[2], &a[3]])
            .output()
            .map_err(|e| StoreError::Other(format!("secret-tool clear: {e}")))?;
        if out.status.success() {
            Ok(())
        } else {
            Err(StoreError::Other(
                String::from_utf8_lossy(&out.stderr).trim().to_string(),
            ))
        }
    }
}

/// Windows: Credential Manager via raw advapi32 FFI, same shape as
/// danieljoos/wincred as used by go-keyring: TargetName
/// `<service>:<account>` (CRED_TYPE_GENERIC, persist local machine),
/// blob = raw UTF-8 password bytes.
#[cfg(target_os = "windows")]
mod store {
    use super::StoreError;

    type Dword = u32;
    type Bool = i32;
    const GENERIC: Dword = 1;
    const PERSIST_LOCAL_MACHINE: Dword = 2;
    const ERROR_NOT_FOUND: Dword = 1168;

    #[repr(C)]
    struct FileTime {
        low: Dword,
        high: Dword,
    }
    #[repr(C)]
    struct CredentialW {
        flags: Dword,
        typ: Dword,
        target_name: *mut u16,
        comment: *mut u16,
        last_written: FileTime,
        blob_size: Dword,
        blob: *mut u8,
        persist: Dword,
        attr_count: Dword,
        attrs: *mut u8,
        target_alias: *mut u16,
        user_name: *mut u16,
    }

    #[link(name = "advapi32")]
    extern "system" {
        fn CredWriteW(c: *const CredentialW, flags: Dword) -> Bool;
        fn CredReadW(target: *const u16, typ: Dword, flags: Dword, out: *mut *mut CredentialW)
            -> Bool;
        fn CredDeleteW(target: *const u16, typ: Dword, flags: Dword) -> Bool;
        fn CredFree(buf: *mut core::ffi::c_void);
    }
    #[link(name = "kernel32")]
    extern "system" {
        fn GetLastError() -> Dword;
    }

    fn wide(s: &str) -> Vec<u16> {
        s.encode_utf16().chain([0]).collect()
    }
    fn target(service: &str, account: &str) -> Vec<u16> {
        wide(&format!("{service}:{account}"))
    }
    fn last_err() -> Dword {
        unsafe { GetLastError() }
    }

    pub fn set(service: &str, account: &str, password: &str) -> Result<(), StoreError> {
        let t = target(service, account);
        let mut user = wide(account);
        let mut blob = password.as_bytes().to_vec();
        let cred = CredentialW {
            flags: 0,
            typ: GENERIC,
            target_name: t.as_ptr() as *mut u16,
            comment: std::ptr::null_mut(),
            last_written: FileTime { low: 0, high: 0 },
            blob_size: blob.len() as Dword,
            blob: blob.as_mut_ptr(),
            persist: PERSIST_LOCAL_MACHINE,
            attr_count: 0,
            attrs: std::ptr::null_mut(),
            target_alias: std::ptr::null_mut(),
            user_name: user.as_mut_ptr(),
        };
        if unsafe { CredWriteW(&cred, 0) } == 0 {
            return Err(StoreError::Other(format!("CredWriteW: {}", last_err())));
        }
        Ok(())
    }

    pub fn get(service: &str, account: &str) -> Result<String, StoreError> {
        let t = target(service, account);
        let mut out: *mut CredentialW = std::ptr::null_mut();
        if unsafe { CredReadW(t.as_ptr(), GENERIC, 0, &mut out) } == 0 {
            return if last_err() == ERROR_NOT_FOUND {
                Err(StoreError::NotFound)
            } else {
                Err(StoreError::Other(format!("CredReadW: {}", last_err())))
            };
        }
        let s = unsafe {
            let c = &*out;
            let bytes = std::slice::from_raw_parts(c.blob, c.blob_size as usize);
            String::from_utf8_lossy(bytes).into_owned()
        };
        unsafe { CredFree(out as *mut core::ffi::c_void) };
        Ok(s)
    }

    pub fn delete(service: &str, account: &str) -> Result<(), StoreError> {
        let t = target(service, account);
        if unsafe { CredDeleteW(t.as_ptr(), GENERIC, 0) } == 0 {
            return if last_err() == ERROR_NOT_FOUND {
                Err(StoreError::NotFound)
            } else {
                Err(StoreError::Other(format!("CredDeleteW: {}", last_err())))
            };
        }
        Ok(())
    }
}

fn kr_set(name: &str, variant: &str, val: &str) -> Result<(), StoreError> {
    store::set(SERVICE, &format!("{name}/{variant}"), val)
}
fn kr_get(name: &str, variant: &str) -> Result<String, StoreError> {
    store::get(SERVICE, &format!("{name}/{variant}"))
}
fn kr_del(name: &str, variant: &str) -> Result<(), StoreError> {
    store::delete(SERVICE, &format!("{name}/{variant}"))
}

// ---------------- CLI ----------------

fn main() {
    std::process::exit(run(std::env::args().skip(1).collect()));
}

fn run(args: Vec<String>) -> i32 {
    if args.is_empty() {
        usage();
        return 2;
    }
    match args[0].as_str() {
        "set" => cmd_set(&args[1..]),
        "get" => cmd_get(&args[1..]),
        "list" | "ls" => cmd_list(&args[1..]),
        "del" | "rm" | "delete" => cmd_del(&args[1..]),
        "run" => cmd_run(&args[1..]),
        "stats" => cmd_stats(&args[1..]),
        "log" => cmd_log(&args[1..]),
        "-h" | "--help" | "help" => {
            usage();
            0
        }
        other => {
            eprintln!("unknown command {other:?}");
            usage();
            2
        }
    }
}

fn usage() {
    eprint!(
        "skey - tiny OS-keyring env vault (rust)\n\
         usage:\n  \
           skey set <name> [variant]        store value (secure prompt, no echo)\n  \
           skey get <name> [variant|N] [-n] print value, no newline unless -n/--newline\n  \
           get always asks for confirmation (no --yes bypass)\n  \
           skey run <name> [variant] --env VAR [-y] -- <cmd> [args...]\n  \
           inject secret as $VAR into child only, redact output\n  \
           skey list [name]  (alias: ls)    list names / variants, never secrets\n  \
           skey del <name> [variant|N]      delete variant or whole name\n  \
           skey stats                       per-entry get/run counts + last caller\n  \
           skey log [-n N] [--json]         recent audit trail (who ran what, exact argv)\n\
         variant: label (prod, dev, ...) or 1-based index; omitted+multiple => picker\n\
         examples:\n  \
           skey set mistral prod\n  \
           MISTRAL_API_KEY=$(skey get mistral prod) pi\n  \
           skey run mistral prod --env MISTRAL_API_KEY -- curl args...\n"
    );
}

fn valid(s: &str) -> bool {
    !s.is_empty()
        && s.len() <= 64
        && s.bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'.' || b == b'_' || b == b'-')
}

// ---------- index (same path/format as Go: {"names": {name: [variants]}}) ----------

#[derive(Debug, Default, Serialize, Deserialize)]
struct Index {
    #[serde(default)]
    names: HashMap<String, Vec<String>>,
}

/// Config dir, honouring SKEY_CONFIG_DIR or SENV_CONFIG_DIR for backwards-compat/tests.
fn skey_dir() -> io::Result<std::path::PathBuf> {
    if let Ok(d) = std::env::var("SKEY_CONFIG_DIR").or_else(|_| std::env::var("SENV_CONFIG_DIR")) {
        if !d.is_empty() {
            return Ok(std::path::PathBuf::from(d));
        }
    }
    let dir =
        dirs::config_dir().ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, "no config dir"))?;
    Ok(dir.join("skey"))
}

/// Mirror of Go's os.UserConfigDir + skey/index.json.
fn index_path() -> io::Result<std::path::PathBuf> {
    Ok(skey_dir()?.join("index.json"))
}

fn load_index() -> io::Result<Index> {
    let mut ix = Index::default();
    let p = index_path()?;
    let b = match std::fs::read(&p) {
        Ok(b) => b,
        Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(ix),
        Err(e) => return Err(e),
    };
    if b.is_empty() {
        return Ok(ix);
    }
    ix = serde_json::from_slice(&b)
        .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, format!("corrupt index {}: {e}", p.display())))?;
    Ok(ix)
}

fn save_index(ix: &Index) -> io::Result<()> {
    let p = index_path()?;
    if let Some(parent) = p.parent() {
        std::fs::create_dir_all(parent)?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(parent, std::fs::Permissions::from_mode(0o700))?;
        }
    }
    let tmp = p.with_extension("json.tmp");
    std::fs::write(&tmp, serde_json::to_string_pretty(ix).unwrap())?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&tmp, std::fs::Permissions::from_mode(0o600))?;
    }
    std::fs::rename(&tmp, &p)?;
    Ok(())
}

fn add_variant(ix: &mut Index, name: &str, variant: &str) {
    let vs = ix.names.entry(name.to_string()).or_default();
    if !vs.iter().any(|v| v == variant) {
        vs.push(variant.to_string());
        vs.sort();
    }
}

fn remove_variant(ix: &mut Index, name: &str, variant: &str) {
    if variant.is_empty() {
        ix.names.remove(name);
        return;
    }
    if let Some(vs) = ix.names.get_mut(name) {
        vs.retain(|v| v != variant);
        if vs.is_empty() {
            ix.names.remove(name);
        }
    }
}

fn sorted_variants(ix: &Index, name: &str) -> Vec<String> {
    let mut vs = ix.names.get(name).cloned().unwrap_or_default();
    vs.sort();
    vs
}

/// "" | label | 1-based index (into sorted list) -> concrete variant.
/// Exact label wins over numeric index. ("", false) = ambiguous -> picker.
fn resolve_variant(variants: &[String], arg: &str) -> (String, bool) {
    if arg.is_empty() {
        if variants.len() == 1 {
            return (variants[0].clone(), true);
        }
        return (String::new(), false);
    }
    if variants.iter().any(|v| v == arg) {
        return (arg.to_string(), true);
    }
    if let Ok(n) = arg.parse::<usize>() {
        if n >= 1 && n <= variants.len() {
            return (variants[n - 1].clone(), true);
        }
    }
    (String::new(), false)
}

/// Numbered picker on stderr; reads one line from /dev/tty (else stdin)
/// so `$(senv get x)` pipelines keep working.
fn pick_variant(name: &str, variants: &[String]) -> Option<String> {
    let mut sorted = variants.to_vec();
    sorted.sort();
    eprintln!("multiple variants for {name:?}:");
    for (i, v) in sorted.iter().enumerate() {
        eprintln!("  {}) {v}", i + 1);
    }
    eprint!("choose [1-{}]: ", sorted.len());
    let _ = io::stderr().flush();
    let line = read_picker_line()?;
    let idx: usize = line.trim().parse().ok()?;
    if idx < 1 || idx > sorted.len() {
        return None;
    }
    Some(sorted[idx - 1].clone())
}

fn read_picker_line() -> Option<String> {
    // Prefer /dev/tty so command substitution keeps working.
    #[cfg(unix)]
    if let Ok(f) = std::fs::File::open("/dev/tty") {
        let mut r = io::BufReader::new(f);
        let mut line = String::new();
        if r.read_line(&mut line).is_ok() && !line.trim().is_empty() {
            return Some(line);
        }
        return None;
    }
    let mut line = String::new();
    if io::stdin().lock().read_line(&mut line).is_ok() && !line.trim().is_empty() {
        return Some(line);
    }
    None
}

// ---------- commands ----------

/// Confirm gate for `get`: prompt on /dev/tty via stderr (never stdout),
/// approve only on explicit y/yes. Never prints the secret.
fn confirm_access(name: &str, variant: &str) -> bool {
    eprint!("Allow access to {name}/{variant}? [y/N]: ");
    let _ = io::stderr().flush();
    let Some(line) = read_confirm_line() else {
        eprintln!("denied (no interactive terminal; automation must use `skey run`)");
        return false;
    };
    match line.trim().to_ascii_lowercase().as_str() {
        "y" | "yes" => true,
        _ => {
            eprintln!("denied");
            false
        }
    }
}

fn read_confirm_line() -> Option<String> {
    #[cfg(unix)]
    if let Ok(f) = std::fs::File::open("/dev/tty") {
        let mut r = io::BufReader::new(f);
        let mut line = String::new();
        match r.read_line(&mut line) {
            Ok(_) if !line.is_empty() => return Some(line),
            _ => return None,
        }
    }
    // Fallback: stdin only when it is a real tty (never pipes).
    if !io::stdin().is_terminal() {
        return None;
    }
    let mut line = String::new();
    match io::stdin().lock().read_line(&mut line) {
        Ok(_) if !line.is_empty() => Some(line),
        _ => None,
    }
}

fn cmd_set(args: &[String]) -> i32 {
    if args.is_empty() || args.len() > 2 {
        eprintln!("usage: skey set <name> [variant]");
        return 2;
    }
    let (name, variant) = (
        args[0].as_str(),
        args.get(1).map(|s| s.as_str()).unwrap_or(DEFAULT_VARIANT),
    );
    if !valid(name) || !valid(variant) {
        eprintln!("error: name/variant must match [A-Za-z0-9._-]{{1,64}}");
        return 2;
    }
    eprint!("value for {name}/{variant} (hidden): ");
    let _ = io::stderr().flush();
    // Interactive tty: hidden prompt. Piped stdin (tests/scripts):
    // read one line so values can be fed without a tty.
    let val = if io::stdin().is_terminal() {
        match rpassword::read_password() {
            Ok(p) => {
                eprintln!();
                p.trim_end_matches(['\r', '\n']).to_string()
            }
            Err(e) => {
                eprintln!("\nerror: prompt failed: {e}");
                return 1;
            }
        }
    } else {
        eprintln!();
        eprintln!("(no tty - reading piped line)");
        let mut line = String::new();
        match io::stdin().lock().read_line(&mut line) {
            Ok(0) | Err(_) => {
                eprintln!("error: empty value, aborted");
                return 1;
            }
            Ok(_) => line.trim_end_matches(['\r', '\n']).to_string(),
        }
    };
    if val.is_empty() {
        eprintln!("error: empty value, aborted");
        return 1;
    }
    if let Err(e) = kr_set(name, variant, &val) {
        eprintln!("error: store failed: {e}");
        return 1;
    }
    match load_index() {
        Ok(mut ix) => {
            add_variant(&mut ix, name, variant);
            if let Err(e) = save_index(&ix) {
                eprintln!("warning: stored but index save failed: {e}");
            }
        }
        Err(e) => eprintln!("warning: stored but index update failed: {e}"),
    }
    eprintln!("stored {name}/{variant}");
    0
}

fn cmd_get(args: &[String]) -> i32 {
    let mut newline = false;
    let mut rest: Vec<&str> = vec![];
    for a in args {
        match a.as_str() {
            "-n" | "--newline" => newline = true,
            "-y" | "--yes" | "--no-confirm" => {
                eprintln!("error: skey get has no --yes flag (always confirms; automation must use `skey run`)");
                return 2;
            }
            _ => rest.push(a),
        }
    }
    if rest.is_empty() || rest.len() > 2 {
        eprintln!("usage: skey get <name> [variant|N] [-n]");
        return 2;
    }
    let (name, varg) = (rest[0], rest.get(1).copied().unwrap_or(""));
    let ix = load_index().unwrap_or_default();
    let variants = sorted_variants(&ix, name);
    let (mut variant, mut ok) = resolve_variant(&variants, varg);
    if !ok && variants.len() > 1 && varg.is_empty() {
        match pick_variant(name, &variants) {
            Some(p) => {
                variant = p;
                ok = true;
            }
            None => {
                eprintln!("cancelled");
                return 1;
            }
        }
    }
    if !ok {
        if varg.is_empty() {
            variant = DEFAULT_VARIANT.to_string(); // single/unknown: try default
        } else {
            variant = varg.to_string(); // explicit label: try verbatim
        }
    }
    if !confirm_access(name, &variant) {
        record_call("get", name, &variant, "denied", "confirm-denied", "", "", None, None);
        return 1;
    }
    match kr_get(name, &variant) {
        Ok(val) => {
            record_call("get", name, &variant, "allowed", "", "", "", None, None);
            print!("{val}");
            if newline {
                println!();
            } else {
                let _ = io::stdout().flush();
            }
            0
        }
        Err(_) => {
            eprintln!(
                "error: no such entry {name}/{variant} (hint: `skey list` / `skey set {name} {variant}`)"
            );
            record_call("get", name, &variant, "denied", "not-found", "", "", None, None);
            1
        }
    }
}

fn cmd_list(args: &[String]) -> i32 {
    let ix = match load_index() {
        Ok(ix) => ix,
        Err(e) => {
            eprintln!("error: {e}");
            return 1;
        }
    };
    if args.len() == 1 {
        let name = &args[0];
        let vs = sorted_variants(&ix, name);
        if vs.is_empty() {
            eprintln!("no such entry {name:?}");
            return 1;
        }
        for (i, v) in vs.iter().enumerate() {
            println!("{}) {name}/{v}", i + 1);
        }
        return 0;
    }
    if args.len() > 1 {
        eprintln!("usage: skey list [name]");
        return 2;
    }
    let mut names: Vec<&String> = ix.names.keys().collect();
    names.sort();
    if names.is_empty() {
        eprintln!("(empty — `skey set <name> [variant]` to add)");
        return 0;
    }
    for n in names {
        let mut vs = ix.names.get(n).cloned().unwrap_or_default();
        vs.sort();
        println!("{n}: {}", vs.join(", "));
    }
    0
}

/// `senv run <name> [variant] --env VAR [--allowlist a,b] [--redact/--no-redact]
/// [-y] -- <cmd> [args...]` — use-without-see. Mirrors Go senv/run.go:
/// secret only in child env (never argv), direct exec (no shell), child
/// stdout/stderr captured and redacted (raw+base64+url forms), exit-code
/// passthrough, confirm gate, allowlist deny BEFORE fetch, metadata-only audit.
fn cmd_run(args: &[String]) -> i32 {
    let mut env_var = String::new();
    let mut allowlist = String::new();
    let mut redact = true;
    let mut yes = false;
    for i in 0..args.len() {
        let a = args[i].as_str();
        if a == "--env" {
            if i + 1 < args.len() {
                env_var = args[i + 1].clone();
            }
        } else if let Some(v) = a.strip_prefix("--env=") {
            env_var = v.to_string();
        } else if a == "--allowlist" || a == "--allow" {
            if i + 1 < args.len() {
                allowlist = args[i + 1].clone();
            }
        } else if let Some(v) = a.strip_prefix("--allowlist=") {
            allowlist = v.to_string();
        }
        // NB: `--allow=X` is NOT accepted (Go parity: only `--allow X`).
        else if a == "--redact" {
            redact = true;
        } else if a == "--no-redact" {
            redact = false;
        } else if a == "-y" || a == "--yes" || a == "--no-confirm" {
            yes = true;
        }
    }
    // Skip flag *values* when re-scanning head below: --env/--allowlist/--allow
    // consume the next arg (Go parity).
    let sep = args.iter().position(|a| a == "--");
    let Some(sep) = sep else {
        eprintln!(
            "usage: skey run <name> [variant] --env VAR [--allowlist a,b] [--no-redact] [-y] -- <cmd> [args...]"
        );
        return 2;
    };
    let mut head: Vec<&str> = vec![];
    let mut i = 0;
    while i < sep {
        let a = args[i].as_str();
        if a == "--env" || a == "--allowlist" || a == "--allow" {
            i += 2;
            continue;
        }
        if a.starts_with("--env=")
            || a.starts_with("--allowlist=")
            || a == "--redact"
            || a == "--no-redact"
            || a == "-y"
            || a == "--yes"
            || a == "--no-confirm"
        {
            i += 1;
            continue;
        }
        head.push(a);
        i += 1;
    }
    let cmd_args: Vec<&str> = args[sep + 1..].iter().map(|s| s.as_str()).collect();
    if head.is_empty() || head.len() > 2 {
        eprintln!("usage: skey run <name> [variant] --env VAR -- <cmd> [args...]");
        return 2;
    }
    if cmd_args.is_empty() {
        eprintln!("error: no command after --");
        return 2;
    }
    if env_var.is_empty() {
        eprintln!("error: --env VAR is required");
        return 2;
    }
    if !valid_env_name(&env_var) {
        eprintln!("error: --env must match [A-Za-z_][A-Za-z0-9_]*");
        return 2;
    }
    let (name, varg) = (head[0], if head.len() == 2 { head[1] } else { "" });

    // Allowlist (basename match) BEFORE touching the secret.
    if !allowlist.is_empty() {
        let base = cmd_args[0].rsplit('/').next().unwrap_or(cmd_args[0]);
        let hit = allowlist.split(',').map(|c| c.trim()).any(|c| {
            if c.is_empty() {
                return false;
            }
            c == cmd_args[0] || c.rsplit('/').next().unwrap_or(c) == base
        });
        if !hit {
            eprintln!("denied: {:?} not in --allowlist {:?}", cmd_args[0], allowlist);
            let base = cmd_args[0].rsplit('/').next().unwrap_or(cmd_args[0]);
            record_call("run", name, varg, "denied", "allowlist-denied", &env_var, base, Some(redact), None);
            return 1;
        }
    }

    // Resolve variant (same rules as get).
    let ix = load_index().unwrap_or_default();
    let variants = sorted_variants(&ix, name);
    let (mut variant, mut ok) = resolve_variant(&variants, varg);
    if !ok && variants.len() > 1 && varg.is_empty() {
        match pick_variant(name, &variants) {
            Some(p) => {
                variant = p;
                ok = true;
            }
            None => {
                eprintln!("cancelled");
                let base = cmd_args[0].rsplit('/').next().unwrap_or(cmd_args[0]);
                record_call("run", name, varg, "denied", "picker-cancelled", &env_var, base, Some(redact), None);
                return 1;
            }
        }
    }
    if !ok {
        if varg.is_empty() {
            variant = DEFAULT_VARIANT.to_string();
        } else {
            variant = varg.to_string();
            ok = true;
        }
    }
    let _ = ok;

    if !yes && !confirm_access(name, &variant) {
        let base = cmd_args[0].rsplit('/').next().unwrap_or(cmd_args[0]);
        record_call("run", name, &variant, "denied", "confirm-denied", &env_var, base, Some(redact), None);
        return 1;
    }
    let secret = match kr_get(name, &variant) {
        Ok(v) => v,
        Err(_) => {
            eprintln!(
                "error: no such entry {name}/{variant} (hint: `skey list` / `skey set {name} {variant}`)"
            );
            let base = cmd_args[0].rsplit('/').next().unwrap_or(cmd_args[0]);
            record_call("run", name, &variant, "denied", "not-found", &env_var, base, Some(redact), None);
            return 1;
        }
    };

    // Direct exec — NO shell. Secret only in child env.
    use std::process::{Command, Stdio};
    let out = Command::new(cmd_args[0])
        .args(&cmd_args[1..])
        .env(&env_var, &secret)
        .stdin(Stdio::inherit())
        .output();
    let out = match out {
        Ok(o) => o,
        Err(e) => {
            eprintln!("error: run failed: {e}");
            run_audit_log(name, &variant, &env_var, cmd_args[0], redact, 1);
            zeroize(secret);
            return 1;
        }
    };
    let code = out.status.code().unwrap_or(1);
    let mut sout = out.stdout;
    let mut serr = out.stderr;
    if redact {
        sout = redact_bytes(&sout, &secret);
        serr = redact_bytes(&serr, &secret);
    }
    let _ = io::stdout().write_all(&sout);
    let _ = io::stdout().flush();
    let _ = io::stderr().write_all(&serr);
    run_audit_log(name, &variant, &env_var, cmd_args[0], redact, code);
    // Keep secret out of memory ASAP (best effort; allocator may retain copies).
    zeroize(secret);
    code
}

fn valid_env_name(s: &str) -> bool {
    if s.is_empty() || s.len() > 128 {
        return false;
    }
    let mut cs = s.chars();
    match cs.next() {
        Some(c) if c == '_' || c.is_ascii_alphabetic() => {}
        _ => return false,
    }
    cs.all(|c| c == '_' || c.is_ascii_alphanumeric())
}

fn replace_all_bytes(hay: &[u8], needle: &[u8], repl: &[u8]) -> Vec<u8> {
    if needle.is_empty() {
        return hay.to_vec();
    }
    let mut out: Vec<u8> = Vec::with_capacity(hay.len());
    let mut i = 0;
    while i + needle.len() <= hay.len() {
        if &hay[i..i + needle.len()] == needle {
            out.extend_from_slice(repl);
            i += needle.len();
        } else {
            out.push(hay[i]);
            i += 1;
        }
    }
    out.extend_from_slice(&hay[i..]);
    out
}

/// Mirror of Go redactBytes: raw + base64 std/url (+raw unpadded) + QueryEscape.
/// Skips short secrets the same way Go does for multi-form (Go redacts <4
/// as raw-only; we do the same).
fn redact_bytes(b: &[u8], secret: &str) -> Vec<u8> {
    use base64::{Engine as _, engine::general_purpose::{STANDARD, URL_SAFE, STANDARD_NO_PAD, URL_SAFE_NO_PAD}};
    if secret.len() < 4 {
        return replace_all_bytes(b, secret.as_bytes(), b"[REDACTED]");
    }
    let mut forms: Vec<String> = vec![secret.to_string()];
    forms.push(STANDARD.encode(secret.as_bytes()));
    forms.push(URL_SAFE.encode(secret.as_bytes()));
    forms.push(STANDARD_NO_PAD.encode(secret.as_bytes()));
    forms.push(URL_SAFE_NO_PAD.encode(secret.as_bytes()));
    let q = query_escape(secret);
    if q != secret {
        forms.push(q);
    }
    forms.sort_by_key(|a| std::cmp::Reverse(a.len())); // longest first
    let mut out = b.to_vec();
    for f in &forms {
        if f.is_empty() {
            continue;
        }
        out = replace_all_bytes(&out, f.as_bytes(), b"[REDACTED]");
    }
    out
}

/// Minimal clone of Go url.QueryEscape: [A-Za-z0-9-_.~] stay, space -> '+',
/// rest -> %XX (uppercase hex). Byte-oriented like Go (non-ASCII -> %XX/byte).
fn query_escape(s: &str) -> String {
    let mut o = String::with_capacity(s.len());
    for b in s.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => o.push(b as char),
            b' ' => o.push('+'),
            _ => {
                o.push('%');
                o.push(char::from_digit((b >> 4) as u32, 16).unwrap().to_ascii_uppercase());
                o.push(char::from_digit((b & 0xF) as u32, 16).unwrap().to_ascii_uppercase());
            }
        }
    }
    o
}

/// In-program call counter + audit trail (mirrors Go senv/stats.go).
/// Every get/run call records, from INSIDE the process: timestamp, pid, ppid
/// + parent name + short chain (best effort), user, cwd, exe, tty hint, the
/// EXACT argv, subcommand, name/variant (NEVER the secret), decision/reason,
/// plus run extras (env name, child basename, redact, exit). Append-only
/// JSON Lines in senv/audit.log (0600); counters in senv/counts.json.
/// Legacy pre-JSON text lines are kept and still shown by `senv log`.
fn parent_comm(pid: u32) -> String {
    if pid == 0 {
        return String::new();
    }
    #[cfg(target_os = "linux")]
    if let Ok(b) = std::fs::read(format!("/proc/{pid}/comm")) {
        return String::from_utf8_lossy(&b).trim().to_string();
    }
    match std::process::Command::new("ps")
        .args(["-p", &pid.to_string(), "-o", "comm="])
        .output()
    {
        Ok(o) => String::from_utf8_lossy(&o.stdout).trim().to_string(),
        Err(_) => String::new(),
    }
}

fn caller_chain(ppid: u32) -> Vec<String> {
    let mut chain = Vec::new();
    let mut pid = ppid;
    for _ in 0..4 {
        if pid == 0 {
            break;
        }
        let name = parent_comm(pid);
        if name.is_empty() {
            chain.push(format!("pid={pid}"));
        } else {
            chain.push(format!("{name}({pid})"));
        }
        // Next ancestor via ps (best effort; stop on failure).
        let next = std::process::Command::new("ps")
            .args(["-p", &pid.to_string(), "-o", "ppid="])
            .output()
            .ok()
            .and_then(|o| String::from_utf8_lossy(&o.stdout).trim().parse::<u32>().ok())
            .unwrap_or(0);
        if next == pid {
            break;
        }
        pid = next;
    }
    chain
}

fn tty_hint() -> String {
    if let Ok(t) = std::env::var("SSH_TTY") {
        if !t.is_empty() {
            return t;
        }
    }
    #[cfg(unix)]
    if std::fs::File::open("/dev/tty").is_ok() {
        return "dev-tty".to_string();
    }
    "no-tty".to_string()
}

fn user_name() -> String {
    if let Ok(u) = std::env::var("USER") {
        if !u.is_empty() {
            return u;
        }
    }
    "".to_string()
}

fn quote_argv(argv: &[String]) -> String {
    argv
        .iter()
        .map(|a| {
            if a.is_empty() {
                "''".to_string()
            } else if a.chars().any(|c| " \t\n\"'\\$`!*?()[]{}<>|&;".contains(c)) {
                format!("'{}'", a.replace('\'', "'\\''"))
            } else {
                a.clone()
            }
        })
        .collect::<Vec<_>>()
        .join(" ")
}

#[derive(Debug, Serialize, Deserialize)]
struct AuditEntry {
    ts: String,
    pid: u32,
    ppid: u32,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pname: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    chain: Vec<String>,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    user: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    cwd: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    exe: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    tty: String,
    argv: Vec<String>,
    argv_str: String,
    sub: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    name: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    variant: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    env: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    cmd: String,
    decision: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    reason: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    redact: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    exit: Option<i32>,
}

#[derive(Debug, Default, Serialize, Deserialize)]
struct CountRec {
    #[serde(default)]
    count: u64,
    #[serde(default)]
    last_ts: String,
    #[serde(default)]
    last_caller: String,
    #[serde(default)]
    last_argv: String,
    #[serde(default)]
    last_decision: String,
}

/// Append one JSON audit line + bump the persistent counter. Never logs secrets.
#[allow(clippy::too_many_arguments)]
fn record_call(
    sub: &str,
    name: &str,
    variant: &str,
    decision: &str,
    reason: &str,
    env_var: &str,
    cmd: &str,
    redact: Option<bool>,
    exit: Option<i32>,
) {
    let pid = std::process::id();
    // getppid without libc: ask ps about ourselves (best effort).
    let ppid = std::process::Command::new("ps")
        .args(["-p", &pid.to_string(), "-o", "ppid="])
        .output()
        .ok()
        .and_then(|o| String::from_utf8_lossy(&o.stdout).trim().parse::<u32>().ok())
        .unwrap_or(0);
    let argv: Vec<String> = std::env::args().collect();
    let e = AuditEntry {
        ts: rfc3339_now(),
        pid,
        ppid,
        pname: parent_comm(ppid),
        chain: caller_chain(ppid),
        user: user_name(),
        cwd: std::env::current_dir().map(|p| p.display().to_string()).unwrap_or_default(),
        exe: std::env::current_exe().map(|p| p.display().to_string()).unwrap_or_default(),
        tty: tty_hint(),
        argv_str: quote_argv(&argv),
        argv,
        sub: sub.to_string(),
        name: name.to_string(),
        variant: variant.to_string(),
        env: env_var.to_string(),
        cmd: cmd.to_string(),
        decision: decision.to_string(),
        reason: reason.to_string(),
        redact,
        exit,
    };
    let dir = match skey_dir() {
        Ok(d) => d,
        Err(_) => return,
    };
    let _ = std::fs::create_dir_all(&dir);
    let line = match serde_json::to_string(&e) {
        Ok(mut s) => {
            s.push('\n');
            s
        }
        Err(_) => return,
    };
    use std::io::Write as _;
    use std::os::unix::fs::OpenOptionsExt;
    let mut opts = std::fs::OpenOptions::new();
    opts.append(true).create(true).write(true);
    #[cfg(unix)]
    opts.mode(0o600);
    if let Ok(mut f) = opts.open(dir.join("audit.log")) {
        let _ = f.write_all(line.as_bytes());
    }
    // Persistent counter.
    let cp = dir.join("counts.json");
    let mut m: HashMap<String, CountRec> = std::fs::read(&cp)
        .ok()
        .and_then(|b| if b.is_empty() { Some(HashMap::new()) } else { serde_json::from_slice(&b).ok() })
        .unwrap_or_default();
    let key = format!("{sub} {name}/{variant}");
    let r = m.entry(key).or_default();
    r.count += 1;
    r.last_ts = e.ts.clone();
    r.last_caller = if e.pname.is_empty() {
        format!("{} pid={} ppid={}", e.user, e.pid, e.ppid)
    } else {
        format!("{} by {}({})", e.user, e.pname, e.ppid)
    };
    r.last_argv = e.argv_str.clone();
    r.last_decision = e.decision.clone();
    if let Ok(b) = serde_json::to_string_pretty(&m) {
        let tmp = dir.join("counts.json.tmp");
        let _ = std::fs::write(&tmp, b);
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let _ = std::fs::set_permissions(&tmp, std::fs::Permissions::from_mode(0o600));
        }
        let _ = std::fs::rename(&tmp, &cp);
    }
}

/// Back-compat wrapper kept at the old call sites: successful run.
fn run_audit_log(name: &str, variant: &str, env_var: &str, cmd: &str, redact: bool, code: i32) {
    let base = cmd.rsplit('/').next().unwrap_or(cmd);
    record_call("run", name, variant, "allowed", "", env_var, base, Some(redact), Some(code));
}

fn cmd_stats(args: &[String]) -> i32 {
    if !args.is_empty() {
        eprintln!("usage: skey stats");
        return 2;
    }
    let dir = match skey_dir() {
        Ok(d) => d,
        Err(e) => {
            eprintln!("error: {e}");
            return 1;
        }
    };
    let b = match std::fs::read(dir.join("counts.json")) {
        Ok(b) if !b.is_empty() => b,
        _ => {
            println!("(no recorded get/run calls yet)");
            return 0;
        }
    };
    let m: HashMap<String, CountRec> = match serde_json::from_slice(&b) {
        Ok(m) => m,
        Err(e) => {
            eprintln!("error: corrupt counts file: {e}");
            return 1;
        }
    };
    let mut keys: Vec<&String> = m.keys().collect();
    keys.sort();
    for k in keys {
        let r = &m[k];
        println!("{}: {} (last {} {}; {})\n  $ {}", k, r.count, r.last_ts, r.last_decision, r.last_caller, r.last_argv);
    }
    0
}

fn cmd_log(args: &[String]) -> i32 {
    let mut n: usize = 20;
    let mut json_out = false;
    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "--json" => json_out = true,
            "-n" | "--tail" => {
                i += 1;
                match args.get(i).and_then(|s| s.parse::<usize>().ok()) {
                    Some(v) => n = v,
                    None => {
                        eprintln!("error: -n needs a non-negative number");
                        return 2;
                    }
                }
            }
            a if a.starts_with("-n=") => match a[3..].parse::<usize>() {
                Ok(v) => n = v,
                Err(_) => {
                    eprintln!("error: -n needs a non-negative number");
                    return 2;
                }
            },
            _ => {
                eprintln!("usage: skey log [-n N|--tail N] [--json]");
                return 2;
            }
        }
        i += 1;
    }
    let dir = match skey_dir() {
        Ok(d) => d,
        Err(e) => {
            eprintln!("error: {e}");
            return 1;
        }
    };
    let b = match std::fs::read(dir.join("audit.log")) {
        Ok(b) => b,
        Err(_) => {
            eprintln!("(empty audit log)");
            return 0;
        }
    };
    let text = String::from_utf8_lossy(&b);
    let lines: Vec<&str> = text.trim_end_matches('\n').split('\n').collect();
    if lines.len() == 1 && lines[0].is_empty() {
        eprintln!("(empty audit log)");
        return 0;
    }
    let start = lines.len().saturating_sub(n);
    for ln in &lines[start..] {
        if json_out {
            println!("{ln}");
            continue;
        }
        match serde_json::from_str::<AuditEntry>(ln) {
            Ok(e) => {
                let mut who = e.user.clone();
                if !e.pname.is_empty() {
                    who += &format!(" via {}({})", e.pname, e.ppid);
                }
                let extra = if e.cmd.is_empty() { String::new() } else { format!(" cmd={}", e.cmd) };
                println!("{} {} {} {}/{}{} by {}\n  $ {}", e.ts, e.decision, e.sub, e.name, e.variant, extra, who, e.argv_str);
            }
            Err(_) => println!("{ln}"), // legacy pre-JSON text line
        }
    }
    0
}

/// UTC RFC3339 (`YYYY-MM-DDTHH:MM:SSZ`) without extra deps (Hinnant days algo).
fn rfc3339_now() -> String {
    use std::time::{SystemTime, UNIX_EPOCH};
    let secs = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0);
    let (days, tod) = (secs.div_euclid(86400), secs.rem_euclid(86400));
    let z = days + 719468;
    let era = z.div_euclid(146097);
    let doe = z.rem_euclid(146097);
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = if m <= 2 { y + 1 } else { y };
    format!(
        "{:04}-{:02}-{:02}T{:02}:{:02}:{:02}Z",
        y,
        m,
        d,
        tod / 3600,
        (tod % 3600) / 60,
        tod % 60
    )
}

/// Best-effort secret wipe (allocator/GC caveat, same as Go).
fn zeroize(mut s: String) {
    unsafe {
        let b = s.as_bytes_mut();
        std::ptr::write_volatile(b.as_mut_ptr(), 0u8);
        for i in 0..b.len() {
            std::ptr::write_volatile(b.as_mut_ptr().add(i), 0);
        }
    }
}

fn cmd_del(args: &[String]) -> i32 {
    if args.is_empty() || args.len() > 2 {
        eprintln!("usage: skey del <name> [variant|N]");
        return 2;
    }
    let (name, mut varg) = (args[0].as_str(), args.get(1).map(|s| s.as_str()).unwrap_or(""));
    let mut ix = match load_index() {
        Ok(ix) => ix,
        Err(e) => {
            eprintln!("error: {e}");
            return 1;
        }
    };
    let variants = sorted_variants(&ix, name);
    if varg.is_empty() && variants.len() > 1 {
        match pick_variant(name, &variants) {
            Some(p) => {
                varg = Box::leak(p.into_boxed_str()); // resolve below via label path
            }
            None => {
                eprintln!("cancelled");
                return 1;
            }
        }
    }
    if varg.is_empty() {
        // delete whole name: all known variants, else try default
        let targets = if variants.is_empty() {
            vec![DEFAULT_VARIANT.to_string()]
        } else {
            variants
        };
        for v in &targets {
            let _ = kr_del(name, v);
        }
        remove_variant(&mut ix, name, "");
        let _ = save_index(&ix);
        eprintln!("deleted {name}");
        return 0;
    }
    let (variant, ok) = resolve_variant(&sorted_variants(&ix, name), varg);
    let variant = if ok { variant } else { varg.to_string() }; // explicit label: verbatim
    if kr_del(name, &variant).is_err() {
        eprintln!("error: no such entry {name}/{variant}");
        return 1;
    }
    remove_variant(&mut ix, name, &variant);
    let _ = save_index(&ix);
    eprintln!("deleted {name}/{variant}");
    0
}
