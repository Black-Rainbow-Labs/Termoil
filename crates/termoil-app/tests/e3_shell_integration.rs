//! E3: shell integration. Each shell installed on this machine is started on a real pseudoterminal
//! with the integration injected, its output goes into a `termoil_vt::Terminal`, and the test
//! waits on the terminal's events: OSC 133 A and B at the prompt, OSC 7 after `cd`, and OSC 133 C
//! and D with the exit code around a failing and a succeeding command. Shells that are not
//! installed print a skip. The test also checks the terminfo source against `TERMINFO_CAPS`.

use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::sync::mpsc::{Receiver, channel};
use std::time::{Duration, Instant};

use termoil_app::shell_integration::{Shell, detect, injection};
use termoil_pty::{ConptyChoice, ConptyConfig, Pty, PtySize, SpawnOptions};
use termoil_vt::terminal::PromptEvent;
use termoil_vt::{Event, Terminal, TerminalConfig};

/// How long any single wait may take. The tests wait on events and never on fixed sleeps.
const DEADLINE: Duration = Duration::from_secs(30);

/// The repository's `shell-integration` folder, or the one named by `TERMOIL_SHELL_INTEGRATION_SRC`.
fn scripts_dir() -> PathBuf {
    if let Some(dir) = std::env::var_os("TERMOIL_SHELL_INTEGRATION_SRC") {
        return PathBuf::from(dir);
    }
    Path::new(env!("CARGO_MANIFEST_DIR")).join("..").join("..").join("shell-integration")
}

/// A running shell whose output feeds a terminal emulator.
struct Session {
    _pty: Pty,
    writer: Box<dyn Write + Send>,
    chunks: Receiver<Vec<u8>>,
    term: Terminal,
    events: Vec<Event>,
    cursor: usize,
    raw: Vec<u8>,
}

impl Session {
    /// Starts `program` with the arguments and environment `injection` gives for the folder.
    fn start(
        program: &str, shell: Shell, dir: &Path, user_args: &[&str], cwd: &Path, wsl: bool, extra_env: &[(&str, &str)],
    ) -> Session {
        let user: Vec<String> = user_args.iter().map(|s| s.to_string()).collect();
        let mut inj = injection(shell, dir, &user).expect("injection");
        inj.env.extend(extra_env.iter().map(|(k, v)| (k.to_string(), v.to_string())));
        if wsl {
            // wsl.exe runs bash and passes only the variables named in WSLENV through.
            let names: Vec<String> = inj.env.iter().map(|(k, _)| k.clone()).collect();
            inj.env.push(("WSLENV".to_string(), names.join(":")));
            let mut args: Vec<String> = ["-d", "Ubuntu", "-e", "bash"].iter().map(|s| s.to_string()).collect();
            args.append(&mut inj.args);
            inj.args = args;
        }
        let opts = SpawnOptions {
            program: program.to_string(),
            args: inj.args,
            cwd: Some(cwd.to_path_buf()),
            env: inj.env,
            size: PtySize { cols: 120, rows: 30, pixel_width: 0, pixel_height: 0 },
            login: false,
        };
        let mut pty = spawn_serialised(&opts);
        let mut reader = pty.take_reader().expect("reader");
        let writer = pty.take_writer().expect("writer");
        let (tx, chunks) = channel();
        std::thread::spawn(move || {
            let mut buf = vec![0u8; 16384];
            while let Ok(n) = reader.read(&mut buf) {
                if n == 0 || tx.send(buf[..n].to_vec()).is_err() {
                    break;
                }
            }
        });
        let term = Terminal::new(120, 30, TerminalConfig::default());
        Session { _pty: pty, writer, chunks, term, events: Vec::new(), cursor: 0, raw: Vec::new() }
    }

    /// Takes one chunk of output into the terminal, collects its events and writes its replies back.
    fn pump(&mut self, timeout: Duration) {
        let Ok(chunk) = self.chunks.recv_timeout(timeout) else { return };
        self.raw.extend_from_slice(&chunk);
        self.term.advance(&chunk);
        self.events.extend(self.term.take_events());
        let replies = self.term.take_replies();
        if !replies.is_empty() {
            let _ = self.writer.write_all(&replies);
            let _ = self.writer.flush();
        }
    }

