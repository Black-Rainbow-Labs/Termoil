//! `terminus ssh [ssh arguments...]`: copies the `xterm-terminus` terminfo entry to the remote host
//! before connecting, then runs ssh with `TERM=xterm-terminus`. When the copy cannot be done the
//! connection still opens, with `TERM=xterm-256color`, which every remote host knows.

use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, ExitCode, Stdio};

/// The TERM value for a session on a host that has the entry.
const TERM_WITH_ENTRY: &str = "xterm-terminus";
/// The TERM value for a session on a host that does not have the entry.
const TERM_FALLBACK: &str = "xterm-256color";
/// The file under the data folder that lists the hosts known to have the entry, one per line.
const CACHE_FILE: &str = "ssh-terminfo-hosts.txt";
/// The flag that turns the copy off. It is read only as the first argument.
const NO_TERMINFO_FLAG: &str = "--no-terminfo";
/// The terminfo source file inside the shipped `shell-integration` folder.
const TERMINFO_SOURCE: &str = "terminfo/xterm-terminus.terminfo";
/// The port ssh uses when none is given.
const DEFAULT_PORT: &str = "22";
/// The command run on the remote host: sh in every case, whatever the login shell is.
const REMOTE_COPY_COMMAND: &str = "sh -c 'mkdir -p ~/.terminfo && tic -x -o ~/.terminfo /dev/stdin'";
/// The ssh options that take a value, as the letters OpenSSH uses.
const OPTIONS_WITH_VALUE: &str = "BbcDEeFIiJLlmOopQRSWw";

/// What the ssh arguments say about where the connection goes.
#[derive(Debug, Default, PartialEq)]
pub struct Target {
    /// The host name or address, when a destination was found.
    pub host: Option<String>,
    /// The user from `user@host` or `-l`.
    pub user: Option<String>,
    /// The port from `-p` or an `ssh://` destination.
    pub port: Option<String>,
    /// The index in the arguments of the destination; a remote command follows it.
    pub dest_index: Option<usize>,
}

/// Finds the destination in `args`, skipping ssh's options and the values of the options that take
/// one. Letters may be combined (`-vp 2222`) and a value may be attached (`-p2222`).
pub fn parse_target(args: &[String]) -> Target {
    let mut target = Target::default();
    let mut i = 0;
    while i < args.len() {
        let a = args[i].as_str();
        if a == "--" {
            i += 1;
            if i < args.len() {
                apply_destination(&mut target, &args[i], i);
            }
            break;
        }
        if a.len() > 1 && a.starts_with('-') {
            i += skip_option_cluster(&mut target, a, args.get(i + 1).map(String::as_str));
        } else {
            apply_destination(&mut target, a, i);
            break;
        }
        i += 1;
    }
    target
}

/// Reads one option argument such as `-vp2222`, records `-l` and `-p`, and returns how many extra
/// arguments it consumed (1 when the value is the next argument).
fn skip_option_cluster(target: &mut Target, cluster: &str, next: Option<&str>) -> usize {
    let letters = &cluster[1..];
    for (pos, c) in letters.char_indices() {
        if !OPTIONS_WITH_VALUE.contains(c) {
            continue;
        }
        let attached = &letters[pos + c.len_utf8()..];
        let (value, used) = if attached.is_empty() { (next.unwrap_or(""), 1) } else { (attached, 0) };
        match c {
            'l' => target.user = Some(value.to_string()),
            'p' => target.port = Some(value.to_string()),
            _ => {}
        }
        return used;
    }
    0
}

/// Splits the destination (`host`, `user@host` or `ssh://user@host:port`) into the target.
fn apply_destination(target: &mut Target, dest: &str, index: usize) {
    target.dest_index = Some(index);
    let (rest, is_uri) = match dest.strip_prefix("ssh://") {
        Some(r) => (r.trim_end_matches('/'), true),
        None => (dest, false),
    };
    let (user, hostport) = match rest.rfind('@') {
        Some(at) => (Some(rest[..at].to_string()), &rest[at + 1..]),
        None => (None, rest),
    };
    let (host, port) = if is_uri { split_host_port(hostport) } else { (hostport.to_string(), None) };
    if user.is_some() {
        target.user = user;
    }
    if port.is_some() {
        target.port = port;
    }
    target.host = Some(host);
}

/// Splits `host:port`, `[v6]:port` or `host` from an ssh URI.
fn split_host_port(s: &str) -> (String, Option<String>) {
    if let Some(inner) = s.strip_prefix('[') {
        return match inner.split_once(']') {
            Some((h, rest)) => (h.to_string(), rest.strip_prefix(':').map(str::to_string)),
            None => (s.to_string(), None),
        };
    }
    match s.rsplit_once(':') {
        Some((h, p)) if !h.contains(':') => (h.to_string(), Some(p.to_string())),
        _ => (s.to_string(), None),
    }
}

