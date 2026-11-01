//! E1: spawn, exit status, cwd, environment, and kill of the child and its tree.

mod common;

use common::*;
use terminus_pty::{ConptyConfig, ExitStatus};

/// Spawns a shell command line on each platform: cmd.exe /c on Windows, sh -c elsewhere.
fn shell_command(script: &str) -> terminus_pty::SpawnOptions {
    if cfg!(windows) {
        command("cmd.exe", &["/c", script], size(100, 30))
    } else {
        command("/bin/sh", &["-c", script], size(100, 30))
    }
}

/// Runs `script`, waits for the marker, and returns the exit status and plain output.
fn run_to_exit(script: &str, config: &ConptyConfig) -> (ExitStatus, String) {
    let mut session = Session::start(&shell_command(script), config);
    assert!(session.wait_for("TERMINUS_MARK"), "marker never arrived, output: {}", show_bytes(&session.bytes()));
    let status = session.pty.wait().expect("wait");
    (status, session.plain_text())
}

/// A non zero exit code is reported as a typed code and the output marker arrives.
#[test]
fn exit_code_three_is_reported() {
    let script = if cfg!(windows) { "echo TERMINUS_MARK & exit 3" } else { "echo TERMINUS_MARK; exit 3" };
    for (name, config) in configs() {
        let (status, _) = run_to_exit(script, &config);
        assert_eq!(status, ExitStatus::Code(3), "{name}");
    }
}

/// Exit code zero is reported as `Code(0)`, and `try_wait` returns the same status afterwards.
#[test]
fn exit_code_zero_is_reported() {
    let script = if cfg!(windows) { "echo TERMINUS_MARK & exit 0" } else { "echo TERMINUS_MARK; exit 0" };
    for (name, config) in configs() {
        let mut session = Session::start(&shell_command(script), &config);
        assert!(session.wait_for("TERMINUS_MARK"), "{name}");
        assert_eq!(session.pty.wait().unwrap(), ExitStatus::Code(0), "{name}");
        assert_eq!(session.pty.try_wait().unwrap(), Some(ExitStatus::Code(0)), "{name}");
    }
}

/// On Unix a child killed by a signal is reported as `Signal`.
#[cfg(unix)]
#[test]
fn signal_exit_is_reported() {
    let mut session = Session::start(&shell_command("echo TERMINUS_MARK; kill -9 $$"), &ConptyConfig::default());
    assert!(session.wait_for("TERMINUS_MARK"));
    assert_eq!(session.pty.wait().unwrap(), ExitStatus::Signal(9));
}

/// pwsh, when installed, reports its exit code as well.
#[cfg(windows)]
#[test]
fn pwsh_exit_code_is_reported() {
    if std::process::Command::new("pwsh").arg("-NoProfile").arg("-Command").arg("exit 0").status().is_err() {
        println!("pwsh not on PATH; skipping");
        return;
    }
    for (name, config) in configs() {
        let opts = command("pwsh.exe", &["-NoProfile", "-Command", "'TERMINUS_' + 'MARK'; exit 3"], size(100, 30));
        let mut session = Session::start(&opts, &config);
        assert!(session.wait_for("TERMINUS_MARK"), "{name}: {}", show_bytes(&session.bytes()));
        assert_eq!(session.pty.wait().unwrap(), ExitStatus::Code(3), "{name}");
    }
}

/// The working directory option reaches the child.
#[test]
fn cwd_is_honoured() {
    let dir = scratch_dir("e1-cwd-dir-TERMINUS_CWD");
    for (name, config) in configs() {
        let script = if cfg!(windows) { "cd & echo TERMINUS_MARK" } else { "pwd; echo TERMINUS_MARK" };
        let mut opts = shell_command(script);
        opts.cwd = Some(dir.clone());
        let mut session = Session::start(&opts, &config);
        assert!(session.wait_for("TERMINUS_MARK"), "{name}");
        session.pty.wait().unwrap();
        assert!(session.plain_text().contains("e1-cwd-dir-TERMINUS_CWD"), "{name}: {}", session.plain_text());
    }
}

/// TERM_PROGRAM and COLORTERM from `default_shell` reach the child's environment.
#[test]
fn environment_reaches_child() {
    let script = if cfg!(windows) {
        "echo TP=%TERM_PROGRAM% CT=%COLORTERM% V=%TERM_PROGRAM_VERSION% & echo TERMINUS_MARK"
    } else {
        "echo TP=$TERM_PROGRAM CT=$COLORTERM TERM=$TERM; echo TERMINUS_MARK"
    };
    for (name, config) in configs() {
        let (_, text) = run_to_exit(script, &config);
        assert!(text.contains("TP=terminus"), "{name}: {text}");
        assert!(text.contains("CT=truecolor"), "{name}: {text}");
        if cfg!(windows) {
            assert!(text.contains(&format!("V={}", env!("CARGO_PKG_VERSION"))), "{name}: {text}");
        }
    }
}