    /// Waits for the next event after the last one found that satisfies `matches`, and returns it.
    fn wait_event(&mut self, what: &str, matches: impl Fn(&Event) -> bool) -> Event {
        let start = Instant::now();
        loop {
            while self.cursor < self.events.len() {
                let event = self.events[self.cursor].clone();
                self.cursor += 1;
                if matches(&event) {
                    return event;
                }
            }
            if start.elapsed() > DEADLINE {
                self.fail(what);
            }
            self.pump(Duration::from_millis(200));
        }
    }

    /// Panics with the events and the tail of the raw output, to show what the shell did.
    fn fail(&self, what: &str) -> ! {
        let tail = &self.raw[self.raw.len().saturating_sub(1500)..];
        let skip = self.events.len().saturating_sub(40);
        let events: Vec<&Event> =
            self.events.iter().skip(skip).filter(|e| !matches!(e, Event::ModeChanged(_))).collect();
        panic!("timed out waiting for {what}\nevents: {events:?}\nraw tail: {:?}", String::from_utf8_lossy(tail));
    }

    /// Waits for a prompt: OSC 133;A and then OSC 133;B.
    fn wait_prompt(&mut self) {
        self.wait_event("Prompt(PromptStart)", |e| matches!(e, Event::Prompt(PromptEvent::PromptStart)));
        self.wait_event("Prompt(InputStart)", |e| matches!(e, Event::Prompt(PromptEvent::InputStart)));
    }

    /// Types a command line and presses Enter.
    fn type_line(&mut self, text: &str) {
        self.writer.write_all(format!("{text}\r").as_bytes()).expect("write");
        self.writer.flush().expect("flush");
    }

    /// Waits for OSC 133;C and then OSC 133;D, and returns the exit code D carried.
    fn wait_command(&mut self) -> Option<i32> {
        self.wait_event("Prompt(OutputStart)", |e| matches!(e, Event::Prompt(PromptEvent::OutputStart)));
        match self
            .wait_event("Prompt(CommandFinished)", |e| matches!(e, Event::Prompt(PromptEvent::CommandFinished(_))))
        {
            Event::Prompt(PromptEvent::CommandFinished(code)) => code,
            other => panic!("unexpected {other:?}"),
        }
    }
}

/// Uses the bundled ConPTY pair when it is present, so the OSC 133 passthrough that was measured is the one tested.
fn conpty_config() -> ConptyConfig {
    let bundled = scripts_dir().join("..").join("installer").join("windows").join("conpty");
    if bundled.join("conpty.dll").is_file() {
        ConptyConfig { choice: ConptyChoice::Bundled(bundled), flags: None, ..ConptyConfig::default() }
    } else {
        ConptyConfig::default()
    }
}

/// How to drive one shell: the commands to type and what the shell is able to report.
struct Spec {
    name: &'static str,
    program: String,
    shell: Shell,
    args: Vec<&'static str>,
    /// Builds the command that changes into a folder.
    cd: fn(&Path) -> String,
    /// A command that exits with 7, when the shell reports exit codes.
    fail: &'static str,
    ok: &'static str,
    /// The integration sends C and D (cmd cannot).
    commands_marked: bool,
    /// Start bash inside WSL (Ubuntu) instead of on Windows.
    wsl: bool,
    /// More environment variables for the shell.
    env: Vec<(&'static str, &'static str)>,
}