/// The cache key for a connection, `user@host:port`.
pub fn cache_key(user: &str, host: &str, port: &str) -> String {
    format!("{user}@{host}:{port}")
}

/// Finds the ssh program on PATH: `ssh` on Unix, `ssh.exe` (or a `.cmd` or `.bat` stand in) on
/// Windows, where a bare name is not resolved to scripts.
fn find_ssh() -> PathBuf {
    let names: &[&str] = if cfg!(windows) { &["ssh.exe", "ssh.cmd", "ssh.bat"] } else { &["ssh"] };
    let path = std::env::var_os("PATH").unwrap_or_default();
    for dir in std::env::split_paths(&path) {
        for name in names {
            let candidate = dir.join(name);
            if candidate.is_file() {
                return candidate;
            }
        }
    }
    PathBuf::from("ssh")
}

/// Asks `ssh -G` what the arguments resolve to and returns the `hostname`, `user` and `port` it
/// prints, or None when ssh could not say.
fn resolve_with_ssh(ssh: &Path, args: &[String]) -> Option<(String, String, String)> {
    let out = Command::new(ssh).arg("-G").args(args).stdin(Stdio::null()).stderr(Stdio::null()).output().ok()?;
    if !out.status.success() {
        return None;
    }
    let text = String::from_utf8_lossy(&out.stdout);
    let field = |name: &str| {
        text.lines().find_map(|l| l.strip_prefix(name).and_then(|r| r.strip_prefix(' ')).map(|v| v.trim().to_string()))
    };
    Some((field("hostname")?, field("user").unwrap_or_default(), field("port").unwrap_or_default()))
}

/// The cache key for the connection: from `ssh -G` when it answers, else from the arguments.
fn connection_key(ssh: &Path, args: &[String], target: &Target) -> Option<String> {
    if let Some((host, user, port)) = resolve_with_ssh(ssh, args) {
        let port = if port.is_empty() { DEFAULT_PORT.to_string() } else { port };
        return Some(cache_key(&user, &host, &port));
    }
    let host = target.host.as_deref()?;
    let user = target.user.clone().or_else(local_user_name).unwrap_or_default();
    Some(cache_key(&user, host, target.port.as_deref().unwrap_or(DEFAULT_PORT)))
}

/// The local user name, which ssh uses when no user is given.
fn local_user_name() -> Option<String> {
    ["USER", "USERNAME"].iter().find_map(|v| std::env::var(v).ok()).filter(|s| !s.is_empty())
}

/// The cache file's path under the data folder.
fn cache_path() -> Option<PathBuf> {
    terminus_app::paths::data_dir().map(|d| d.join(CACHE_FILE))
}

/// True when `key` is listed in the cache file.
fn is_cached(key: &str) -> bool {
    let Some(path) = cache_path() else { return false };
    std::fs::read_to_string(path).is_ok_and(|t| t.lines().any(|l| l == key))
}

/// Adds `key` to the cache file, creating the data folder when needed.
fn remember(key: &str) {
    let Some(path) = cache_path() else { return };
    if let Some(dir) = path.parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    if let Ok(mut f) = std::fs::OpenOptions::new().create(true).append(true).open(&path) {
        let _ = writeln!(f, "{key}");
    }
}

/// Reads the terminfo source shipped with Terminus.
fn read_terminfo_source() -> Result<Vec<u8>, String> {
    let dir = terminus_app::shell_integration::source_dir().ok_or("the shipped terminfo source was not found")?;
    std::fs::read(dir.join(TERMINFO_SOURCE)).map_err(|e| format!("the terminfo source could not be read: {e}"))
}

/// Sends `source` to the remote host with one ssh command whose stdin is the source. The
/// connection options are the original ones up to the destination, so the remote command the
/// person typed is not run here. Returns the reason on failure.
fn copy_terminfo(ssh: &Path, args: &[String], dest_index: usize, source: &[u8]) -> Result<(), String> {
    let mut child = Command::new(ssh)
        .arg("-T")
        .args(&args[..=dest_index])
        .arg(REMOTE_COPY_COMMAND)
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| format!("ssh could not be started: {e}"))?;
    if let Some(mut stdin) = child.stdin.take() {
        let _ = stdin.write_all(source);
    }
    let out = child.wait_with_output().map_err(|e| format!("ssh failed: {e}"))?;
    if out.status.success() {
        return Ok(());
    }
    let stderr = String::from_utf8_lossy(&out.stderr);
    let last = stderr.lines().rev().find(|l| !l.trim().is_empty()).unwrap_or("").trim().to_string();
    Err(if last.is_empty() { format!("ssh exited with {}", out.status) } else { last })
}

