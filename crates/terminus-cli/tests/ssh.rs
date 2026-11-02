//! `terminus ssh` against a stand in `ssh` program placed first on PATH. The stand in appends its
//! arguments and TERM to a log file, and the terminfo source it receives on stdin for the copy
//! command. No network is used.

use std::path::{Path, PathBuf};
use std::process::{Command, Output};

/// The `terminus` program under test.
fn cli_exe() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_terminus"))
}

/// A private folder holding the stand in, its log and the data folder.
struct Sandbox {
    dir: PathBuf,
}

impl Drop for Sandbox {
    /// Removes the folder.
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}

impl Sandbox {
    /// Creates the folder and writes the stand in ssh for this platform. The stand in logs every
    /// call, prints nothing for `-G`, copies stdin to the log for the copy command (whose first
    /// argument is `-T`), and fails that command when `SSH_FAIL_COPY` is set.
    fn new(tag: &str) -> Sandbox {
        let dir = std::env::temp_dir().join(format!("terminus-ssh-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("data")).unwrap();
        let unix = "#!/bin/sh\n\
            echo \"ARGS: $*\" >> \"$SSH_LOG\"\n\
            echo \"TERM=$TERM\" >> \"$SSH_LOG\"\n\
            if [ \"$1\" = \"-G\" ]; then\n\
              if [ -n \"$SSH_G_HOST\" ]; then printf 'user amy\\nhostname %s\\nport 2200\\n' \"$SSH_G_HOST\"; fi\n\
              exit 0\n\
            fi\n\
            if [ \"$1\" = \"-T\" ]; then\n\
              echo STDIN-BEGIN >> \"$SSH_LOG\"; cat >> \"$SSH_LOG\"; echo STDIN-END >> \"$SSH_LOG\"\n\
              if [ -n \"$SSH_FAIL_COPY\" ]; then echo 'tic: command not found' >&2; exit 127; fi\n\
              exit 0\n\
            fi\n\
            exit ${SSH_EXIT:-0}\n";
        let cmd = "@echo off\r\n\
            echo ARGS: %* >>\"%SSH_LOG%\"\r\n\
            echo TERM=%TERM% >>\"%SSH_LOG%\"\r\n\
            if \"%1\"==\"-G\" goto g\r\n\
            if \"%1\"==\"-T\" goto copy\r\n\
            if not defined SSH_EXIT set SSH_EXIT=0\r\n\
            exit /b %SSH_EXIT%\r\n\
            :g\r\n\
            if defined SSH_G_HOST (echo user amy& echo hostname %SSH_G_HOST%& echo port 2200)\r\n\
            exit /b 0\r\n\
            :copy\r\n\
            echo STDIN-BEGIN >>\"%SSH_LOG%\"\r\n\
            findstr \"^\" >>\"%SSH_LOG%\"\r\n\
            echo STDIN-END >>\"%SSH_LOG%\"\r\n\
            if defined SSH_FAIL_COPY (echo tic: command not found 1>&2& exit /b 127)\r\n\
            exit /b 0\r\n";
        std::fs::write(dir.join("ssh"), unix).unwrap();
        std::fs::write(dir.join("ssh.cmd"), cmd).unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(dir.join("ssh"), std::fs::Permissions::from_mode(0o755)).unwrap();
        }
        Sandbox { dir }
    }

    /// The log file the stand in writes.
    fn log_path(&self) -> PathBuf {
        self.dir.join("ssh.log")
    }

    /// The log text, or an empty string before the first call.
    fn log(&self) -> String {
        std::fs::read_to_string(self.log_path()).unwrap_or_default()
    }

    /// The cache file text under the private data folder.
    fn cache(&self) -> String {
        let data = self.dir.join("data").join("terminus");
        std::fs::read_to_string(data.join("ssh-terminfo-hosts.txt")).unwrap_or_default()
    }

    /// Runs `terminus ssh <args>` with the stand in first on PATH and the data folder private.
    fn run(&self, args: &[&str], extra_env: &[(&str, &str)]) -> Output {
        let sep = if cfg!(windows) { ";" } else { ":" };
        let path = format!("{}{sep}{}", self.dir.display(), std::env::var("PATH").unwrap_or_default());
        let data = self.dir.join("data");
        let mut cmd = Command::new(cli_exe());
        cmd.arg("ssh").args(args).env("PATH", path).env("SSH_LOG", self.log_path());
        // TERMINUS_DATA_DIR names the data folder on every platform; macOS ignores the other two.
        cmd.env("TERMINUS_DATA_DIR", data.join("terminus")).env("HOME", &data);
        for (k, v) in extra_env {
            cmd.env(k, v);
        }
        cmd.output().expect("run terminus ssh")
    }
}

/// The ssh invocations in the log, each as its `ARGS:` line and the TERM that followed it.
fn calls(log: &str) -> Vec<(String, String)> {
    let lines: Vec<&str> = log.lines().collect();
    lines
        .iter()
        .enumerate()
        .filter_map(|(i, l)| l.strip_prefix("ARGS: ").map(|a| (a.trim().to_string(), lines[i + 1].trim().to_string())))
        .collect()
}

/// The calls that are the terminfo copy (their arguments start with `-T`).
fn copy_calls(log: &str) -> Vec<(String, String)> {
    calls(log).into_iter().filter(|(a, _)| a.starts_with("-T ")).collect()
}

/// The last call, which is the session itself.
fn session_call(log: &str) -> (String, String) {
    calls(log).pop().expect("the stand in was called")
}

/// Checks the path exists, to make assertion messages name the sandbox.
fn assert_exists(p: &Path) {
    assert!(p.exists(), "{} is missing", p.display());
}

#[test]
fn copies_the_terminfo_source_then_connects_with_the_entry_and_caches_the_host() {
    let sb = Sandbox::new("copy");
    let out = sb.run(&["-p", "2222", "bob@box", "ls", "-la"], &[]);
    assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stderr));
    let log = sb.log();
    let copies = copy_calls(&log);
    assert_eq!(copies.len(), 1, "{log}");
    let copy = &copies[0].0;
    assert!(copy.starts_with("-T -p 2222 bob@box "), "{copy}");
    assert!(copy.contains("mkdir -p ~/.terminfo && tic -x -o ~/.terminfo /dev/stdin"), "{copy}");
    assert!(!copy.contains("ls -la"), "the typed remote command must not run in the copy: {copy}");
    let stdin = log.split("STDIN-BEGIN").nth(1).and_then(|r| r.split("STDIN-END").next()).unwrap_or("");
    assert!(stdin.contains("xterm-terminus") && stdin.contains("colors"), "stdin was: {stdin}");
    let (args, term) = session_call(&log);
    assert_eq!(args, "-p 2222 bob@box ls -la");
    assert_eq!(term, "TERM=xterm-terminus");
    assert!(sb.cache().lines().any(|l| l == "bob@box:2222"), "cache: {}", sb.cache());

    // The second connection to the same host skips the copy and still gets the entry.
    let out = sb.run(&["-p", "2222", "bob@box"], &[]);
    assert!(out.status.success());
    let log = sb.log();
    assert_eq!(copy_calls(&log).len(), 1, "no second copy: {log}");
    assert_eq!(session_call(&log).1, "TERM=xterm-terminus");
    assert_exists(&sb.dir);
}