/// `default_shell` names a real shell and sets the documented variables.
#[test]
fn default_shell_options() {
    let opts = terminus_pty::SpawnOptions::default_shell(size(80, 24));
    assert!(!opts.program.is_empty());
    let get = |key: &str| opts.env.iter().find(|(k, _)| k == key).map(|(_, v)| v.clone());
    assert_eq!(get("TERM_PROGRAM").as_deref(), Some("terminus"));
    assert_eq!(get("COLORTERM").as_deref(), Some("truecolor"));
    assert_eq!(get("TERM_PROGRAM_VERSION").as_deref(), Some(env!("CARGO_PKG_VERSION")));
    assert_eq!(get("TERM").as_deref(), if cfg!(unix) { Some("xterm-terminus") } else { None });
}

/// Kills the tree (by `kill` or by dropping the `Pty`) and checks that the grandchild process is gone.
#[cfg(windows)]
fn assert_tree_ends(name: &str, config: &ConptyConfig, by_drop: bool) {
    let script = "$p = Start-Process ping -ArgumentList '-n','200','127.0.0.1' -PassThru -NoNewWindow; Write-Host ('GCPID=' + $p.Id + '='); Start-Sleep 200";
    let mut session =
        Session::start(&command("powershell.exe", &["-NoProfile", "-Command", script], size(100, 30)), config);
    assert!(
        session.wait_until(|b| grandchild_pid(&strip_escapes(b)).is_some()),
        "{name}: no pid, output {}",
        show_bytes(&session.bytes())
    );
    let grandchild = grandchild_pid(&session.plain_text()).unwrap();
    let handle = windows_process::open(grandchild).expect("grandchild exists before the kill");
    assert!(session.pty.try_wait().unwrap().is_none(), "{name}: child must still be running");
    if by_drop {
        let Session { pty, writer, .. } = session;
        drop(writer);
        drop(pty);
    } else {
        session.pty.kill().expect("kill");
        assert!(session.pty.wait().is_ok());
    }
    assert!(
        windows_process::wait_exited(&handle),
        "{name}: grandchild {grandchild} still running after the tree was ended"
    );
}

/// Reads the PID that the child printed as `GCPID=<digits>=`.
#[cfg(windows)]
fn grandchild_pid(text: &str) -> Option<u32> {
    let rest = &text[text.find("GCPID=")? + 6..];
    let end = rest.find('=')?;
    rest[..end].trim().parse().ok()
}

/// `kill` ends the child and a grandchild it started.
#[cfg(windows)]
#[test]
fn kill_ends_the_tree() {
    for (name, config) in configs() {
        assert_tree_ends(name, &config, false);
    }
}

/// Dropping the `Pty` ends the child and a grandchild it started.
#[cfg(windows)]
#[test]
fn drop_ends_the_tree() {
    for (name, config) in configs() {
        assert_tree_ends(name, &config, true);
    }
}

/// Small wrappers over the process APIs the tree tests need.
#[cfg(windows)]
mod windows_process {
    use windows::Win32::Foundation::{HANDLE, WAIT_OBJECT_0};
    use windows::Win32::System::Threading::{OpenProcess, PROCESS_SYNCHRONIZE, WaitForSingleObject};

    /// Opens a process for waiting; `None` when it does not exist.
    pub fn open(pid: u32) -> Option<HANDLE> {
        // SAFETY: plain query of a process id.
        unsafe { OpenProcess(PROCESS_SYNCHRONIZE, false, pid).ok() }
    }

    /// Waits up to ten seconds for the process to end.
    pub fn wait_exited(handle: &HANDLE) -> bool {
        // SAFETY: the handle came from OpenProcess and is still open.
        unsafe { WaitForSingleObject(*handle, 10_000) == WAIT_OBJECT_0 }
    }
}

/// Two pwsh sessions alive at once. pwsh from the Microsoft Store runs in a job the system makes
/// for its package, and a second instance created with our job in its attribute list was refused
/// with access denied while the first was alive.
#[cfg(windows)]
#[test]
fn two_pwsh_sessions_at_once() {
    if std::process::Command::new("pwsh").arg("-NoProfile").arg("-Command").arg("exit 0").status().is_err() {
        println!("pwsh not on PATH; skipping");
        return;
    }
    for (name, config) in configs() {
        let first = command("pwsh.exe", &["-NoProfile", "-Command", "'FIRST_' + 'UP'; Start-Sleep 5"], size(100, 30));
        let mut a = Session::start(&first, &config);
        assert!(a.wait_for("FIRST_UP"), "{name}: {}", show_bytes(&a.bytes()));
        let second = command("pwsh.exe", &["-NoProfile", "-Command", "'TERMINUS_' + 'MARK'; exit 5"], size(100, 30));
        let mut b = Session::start(&second, &config);
        assert!(b.wait_for("TERMINUS_MARK"), "{name}: {}", show_bytes(&b.bytes()));
        assert_eq!(b.pty.wait().unwrap(), ExitStatus::Code(5), "{name}");
        a.pty.kill().unwrap();
    }
}