/// A new empty folder under the temp directory, with a name no other folder has.
fn unique_dir(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("termoil-e3-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("temp dir");
    dir
}

/// Runs the four E3 checks against one shell.
fn run_spec(spec: Spec) {
    let target = unique_dir(spec.name);
    let start = unique_dir(&format!("{}-start", spec.name));
    let dir = scripts_dir().canonicalize().expect("shell-integration folder");
    let dir = PathBuf::from(dir.to_string_lossy().trim_start_matches(r"\\?\"));
    let dir = if spec.wsl { wsl_path(&dir) } else { dir };
    let mut s = Session::start(&spec.program, spec.shell, &dir, &spec.args, &start, spec.wsl, &spec.env);
    s.wait_prompt();
    let early_finish = s.events.iter().any(|e| matches!(e, Event::Prompt(PromptEvent::CommandFinished(_))));
    assert!(!early_finish, "{}: OSC 133;D was sent before the first command", spec.name);
    let cd_target = if spec.wsl { wsl_path(&target) } else { target.clone() };
    s.type_line(&(spec.cd)(&cd_target));
    let leaf = target.file_name().unwrap().to_string_lossy().into_owned();
    let url = s.wait_event("WorkingDirectory", |e| matches!(e, Event::WorkingDirectory(u) if u.contains(&leaf)));
    let Event::WorkingDirectory(url) = url else { unreachable!() };
    assert!(url.starts_with("file://"), "{}: {url}", spec.name);
    println!("{}: WorkingDirectory {url}", spec.name);
    if spec.commands_marked {
        s.wait_prompt();
        s.type_line(spec.fail);
        assert_eq!(s.wait_command(), Some(7), "{}: failing command", spec.name);
        s.wait_prompt();
        s.type_line(spec.ok);
        assert_eq!(s.wait_command(), Some(0), "{}: succeeding command", spec.name);
    }
    drop(s);
    let _ = std::fs::remove_dir_all(&target);
    let _ = std::fs::remove_dir_all(&start);
}

/// True when running `program` with `args` succeeds, which is how installed shells are found.
fn available(program: &str, args: &[&str]) -> bool {
    let _guard = SPAWN_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    std::process::Command::new(program).args(args).output().is_ok_and(|o| o.status.success())
}

/// A PowerShell style spec for `program`.
fn powershell_spec(name: &'static str, program: &str) -> Spec {
    Spec {
        name,
        program: program.to_string(),
        shell: detect(program),
        args: vec!["-NoLogo"],
        cd: |p| format!("cd '{}'", p.display()),
        fail: if cfg!(windows) { "cmd /c exit 7" } else { "sh -c 'exit 7'" },
        ok: "Get-Date | Out-Null",
        commands_marked: true,
        wsl: false,
        env: Vec::new(),
    }
}

/// PowerShell 7.
#[test]
fn pwsh() {
    let Some(program) = real_pwsh_path() else { return println!("pwsh is not installed; skipping") };
    run_spec(powershell_spec("pwsh", &program));
}

/// Windows PowerShell 5.1.
#[test]
fn windows_powershell() {
    if !cfg!(windows) {
        return println!("Windows PowerShell exists only on Windows; skipping");
    }
    run_spec(powershell_spec("powershell", "powershell.exe"));
}

/// Git Bash on Windows, or bash from the PATH elsewhere.
#[test]
fn bash() {
    let program = if cfg!(windows) { "C:/Program Files/Git/bin/bash.exe".to_string() } else { "bash".to_string() };
    if !available(&program, &["-c", "exit 0"]) {
        return println!("bash is not installed; skipping");
    }
    run_spec(Spec {
        name: "bash",
        program,
        shell: Shell::Bash,
        args: vec!["-i"],
        cd: |p| format!("cd '{}'", p.to_string_lossy().replace('\\', "/")),
        fail: "(exit 7)",
        ok: "true",
        commands_marked: true,
        wsl: false,
        env: Vec::new(),
    });
}

/// cmd.exe: only the prompt marks and the working directory.
#[test]
fn cmd() {
    if !cfg!(windows) {
        return println!("cmd exists only on Windows; skipping");
    }
    run_spec(Spec {
        name: "cmd",
        program: "cmd.exe".to_string(),
        shell: Shell::Cmd,
        args: vec![],
        cd: |p| format!("cd /d {}", p.display()),
        fail: "",
        ok: "",
        commands_marked: false,
        wsl: false,
        env: Vec::new(),
    });
}

/// zsh, when installed.
#[test]
fn zsh() {
    if !available("zsh", &["-c", "exit 0"]) {
        return println!("zsh is not installed; skipping");
    }
    run_spec(Spec {
        name: "zsh",
        program: "zsh".to_string(),
        shell: Shell::Zsh,
        args: vec!["-i"],
        cd: |p| format!("cd '{}'", p.display()),
        fail: "(exit 7)",
        ok: "true",
        commands_marked: true,
        wsl: false,
        env: Vec::new(),
    });
}

/// fish, when installed.
#[test]
fn fish() {
    if !available("fish", &["-c", "exit 0"]) {
        return println!("fish is not installed; skipping");
    }
    run_spec(Spec {
        name: "fish",
        program: "fish".to_string(),
        shell: Shell::Fish,
        args: vec![],
        cd: |p| format!("cd '{}'", p.display()),
        fail: "sh -c 'exit 7'",
        ok: "true",
        commands_marked: true,
        wsl: false,
        env: Vec::new(),
    });
}

/// nushell, when installed.
#[test]
fn nushell() {
    if !available("nu", &["-c", "exit 0"]) {
        return println!("nushell is not installed; skipping");
    }
    run_spec(Spec {
        name: "nushell",
        program: "nu".to_string(),
        shell: Shell::Nushell,
        args: vec![],
        cd: |p| format!("cd '{}'", p.to_string_lossy().replace('\\', "/")),
        fail: "^nu -c 'exit 7'",
        ok: "echo ok",
        commands_marked: true,
        wsl: false,
        env: Vec::new(),
    });
}

/// Turns Rust string escapes (`\x1b`, `\x07`, `\x7f`, `\\`, `\"`) into the characters they name.
fn unescape_rust(text: &str) -> String {
    let mut out = String::new();
    let mut chars = text.chars();
    while let Some(c) = chars.next() {
        if c != '\\' {
            out.push(c);
            continue;
        }
        match chars.next() {
            Some('x') => {
                let hex: String = chars.by_ref().take(2).collect();
                out.push(u8::from_str_radix(&hex, 16).unwrap() as char);
            }
            Some(other) => out.push(other),
            None => {}
        }
    }
    out
}

/// Reads the `(name, value)` rows of `TERMINFO_CAPS` out of the terminal's source file.
fn terminfo_caps_from_source() -> Vec<(String, String)> {
    let path = scripts_dir().join("..").join("crates/termoil-vt/src/terminal/report.rs");
    let text = std::fs::read_to_string(path).expect("report.rs");
    let body = text.split("TERMINFO_CAPS: &[(&str, &str)] = &[").nth(1).expect("table").split("];").next().unwrap();
    let mut rows = Vec::new();
    for line in body.lines().map(str::trim).filter(|l| l.starts_with("(\"")) {
        let inner = line.strip_prefix("(\"").and_then(|l| l.strip_suffix("\"),")).expect("row shape");
        let (name, value) = inner.split_once("\", \"").expect("row");
        rows.push((name.to_string(), unescape_rust(value)));
    }
    rows
}

/// Turns terminfo escapes (`\E`, `^G`, `^?`, `\\`, `\,`, `\^`, `\:`) into the characters they name.
fn unescape_terminfo(text: &str) -> String {
    let mut out = String::new();
    let mut chars = text.chars();
    while let Some(c) = chars.next() {
        match c {
            '\\' => match chars.next() {
                Some('E') | Some('e') => out.push('\x1b'),
                Some(other) => out.push(other),
                None => {}
            },
            '^' => match chars.next() {
                Some('?') => out.push('\x7f'),
                Some(letter) => out.push(((letter as u8) & 0x1f) as char),
                None => {}
            },
            other => out.push(other),
        }
    }
    out
}

/// Splits a terminfo entry into its capabilities on commas that are not escaped.
fn split_caps(body: &str) -> Vec<String> {
    let mut caps = Vec::new();
    let mut current = String::new();
    let mut escaped = false;
    for c in body.chars() {
        if c == ',' && !escaped {
            caps.push(current.trim().to_string());
            current.clear();
        } else {
            current.push(c);
        }
        escaped = c == '\\' && !escaped;
    }
    caps.into_iter().filter(|c| !c.is_empty()).collect()
}

/// Every `TERMINFO_CAPS` entry appears in `xterm-termoil.terminfo` with the same value.
#[test]
fn terminfo_matches_caps_table() {
    let text =
        std::fs::read_to_string(scripts_dir().join("terminfo").join("xterm-termoil.terminfo")).expect("terminfo");
    let lines: Vec<&str> = text.lines().filter(|l| !l.trim_start().starts_with('#') && !l.trim().is_empty()).collect();
    let header = lines[0].trim_end_matches(',');
    let name = header.split('|').next().unwrap().to_string();
    let caps = split_caps(&lines[1..].join(" "));
    assert!(caps.iter().any(|c| c == "use=xterm-256color"));
    for (key, value) in terminfo_caps_from_source() {
        if key == "TN" || key == "name" {
            assert_eq!(value, name, "{key}");
            continue;
        }
        let key = if key == "Co" { "colors".to_string() } else { key };
        let numeric = ["colors", "cols", "lines"].contains(&key.as_str());
        let found = caps.iter().find_map(|c| {
            let split = c.find(['=', '#']).unwrap_or(c.len());
            (c[..split] == key).then(|| (c[split..].to_string(), c.len() == split))
        });
        let (rest, boolean) = found.unwrap_or_else(|| panic!("{key} is missing from the terminfo file"));
        if value.is_empty() {
            assert!(boolean, "{key} should be a boolean");
        } else if numeric {
            assert_eq!(rest, format!("#{value}"), "{key}");
        } else {
            assert_eq!(unescape_terminfo(&rest[1..]), value, "{key}");
        }
    }
}

/// Without `TERMOIL_SHELL_INTEGRATION=1` the scripts change nothing, so sourcing one from an rc
/// file in another terminal has no effect there.
#[test]
fn scripts_do_nothing_without_the_switch() {
    let dir = scripts_dir();
    let bash = "C:/Program Files/Git/bin/bash.exe";
    if available(bash, &["-c", "exit 0"]) {
        let script = format!(
            "PROMPT_COMMAND=; PS1='$ '; . '{}/bash/termoil.bash'; echo \"[$PROMPT_COMMAND][$PS1][${{TERM_PROGRAM:-}}]\"",
            dir.display()
        );
        let out = run_locked(
            std::process::Command::new(bash)
                .args(["-i", "-c", &script])
                .env_remove("TERMOIL_SHELL_INTEGRATION")
                .env_remove("TERM_PROGRAM"),
        );
        assert!(String::from_utf8_lossy(&out.stdout).contains("[][$ ][]"), "{}", String::from_utf8_lossy(&out.stdout));
    }
    if let Some(pwsh) = real_pwsh_path() {
        let script = format!(
            ". '{}/powershell/termoil.ps1'; 'flag=' + [bool]$global:__TermoilIntegrated + ' tp=' + $env:TERM_PROGRAM",
            dir.display()
        );
        let out = run_locked(
            std::process::Command::new(&pwsh)
                .args(["-NoProfile", "-Command", &script])
                .env_remove("TERMOIL_SHELL_INTEGRATION")
                .env_remove("TERM_PROGRAM"),
        );
        assert!(
            String::from_utf8_lossy(&out.stdout).contains("flag=False tp="),
            "{}",
            String::from_utf8_lossy(&out.stdout)
        );
    }
}

/// The real path of pwsh. On Windows `pwsh` on the PATH can be a Store alias, which the process
/// creation call used for a pseudoterminal sometimes refuses, so ask pwsh where it really is.
fn real_pwsh_path() -> Option<String> {
    static PATH: std::sync::OnceLock<Option<String>> = std::sync::OnceLock::new();
    PATH.get_or_init(|| {
        for _ in 0..5 {
            let _guard = SPAWN_LOCK.lock().unwrap_or_else(|e| e.into_inner());
            let args = ["-NoProfile", "-Command", "[Environment]::ProcessPath"];
            let out = std::process::Command::new("pwsh").args(args).output();
            let path = out.map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string()).unwrap_or_default();
            if !path.is_empty() {
                return Some(path);
            }
        }
        None
    })
    .clone()
}

/// Held while a pseudoterminal is created. Creating several at the same moment from tests that
/// run in parallel makes one of the creations fail on this machine, so they are made one at a time.
static SPAWN_LOCK: Mutex<()> = Mutex::new(());

/// Creates the pseudoterminal and starts the shell, one test at a time.
fn spawn_serialised(opts: &SpawnOptions) -> Pty {
    let _guard = SPAWN_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    Pty::spawn_with_config(opts, &conpty_config()).expect("spawn shell")
}

/// Runs a command to completion while holding the spawn lock, so it does not start at the same
/// moment as a pseudoterminal child in another test.
fn run_locked(command: &mut std::process::Command) -> std::process::Output {
    let _guard = SPAWN_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    command.output().unwrap()
}

/// Turns `C:\a\b` into `/mnt/c/a/b`, the path WSL sees for a Windows folder.
fn wsl_path(path: &Path) -> PathBuf {
    let text = path.to_string_lossy().replace('\\', "/");
    match text.split_once(':') {
        Some((drive, rest)) => PathBuf::from(format!("/mnt/{}{rest}", drive.to_ascii_lowercase())),
        None => PathBuf::from(text),
    }
}

/// bash with the path used before bash 4.4 (no PS0: the DEBUG trap and the history number), as
/// macOS's bash 3.2 runs it. A subshell such as `(exit 7)` must still report its exit code.
#[test]
fn bash_without_ps0() {
    let program = if cfg!(windows) { "C:/Program Files/Git/bin/bash.exe".to_string() } else { "bash".to_string() };
    if !available(&program, &["-c", "exit 0"]) {
        return println!("bash is not installed; skipping");
    }
    run_spec(Spec {
        name: "bash-no-ps0",
        program,
        shell: Shell::Bash,
        args: vec!["-i"],
        cd: |p| format!("cd '{}'", p.to_string_lossy().replace('\\', "/")),
        fail: "(exit 7)",
        ok: "true",
        commands_marked: true,
        wsl: false,
        env: vec![("TERMOIL_BASH_NO_PS0", "1")],
    });
}

/// bash inside WSL (Ubuntu), a bonus run that exercises the Linux path of the bash script.
#[test]
fn bash_in_wsl() {
    let probe = ["-d", "Ubuntu", "-e", "bash", "-c", "exit 0"];
    if !cfg!(windows) || !available("wsl.exe", &probe) {
        return println!("WSL Ubuntu is not available; skipping");
    }
    run_spec(Spec {
        name: "wsl-bash",
        program: "wsl.exe".to_string(),
        shell: Shell::Bash,
        args: vec!["-i"],
        cd: |p| format!("cd '{}'", p.display()),
        fail: "(exit 7)",
        ok: "true",
        commands_marked: true,
        wsl: true,
        env: Vec::new(),
    });
}

/// A folder holding a stand in `ssh` that prints the TERM it was given, and the PATH with that
/// folder first.
fn fake_ssh() -> (PathBuf, String) {
    let dir = unique_dir("fake-ssh");
    std::fs::write(dir.join("ssh"), "#!/bin/sh\necho \"TERM=$TERM\"\n").unwrap();
    std::fs::write(dir.join("ssh.cmd"), "@echo TERM=%TERM%\r\n").unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(dir.join("ssh"), std::fs::Permissions::from_mode(0o755)).unwrap();
    }
    let sep = if cfg!(windows) { ";" } else { ":" };
    let path = format!("{}{sep}{}", dir.display(), std::env::var("PATH").unwrap_or_default());
    (dir, path)
}

/// Runs `program` with `args` as an integrated shell with TERM=xterm-termoil and the stand in
/// ssh first on PATH, and returns what it printed. The callers start each shell without the
/// person's startup files, which can put another ssh first on PATH.
fn run_with_fake_ssh(program: &str, args: &[&str], path: &str) -> String {
    let out = run_locked(
        std::process::Command::new(program)
            .args(args)
            .env("TERMOIL_SHELL_INTEGRATION", "1")
            .env("TERM", "xterm-termoil")
            .env("PATH", path),
    );
    String::from_utf8_lossy(&out.stdout).into_owned()
}

/// ssh started from an integrated shell gets TERM=xterm-256color, because the remote host has no
/// xterm-termoil entry (PRD 5.3, the right terminfo on SSH).
#[test]
fn ssh_gets_a_term_the_remote_host_knows() {
    let dir = scripts_dir();
    let (fake, path) = fake_ssh();
    let bash = if cfg!(windows) { "C:/Program Files/Git/bin/bash.exe" } else { "bash" };
    if available(bash, &["-c", "exit 0"]) {
        // Git Bash's launcher puts its own folders first on PATH, so the stand in goes first
        // inside the script, written the way Git Bash writes a Windows path (/c/...).
        let unix = |p: &Path| {
            let s = p.display().to_string().replace('\\', "/");
            match s.split_once(":/") {
                Some((drive, rest)) if cfg!(windows) => format!("/{}/{rest}", drive.to_lowercase()),
                _ => s,
            }
        };
        let script = format!(". '{}/bash/termoil.bash'; PATH='{}':\"$PATH\"; ssh", unix(&dir), unix(&fake));
        let out = run_with_fake_ssh(bash, &["--norc", "-i", "-c", &script], &path);
        assert!(out.contains("TERM=xterm-256color"), "bash: {out}");
    }
    if !cfg!(windows) && available("zsh", &["-c", "exit 0"]) {
        let script = format!(". '{}/zsh/termoil.zsh'; ssh", dir.display());
        let out = run_with_fake_ssh("zsh", &["-f", "-i", "-c", &script], &path);
        assert!(out.contains("TERM=xterm-256color"), "zsh: {out}");
    }
    if !cfg!(windows) && available("fish", &["-c", "exit 0"]) {
        let script = format!("source '{}/fish/vendor_conf.d/termoil.fish'; ssh", dir.display());
        let out = run_with_fake_ssh("fish", &["--no-config", "-i", "-c", &script], &path);
        assert!(out.contains("TERM=xterm-256color"), "fish: {out}");
    }
    if available("nu", &["-c", "exit 0"]) {
        let script = format!("source '{}/nushell/termoil.nu'; ssh", dir.display().to_string().replace('\\', "/"));
        let out = run_with_fake_ssh("nu", &["-c", &script], &path);
        assert!(out.contains("TERM=xterm-256color"), "nushell: {out}");
    }
    let _ = std::fs::remove_dir_all(&fake);
}