#[test]
fn a_different_port_is_a_different_host_in_the_cache() {
    let sb = Sandbox::new("port");
    sb.run(&["-p", "22", "bob@box"], &[]);
    sb.run(&["-p", "2222", "bob@box"], &[]);
    assert_eq!(copy_calls(&sb.log()).len(), 2);
    assert!(sb.cache().contains("bob@box:22") && sb.cache().contains("bob@box:2222"));
}

#[test]
fn cache_key_comes_from_ssh_dash_g_when_it_answers() {
    let sb = Sandbox::new("dashg");
    sb.run(&["alias"], &[("SSH_G_HOST", "resolved.example")]);
    assert!(sb.cache().lines().any(|l| l == "amy@resolved.example:2200"), "cache: {}", sb.cache());
}

#[test]
fn a_failed_copy_falls_back_to_xterm_256color_and_is_not_cached() {
    let sb = Sandbox::new("fail");
    let out = sb.run(&["bob@box"], &[("SSH_FAIL_COPY", "1"), ("SSH_EXIT", "7")]);
    assert_eq!(out.status.code(), Some(7), "the session's exit code is returned");
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert_eq!(stderr.lines().filter(|l| l.contains("terminfo entry was not copied")).count(), 1, "{stderr}");
    assert!(stderr.contains("tic: command not found") && stderr.contains("xterm-256color"), "{stderr}");
    assert_eq!(session_call(&sb.log()), ("bob@box".to_string(), "TERM=xterm-256color".to_string()));
    assert_eq!(sb.cache(), "");
}

#[test]
fn no_terminfo_skips_the_copy() {
    let sb = Sandbox::new("skip");
    let out = sb.run(&["--no-terminfo", "-i", "key", "box", "uptime"], &[]);
    assert!(out.status.success());
    let log = sb.log();
    assert!(copy_calls(&log).is_empty(), "{log}");
    assert_eq!(session_call(&log), ("-i key box uptime".to_string(), "TERM=xterm-256color".to_string()));
    assert_eq!(sb.cache(), "");
}
