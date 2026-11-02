//! E5 and A1 (docs/TEST-SUITE.md sections 11 and 14).
//!
//! E5 starts `terminus-gui --headless` on a private socket with a private config file and drives
//! it with the real `terminus` program: every command line action directly, and every catalogue
//! action through `run-action`. A1 then checks, naming what is missing, that every catalogue row
//! reached `run_action` (it did not answer "unknown action"), has a section in docs/commands.md, and
//! was exercised here. The socket must be readable and writable by the current user only.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Output};
use std::time::{Duration, Instant};

use serde_json::{Value, json};
use terminus_protocol::CATALOGUE;

/// The `terminus` program under test.
fn cli_exe() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_terminus"))
}

/// `terminus-gui` next to it, built first when missing.
fn gui_exe() -> PathBuf {
    let exe = cli_exe().with_file_name(if cfg!(windows) { "terminus-gui.exe" } else { "terminus-gui" });
    if !exe.is_file() {
        let manifest = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../Cargo.toml");
        let mut cmd = Command::new(env!("CARGO"));
        cmd.args(["build", "-p", "terminus-app", "--bin", "terminus-gui", "--manifest-path"]).arg(manifest);
        if !cfg!(debug_assertions) {
            cmd.arg("--release");
        }
        assert!(cmd.status().expect("cargo build").success(), "building terminus-gui failed");
    }
    exe
}

/// A running headless instance and the environment that reaches it.
struct Instance {
    child: Child,
    socket: String,
    config: PathBuf,
    dir: PathBuf,
}