/// Chooses the TERM for the session, copying the terminfo entry first when the host does not
/// already have it. Prints one line when the copy was not possible.
fn choose_term(ssh: &Path, args: &[String], target: &Target) -> &'static str {
    let Some(dest_index) = target.dest_index else { return TERM_FALLBACK };
    let key = connection_key(ssh, args, target);
    if key.as_deref().is_some_and(is_cached) {
        return TERM_WITH_ENTRY;
    }
    let copied = read_terminfo_source().and_then(|src| copy_terminfo(ssh, args, dest_index, &src));
    match copied {
        Ok(()) => {
            if let Some(k) = key {
                remember(&k);
            }
            TERM_WITH_ENTRY
        }
        Err(why) => {
            eprintln!("terminus ssh: the terminfo entry was not copied ({why}); using TERM={TERM_FALLBACK}");
            TERM_FALLBACK
        }
    }
}

/// Runs `terminus ssh`: `rest` is everything after the word `ssh`. Exits with ssh's exit code.
pub fn run(rest: &[String]) -> ExitCode {
    let (skip_copy, args) = match rest.first().map(String::as_str) {
        Some(NO_TERMINFO_FLAG) => (true, &rest[1..]),
        _ => (false, rest),
    };
    let ssh = find_ssh();
    let target = parse_target(args);
    let term = if skip_copy { TERM_FALLBACK } else { choose_term(&ssh, args, &target) };
    match Command::new(&ssh).args(args).env("TERM", term).status() {
        Ok(status) => ExitCode::from(status.code().map_or(255, |c| c.clamp(0, 255) as u8)),
        Err(e) => {
            eprintln!("terminus ssh: could not start {}: {e}", ssh.display());
            ExitCode::from(255)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Turns string literals into an argument vector.
    fn v(a: &[&str]) -> Vec<String> {
        a.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn plain_host_and_user_at_host() {
        let t = parse_target(&v(&["example.com"]));
        assert_eq!((t.host.as_deref(), t.user, t.dest_index), (Some("example.com"), None, Some(0)));
        let t = parse_target(&v(&["bob@example.com"]));
        assert_eq!((t.host.as_deref(), t.user.as_deref()), (Some("example.com"), Some("bob")));
    }

    #[test]
    fn options_with_values_are_skipped() {
        let t = parse_target(&v(&["-p", "2222", "-l", "amy", "-i", "key", "-o", "Opt=x", "-J", "jump", "-v", "host"]));
        assert_eq!(t.host.as_deref(), Some("host"));
        assert_eq!((t.user.as_deref(), t.port.as_deref()), (Some("amy"), Some("2222")));
        assert_eq!(t.dest_index, Some(11));
    }

    #[test]
    fn attached_and_combined_options() {
        let t = parse_target(&v(&["-p2222", "-vl", "amy", "-oStrict=no", "-tt", "host"]));
        assert_eq!(t.host.as_deref(), Some("host"));
        assert_eq!((t.user.as_deref(), t.port.as_deref()), (Some("amy"), Some("2222")));
    }

    #[test]
    fn user_in_destination_wins_over_dash_l() {
        let t = parse_target(&v(&["-l", "amy", "bob@host"]));
        assert_eq!(t.user.as_deref(), Some("bob"));
    }

    #[test]
    fn trailing_remote_command_is_not_the_destination() {
        let t = parse_target(&v(&["-p", "22", "host", "ls", "-la", "/tmp"]));
        assert_eq!((t.host.as_deref(), t.dest_index), (Some("host"), Some(2)));
    }

    #[test]
    fn double_dash_ends_options() {
        let t = parse_target(&v(&["-v", "--", "-odd-host", "id"]));
        assert_eq!((t.host.as_deref(), t.dest_index), (Some("-odd-host"), Some(2)));
    }

    #[test]
    fn ssh_uri_destinations() {
        let t = parse_target(&v(&["ssh://bob@example.com:2200/"]));
        assert_eq!(
            (t.host.as_deref(), t.user.as_deref(), t.port.as_deref()),
            (Some("example.com"), Some("bob"), Some("2200"))
        );
        let t = parse_target(&v(&["ssh://[::1]:22"]));
        assert_eq!((t.host.as_deref(), t.port.as_deref()), (Some("::1"), Some("22")));
    }

    #[test]
    fn no_destination() {
        assert_eq!(parse_target(&v(&["-V"])).host, None);
        assert_eq!(parse_target(&[]).dest_index, None);
    }

    #[test]
    fn cache_key_format() {
        assert_eq!(cache_key("bob", "h", "22"), "bob@h:22");
    }
}