impl Drop for Instance {
    /// Stops the instance if the test did not.
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

impl Instance {
    /// Starts the instance with a config that runs a fast shell with shell integration.
    fn start(tag: &str) -> Instance {
        let dir = std::env::temp_dir().join(format!("terminus-e5-{tag}-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let config = dir.join("config.toml");
        std::fs::write(&config, config_text(1_000_000)).unwrap();
        let socket = if cfg!(windows) {
            format!(r"\\.\pipe\terminus-e5-{tag}-{}", std::process::id())
        } else {
            dir.join("t.sock").to_string_lossy().into_owned()
        };
        let child = Command::new(gui_exe())
            .arg("--headless")
            .env("TERMINUS_SOCKET", &socket)
            .env("TERMINUS_CONFIG", &config)
            .spawn()
            .expect("start terminus-gui");
        let inst = Instance { child, socket, config, dir };
        let deadline = Instant::now() + Duration::from_secs(20);
        while inst.cli(&["+list"]).status.code() != Some(0) {
            assert!(Instant::now() < deadline, "the headless instance never answered");
            std::thread::sleep(Duration::from_millis(50));
        }
        inst
    }

    /// Runs `terminus` with arguments against this instance.
    fn cli(&self, args: &[&str]) -> Output {
        Command::new(cli_exe())
            .args(args)
            .env("TERMINUS_SOCKET", &self.socket)
            .env("TERMINUS_CONFIG", &self.config)
            .output()
            .expect("run terminus")
    }

    /// Runs a command that must succeed and returns its standard output.
    fn ok(&self, args: &[&str]) -> String {
        let out = self.cli(args);
        let stdout = String::from_utf8_lossy(&out.stdout).into_owned();
        assert!(
            out.status.success(),
            "terminus {} failed: {}{}",
            args.join(" "),
            stdout,
            String::from_utf8_lossy(&out.stderr)
        );
        stdout
    }

    /// Runs a command that prints JSON and parses it.
    fn json(&self, args: &[&str]) -> Value {
        let s = self.ok(args);
        serde_json::from_str(&s).unwrap_or_else(|e| panic!("terminus {} printed {s:?}: {e}", args.join(" ")))
    }

    /// Polls `screen` for a pane until it contains `needle`.
    fn wait_screen(&self, pane: &str, needle: &str) -> String {
        let deadline = Instant::now() + Duration::from_secs(20);
        loop {
            let s = self.ok(&["+screen", "--pane", pane]);
            if s.contains(needle) {
                return s;
            }
            assert!(Instant::now() < deadline, "pane {pane} never showed {needle:?}; screen:\n{s}");
            std::thread::sleep(Duration::from_millis(50));
        }
    }
}

/// The test configuration: a shell that starts in milliseconds, with shell integration.
fn config_text(scrollback: u64) -> String {
    let shell = if cfg!(windows) { "cmd.exe" } else { "bash" };
    format!("[shell]\nprogram = \"{shell}\"\nintegration = true\n\n[scrollback]\nlimit_bytes = {scrollback}\n")
}

/// The pane ids in `list` output, in order.
fn pane_ids(list: &Value) -> Vec<u64> {
    let mut out = Vec::new();
    for w in list["windows"].as_array().into_iter().flatten() {
        for t in w["tabs"].as_array().into_iter().flatten() {
            for p in t["panes"].as_array().into_iter().flatten() {
                out.push(p["id"].as_u64().unwrap());
            }
        }
    }
    out
}

/// The `list` entry of a pane.
fn pane_entry(list: &Value, id: u64) -> Value {
    for w in list["windows"].as_array().into_iter().flatten() {
        for t in w["tabs"].as_array().into_iter().flatten() {
            for p in t["panes"].as_array().into_iter().flatten() {
                if p["id"] == id {
                    let mut p = p.clone();
                    p["tab_title"] = t["title"].clone();
                    return p;
                }
            }
        }
    }
    panic!("pane {id} is not in the list: {list}");
}

/// Arguments that make each action runnable against `pane` through `run-action`.
fn run_action_args(name: &str, pane: u64) -> Value {
    match name {
        "send-text" => json!({ "pane": pane, "text": "" }),
        "send-key" => json!({ "pane": pane, "key": "escape" }),
        "resize" => json!({ "pane": pane, "cols": 80, "rows": 24 }),
        "resize-split" => json!({ "pane": pane, "direction": "right" }),
        "move-tab-to-window" => json!({ "pane": pane }),
        "set-theme" => json!({ "name": "Terminus Light" }),
        "activate-key-table" => json!({ "name": "e5", "once": true }),
        "set-title" => json!({ "pane": pane, "title": "run-action" }),
        "run-action" => json!({ "name": "version" }),
        "wait-prompt" => json!({ "pane": pane, "timeout": 20 }),
        "search" => json!({ "pane": pane, "pattern": "E5" }),
        "install-shell-integration" => json!({ "shells": "bash", "dry-run": true }),
        "new-window"
        | "new-tab"
        | "show-config"
        | "reload-config"
        | "open-config"
        | "version"
        | "list"
        | "quick-terminal"
        | "quit"
        | "deactivate-key-table"
        | "deactivate-all-key-tables" => json!({}),
        "split" | "split-right" | "split-down" => json!({ "pane": pane }),
        _ => json!({ "pane": pane }),
    }
}

/// Every command line action, directly.
fn direct_commands(inst: &Instance, covered: &mut BTreeSet<String>) -> (String, String) {
    let mut seen = |s: &str| {
        covered.insert(s.to_string());
    };
    assert!(inst.ok(&["+version"]).contains(terminus_protocol::VERSION));
    seen("version");
    let first = inst.json(&["+new-window"]);
    seen("new-window");
    let p1 = first["pane"].as_u64().unwrap().to_string();
    let cwd = inst.dir.join("cwd");
    std::fs::create_dir_all(&cwd).unwrap();
    let tab = inst.json(&["+new-tab", "--cwd", cwd.to_str().unwrap(), "--name", "e5 tab"]);
    seen("new-tab");
    let p2 = tab["pane"].as_u64().unwrap().to_string();
    let entry = pane_entry(&inst.json(&["+list"]), p2.parse().unwrap());
    seen("list");
    assert_eq!(entry["tab_title"], "e5 tab");
    assert_eq!(PathBuf::from(entry["cwd"].as_str().unwrap()), cwd);
    inst.ok(&["+wait-prompt", "--pane", &p2, "--timeout", "30"]);
    seen("wait-prompt");
    inst.ok(&["+send-text", "--pane", &p2, "echo E5_ONE\\n"]);
    seen("send-text");
    inst.ok(&["+wait-prompt", "--pane", &p2, "--timeout", "30"]);
    inst.wait_screen(&p2, "E5_ONE");
    seen("screen");
    let grid = inst.json(&["+screen", "--pane", &p2, "--json"]);
    assert!(grid["lines"].as_array().is_some_and(|l| !l.is_empty()), "{grid}");
    inst.ok(&["+send-text", "--pane", &p2, "echo E5_KEY"]);
    inst.ok(&["+send-key", "--pane", &p2, "enter"]);
    seen("send-key");
    inst.ok(&["+wait-prompt", "--pane", &p2, "--timeout", "30"]);
    assert!(inst.ok(&["+scrollback", "--pane", &p2, "--lines", "50"]).contains("E5_KEY"));
    seen("scrollback");
    (p1, p2)
}

/// Layout, titles, search and the configuration commands.
fn more_commands(inst: &Instance, p2: &str, covered: &mut BTreeSet<String>) {
    let mut seen = |s: &str| {
        covered.insert(s.to_string());
    };
    inst.ok(&["+resize", "--pane", p2, "--cols", "90", "--rows", "20"]);
    seen("resize");
    let e = pane_entry(&inst.json(&["+list"]), p2.parse().unwrap());
    assert_eq!((e["cols"].as_u64(), e["rows"].as_u64()), (Some(90), Some(20)));
    inst.ok(&["+set-title", "--pane", p2, "renamed"]);
    seen("set-title");
    assert_eq!(pane_entry(&inst.json(&["+list"]), p2.parse().unwrap())["tab_title"], "renamed");
    let p3 = inst.json(&["+split", "--pane", p2, "--direction", "down"])["pane"].as_u64().unwrap().to_string();
    seen("split");
    assert_eq!(inst.json(&["+resize-split", "--pane", p2, "--direction", "down", "--percent", "10"])["moved"], true);
    seen("resize-split");
    let moved = inst.json(&["+move-tab-to-window", "--pane", &p3]);
    assert_eq!(moved["moved"], true, "{moved}");
    seen("move-tab-to-window");
    inst.ok(&["+focus", "--pane", &p3, "--direction", "up"]);
    seen("focus");
    assert_eq!(pane_entry(&inst.json(&["+list"]), p2.parse().unwrap())["focused"], true);
    assert_eq!(inst.json(&["+search", "--pane", p2, "--pattern", "E5_ONE"])["found"], true);
    seen("search");
    inst.ok(&["+clear-scrollback", "--pane", p2]);
    seen("clear-scrollback");
    inst.ok(&["+close", "--pane", &p3]);
    seen("close");
    assert!(!pane_ids(&inst.json(&["+list"])).contains(&p3.parse().unwrap()));
    inst.ok(&["+quick-terminal"]);
    seen("quick-terminal");
    assert!(inst.ok(&["+show-config", "--default"]).contains("[font]"));
    std::fs::write(&inst.config, config_text(7_654_321)).unwrap();
    inst.ok(&["+reload-config"]);
    seen("reload-config");
    assert!(inst.ok(&["+show-config"]).contains("7654321"), "reload-config did not apply the new file");
    seen("show-config");
    inst.ok(&["+set-theme", "Terminus Light"]);
    assert!(std::fs::read_to_string(&inst.config).unwrap().contains("theme = \"Terminus Light\""));
    assert!(inst.ok(&["+show-config"]).contains("Terminus Light"));
    seen("set-theme");
    inst.ok(&["+install-shell-integration", "--shells", "bash", "--dry-run"]);
    seen("install-shell-integration");
    inst.ok(&["+run-action", "--name", "version"]);
    seen("run-action");
    let active = |v: Value| {
        v["active"].as_array().map(|a| a.iter().filter_map(|t| t.as_str().map(String::from)).collect::<Vec<_>>())
    };
    assert_eq!(active(inst.json(&["+activate-key-table", "--name", "a"])), Some(vec!["a".to_string()]));
    assert_eq!(
        active(inst.json(&["+activate-key-table", "--name", "b", "--once"])),
        Some(vec!["a".to_string(), "b".to_string()])
    );
    seen("activate-key-table");
    assert_eq!(active(inst.json(&["+deactivate-key-table"])), Some(vec!["a".to_string()]));
    seen("deactivate-key-table");
    inst.ok(&["+activate-key-table", "--name", "c"]);
    assert_eq!(active(inst.json(&["+deactivate-all-key-tables"])), Some(Vec::new()));
    seen("deactivate-all-key-tables");
}

/// Every catalogue action through `run-action`: none may be unknown to `run_action`.
fn every_action(inst: &Instance, covered: &mut BTreeSet<String>) {
    for a in CATALOGUE.iter().filter(|a| a.name != "quit" && a.name != "close") {
        let pane = inst.json(&["+new-tab"])["pane"].as_u64().unwrap();
        inst.ok(&["+wait-prompt", "--pane", &pane.to_string(), "--timeout", "30"]);
        let args = run_action_args(a.name, pane).to_string();
        let out = inst.cli(&["+run-action", "--name", a.name, "--args", &args]);
        let err = String::from_utf8_lossy(&out.stderr);
        assert!(!err.contains("unknown action"), "{} did not reach run_action: {err}", a.name);
        covered.insert(format!("run-action:{}", a.name));
    }
    let pane = inst.json(&["+new-tab"])["pane"].as_u64().unwrap();
    inst.ok(&["+run-action", "--name", "close", "--args", &json!({ "pane": pane }).to_string()]);
    covered.insert("run-action:close".into());
}

/// A1: every catalogue row reached run_action, has a docs section, and was exercised.
fn a1_coverage(covered: &BTreeSet<String>) {
    let docs = std::fs::read_to_string(Path::new(env!("CARGO_MANIFEST_DIR")).join("../../docs/commands.md")).unwrap();
    let mut missing = Vec::new();
    for a in CATALOGUE {
        if !docs.contains(&format!("\n## {}\n", a.name)) {
            missing.push(format!("{}: no section in docs/commands.md", a.name));
        }
        if a.cli && !covered.contains(a.name) {
            missing.push(format!("{}: no direct command in E5", a.name));
        }
        if !covered.contains(&format!("run-action:{}", a.name)) {
            missing.push(format!("{}: not run through run-action in E5", a.name));
        }
    }
    assert!(missing.is_empty(), "A1:\n{}", missing.join("\n"));
}

/// The socket grants access to the current user only.
fn owner_only(inst: &Instance) {
    let access = terminus_app::ipc_server::describe_access(&inst.socket).expect("read the socket's access rules");
    if cfg!(windows) {
        assert!(access.starts_with("D:P"), "the DACL is not protected: {access}");
        assert_eq!(access.matches("(A;").count(), 1, "more than one allow entry: {access}");
        // A user account's SID, which SDDL writes as LA for the built in Administrator (as on CI
        // runners) and in full for every other account.
        assert!(
            access.contains(";;;S-1-5-21-") || access.ends_with(";;;LA)"),
            "the entry is not for a user account: {access}"
        );
    } else {
        assert_eq!(access, "600");
    }
}

#[test]
fn e5_every_command_through_the_cli() {
    let inst = Instance::start("all");
    let mut covered = BTreeSet::new();
    owner_only(&inst);
    let (_p1, p2) = direct_commands(&inst, &mut covered);
    more_commands(&inst, &p2, &mut covered);
    every_action(&inst, &mut covered);
    let out = inst.cli(&["+run-action", "--name", "quit"]);
    assert!(out.status.success());
    covered.insert("run-action:quit".into());
    a1_coverage(&covered);
}

#[test]
fn e5_errors_are_reported() {
    let inst = Instance::start("errors");
    let out = inst.cli(&["+screen", "--pane", "999"]);
    assert!(!out.status.success());
    assert!(String::from_utf8_lossy(&out.stderr).contains("not found"));
    assert!(!inst.cli(&["+nope"]).status.success());
    assert!(!inst.cli(&["+resize", "--cols", "10"]).status.success());
    inst.ok(&["+run-action", "--name", "quit"]);
}

/// Stops a process when the test ends, passed or failed, so a separate instance never outlives it
/// (cargo waits for every process holding the test's output).
struct KillOnDrop(u32);

impl Drop for KillOnDrop {
    /// Ends the process.
    fn drop(&mut self) {
        let pid = self.0.to_string();
        let _ = if cfg!(windows) {
            Command::new("taskkill").args(["/PID", &pid, "/F"]).output()
        } else {
            Command::new("kill").arg(&pid).output()
        };
    }
}

/// Runs `terminus --socket <socket>` with arguments.
fn cli_at(socket: &str, args: &[&str]) -> Output {
    Command::new(cli_exe())
        .arg("--socket")
        .arg(socket)
        .args(args)
        .env_remove("TERMINUS_SOCKET")
        .output()
        .expect("run terminus")
}

/// Polls a command against a socket until it succeeds and its output satisfies `ok`.
fn poll_at(socket: &str, args: &[&str], ok: impl Fn(&str) -> bool) -> String {
    let deadline = Instant::now() + Duration::from_secs(30);
    loop {
        let out = cli_at(socket, args);
        let text = String::from_utf8_lossy(&out.stdout).into_owned();
        if out.status.success() && ok(&text) {
            return text;
        }
        assert!(
            Instant::now() < deadline,
            "terminus --socket {socket} {} never gave the expected output; last: {text}{}",
            args.join(" "),
            String::from_utf8_lossy(&out.stderr)
        );
        std::thread::sleep(Duration::from_millis(100));
    }
}

/// docs/decisions/detach.md: `new-window --separate` starts an instance with its own socket; a
/// pane of it reaches that instance with `terminus +list`; it keeps running when the primary stops.
#[test]
fn e5_separate_instance() {
    let mut inst = Instance::start("sep");
    let r = inst.json(&["+new-window", "--separate"]);
    let socket = r["socket"].as_str().expect("the answer names the socket").to_string();
    let _separate = KillOnDrop(r["pid"].as_u64().expect("the answer names the pid") as u32);
    let list: Value = serde_json::from_str(&poll_at(&socket, &["+list"], |t| {
        pane_ids(&serde_json::from_str(t).unwrap_or(Value::Null)).len() == 1
    }))
    .unwrap();
    let pane = pane_ids(&list)[0];
    let shell_pid = pane_entry(&list, pane)["pid"].as_u64().expect("pid");
    poll_at(&socket, &["+wait-prompt", "--pane", &pane.to_string(), "--timeout", "30"], |_| true);
    // send-text unescapes backslash sequences, so the path's backslashes are doubled.
    let cmd = format!("\"{}\" +list\n", cli_exe().display().to_string().replace('\\', "\\\\"));
    assert!(cli_at(&socket, &["+send-text", "--pane", &pane.to_string(), &cmd]).status.success());
    poll_at(&socket, &["+screen", "--pane", &pane.to_string()], |t| t.contains(&format!("\"pid\": {shell_pid}")));
    let _ = inst.child.kill();
    let _ = inst.child.wait();
    assert!(!inst.cli(&["+list"]).status.success(), "the primary still answers after being stopped");
    poll_at(&socket, &["+list"], |t| t.contains(&format!("\"pid\": {shell_pid}")));
    assert!(cli_at(&socket, &["+send-text", "--pane", &pane.to_string(), "echo SEP_ALIVE\n"]).status.success());
    poll_at(&socket, &["+screen", "--pane", &pane.to_string()], |t| t.lines().any(|l| l.trim() == "SEP_ALIVE"));
    let _ = cli_at(&socket, &["+run-action", "--name", "quit"]);
}
