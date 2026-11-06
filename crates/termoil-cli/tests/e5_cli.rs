//! E5 and A1 (docs/TEST-SUITE.md sections 11 and 14).
//!
//! E5 starts `termoil-gui --headless` on a private socket with a private config file and drives
//! it with the real `termoil` program: every command line action directly, and every catalogue
//! action through `run-action`. A1 then checks, naming what is missing, that every catalogue row
//! reached `run_action` (it did not answer "unknown action"), has a section in docs/commands.md, and
//! was exercised here. The socket must be readable and writable by the current user only.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Output};
use std::time::{Duration, Instant};

use serde_json::{Value, json};
use termoil_protocol::CATALOGUE;

/// The `termoil` program under test.
fn cli_exe() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_termoil"))
}

/// `termoil-gui` next to it, built first when missing.
fn gui_exe() -> PathBuf {
    let exe = cli_exe().with_file_name(if cfg!(windows) { "termoil-gui.exe" } else { "termoil-gui" });
    if !exe.is_file() {
        let manifest = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../Cargo.toml");
        let mut cmd = Command::new(env!("CARGO"));
        cmd.args(["build", "-p", "termoil-app", "--bin", "termoil-gui", "--manifest-path"]).arg(manifest);
        if !cfg!(debug_assertions) {
            cmd.arg("--release");
        }
        assert!(cmd.status().expect("cargo build").success(), "building termoil-gui failed");
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
        let dir = std::env::temp_dir().join(format!("termoil-e5-{tag}-{}", std::process::id()));
        Instance::start_in(dir, tag, config_text(1_000_000))
    }

    /// Starts an instance in `dir` (kept if it exists, so a second instance can reuse the data folder)
    /// with the given configuration text.
    fn start_in(dir: PathBuf, tag: &str, config_file: String) -> Instance {
        Instance::start_with(dir, tag, config_file, &[])
    }

    /// Starts an instance in `dir` with the given configuration text and extra environment variables.
    fn start_with(dir: PathBuf, tag: &str, config_file: String, env: &[(&str, &str)]) -> Instance {
        std::fs::create_dir_all(&dir).unwrap();
        let config = dir.join("config.toml");
        std::fs::write(&config, config_file).unwrap();
        let socket = if cfg!(windows) {
            format!(r"\\.\pipe\termoil-e5-{tag}-{}", std::process::id())
        } else {
            dir.join(format!("t{}.sock", std::time::UNIX_EPOCH.elapsed().unwrap().as_nanos()))
                .to_string_lossy()
                .into_owned()
        };
        let releases = release_server();
        let mut command = Command::new(gui_exe());
        // The E5 instances never send usage statistics unless a test says so (`TERMOIL_TELEMETRY`).
        command.env("TERMOIL_TELEMETRY", "off");
        for (key, value) in env {
            command.env(key, value);
        }
        let child = command
            .arg("--headless")
            .env("TERMOIL_SOCKET", &socket)
            .env("TERMOIL_CONFIG", &config)
            .env("TERMOIL_DATA_DIR", dir.join("data"))
            .env("TERMOIL_RELEASES_STABLE", format!("{releases}/stable.json"))
            .env("TERMOIL_RELEASES_NIGHTLY", format!("{releases}/nightly.json"))
            .spawn()
            .expect("start termoil-gui");
        let inst = Instance { child, socket, config, dir };
        let deadline = Instant::now() + Duration::from_secs(20);
        while inst.cli(&["+list"]).status.code() != Some(0) {
            assert!(Instant::now() < deadline, "the headless instance never answered");
            std::thread::sleep(Duration::from_millis(50));
        }
        inst
    }

    /// Runs `termoil` with arguments against this instance.
    fn cli(&self, args: &[&str]) -> Output {
        Command::new(cli_exe())
            .args(args)
            .env("TERMOIL_SOCKET", &self.socket)
            .env("TERMOIL_CONFIG", &self.config)
            .output()
            .expect("run termoil")
    }

    /// Runs a command that must succeed and returns its standard output.
    fn ok(&self, args: &[&str]) -> String {
        let out = self.cli(args);
        let stdout = String::from_utf8_lossy(&out.stdout).into_owned();
        assert!(
            out.status.success(),
            "termoil {} failed: {}{}",
            args.join(" "),
            stdout,
            String::from_utf8_lossy(&out.stderr)
        );
        stdout
    }

    /// Runs a command that prints JSON and parses it.
    fn json(&self, args: &[&str]) -> Value {
        let s = self.ok(args);
        serde_json::from_str(&s).unwrap_or_else(|e| panic!("termoil {} printed {s:?}: {e}", args.join(" ")))
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
        "set-theme" => json!({ "name": "Termoil Light" }),
        "activate-key-table" => json!({ "name": "e5", "once": true }),
        "set-title" => json!({ "pane": pane, "title": "run-action" }),
        "run-action" => json!({ "name": "version" }),
        "wait-prompt" => json!({ "pane": pane, "timeout": 20 }),
        "search" => json!({ "pane": pane, "pattern": "E5" }),
        "install-shell-integration" => json!({ "shells": "bash", "dry-run": true }),
        "install-update" => json!({ "restart": false }),
        "skip-update" => json!({ "version": "98.0.0" }),
        "check-for-updates" | "list-updates" | "telemetry-status" => json!({}),
        "save-session" => json!({}),
        "new-window"
        | "new-tab"
        | "show-config"
        | "reload-config"
        | "open-config"
        | "version"
        | "list"
        | "list-themes"
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
    assert!(inst.ok(&["+version"]).contains(termoil_protocol::VERSION));
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
    let doomed = inst.json(&["+new-tab"])["pane"].as_u64().unwrap();
    let closed = inst.json(&["+close-tab", "--pane", &doomed.to_string()]);
    assert_eq!(closed["closed"], json!([doomed]), "{closed}");
    seen("close-tab");
    assert!(!pane_ids(&inst.json(&["+list"])).contains(&doomed));
    inst.ok(&["+quick-terminal"]);
    seen("quick-terminal");
    assert!(inst.ok(&["+show-config", "--default"]).contains("[font]"));
    std::fs::write(&inst.config, config_text(7_654_321)).unwrap();
    inst.ok(&["+reload-config"]);
    seen("reload-config");
    assert!(inst.ok(&["+show-config"]).contains("7654321"), "reload-config did not apply the new file");
    seen("show-config");
    let listed = inst.json(&["+list-themes"]);
    seen("list-themes");
    assert_eq!(listed["featured"][0], "Unluminous Dark", "{listed}");
    assert!(listed["themes"].as_array().is_some_and(|t| t.len() > 800 && t.iter().any(|n| n == "Termoil Light")));
    inst.ok(&["+set-theme", "Termoil Light"]);
    assert!(std::fs::read_to_string(&inst.config).unwrap().contains("theme = \"Termoil Light\""));
    assert!(inst.ok(&["+show-config"]).contains("Termoil Light"));
    seen("set-theme");
    let saved = inst.json(&["+save-session"]);
    seen("save-session");
    let session_file = inst.dir.join("data").join("session.json");
    assert_eq!(PathBuf::from(saved["path"].as_str().unwrap()), session_file);
    let text = std::fs::read_to_string(&session_file).expect("save-session wrote the session file");
    assert!(serde_json::from_str::<Value>(&text).is_ok_and(|v| v["version"] == 1 && v["windows"].is_array()));
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

/// The stable channel the release server offers: one version older than any build under test.
const STABLE_VERSION: &str = "1.0.0";
/// The nightly channel: a newer version with a good package and an older one whose SHA-256 is wrong.
const NIGHTLY_GOOD: &str = "99.1.0";
const NIGHTLY_BAD: &str = "99.0.0";
/// The package every release on the server points at.
const PACKAGE: &[u8] = b"not really an installer, but the bytes the manifest promises";

/// One version in the site's manifest shape, every kind pointing at the same package.
fn manifest_entry(base: &str, version: &str, sha256: &str) -> Value {
    let file = json!({ "file": format!("pkg-{version}.bin"), "url": format!("{base}/pkg.bin"), "bytes": PACKAGE.len(), "sha256": sha256 });
    let files = json!({ "windows": file, "macos": file, "deb": file, "appimage": file });
    json!({ "version": version, "date": "2026-11-06", "notes": format!("{base}/notes/{version}"), "files": files })
}

/// A channel manifest: the newest entry at the top level and every entry in `versions`.
fn manifest(channel: &str, entries: Vec<Value>) -> String {
    let mut top = entries[0].clone();
    top["product"] = json!("Termoil");
    top["channel"] = json!(channel);
    top["versions"] = json!(entries);
    top.to_string()
}

/// Starts, once per test process, a local HTTP server with `stable.json`, `nightly.json` and the package,
/// and answers its base URL. The instances under test are pointed at it, so no test reaches the network.
fn release_server() -> String {
    static BASE: std::sync::OnceLock<String> = std::sync::OnceLock::new();
    BASE.get_or_init(|| {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").expect("bind the release server");
        let base = format!("http://{}", listener.local_addr().unwrap());
        let dir = std::env::temp_dir().join(format!("termoil-e5-pkg-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("pkg.bin"), PACKAGE).unwrap();
        let sha = termoil_app::update::install::sha256_file(&dir.join("pkg.bin")).unwrap();
        let _ = std::fs::remove_dir_all(&dir);
        let stable = manifest("stable", vec![manifest_entry(&base, STABLE_VERSION, &sha)]);
        let wrong = "0".repeat(64);
        let nightly = manifest(
            "nightly",
            vec![manifest_entry(&base, NIGHTLY_GOOD, &sha), manifest_entry(&base, NIGHTLY_BAD, &wrong)],
        );
        std::thread::spawn(move || {
            for stream in listener.incoming().flatten() {
                serve_one(stream, &stable, &nightly);
            }
        });
        base
    })
    .clone()
}

/// Answers one HTTP request from the release server's three files, or 404.
fn serve_one(mut stream: std::net::TcpStream, stable: &str, nightly: &str) {
    use std::io::{BufRead, BufReader, Write};
    let mut line = String::new();
    let mut reader = BufReader::new(stream.try_clone().unwrap());
    if reader.read_line(&mut line).is_err() {
        return;
    }
    loop {
        let mut header = String::new();
        if reader.read_line(&mut header).map_or(true, |n| n == 0) || header.trim().is_empty() {
            break;
        }
    }
    let path = line.split_whitespace().nth(1).unwrap_or("/");
    let (status, body): (&str, &[u8]) = match path {
        "/stable.json" => ("200 OK", stable.as_bytes()),
        "/nightly.json" => ("200 OK", nightly.as_bytes()),
        "/pkg.bin" => ("200 OK", PACKAGE),
        _ => ("404 Not Found", b"not here"),
    };
    let head = format!("HTTP/1.1 {status}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n", body.len());
    let _ = stream.write_all(head.as_bytes()).and_then(|_| stream.write_all(body));
}

/// The update actions against the release server: both channels checked and listed, a package downloaded
/// and checked, a package with the wrong SHA-256 refused and deleted, an unknown version refused, and a
/// skipped version written to the configuration.
fn update_commands(inst: &Instance, covered: &mut BTreeSet<String>) {
    let stable = inst.json(&["+check-for-updates"]);
    assert_eq!(
        (stable["channel"].as_str(), stable["latest"].as_str(), stable["newer"].as_bool()),
        (Some("stable"), Some(STABLE_VERSION), Some(false))
    );
    let nightly = inst.json(&["+check-for-updates", "--channel", "nightly"]);
    assert_eq!((nightly["latest"].as_str(), nightly["newer"].as_bool()), (Some(NIGHTLY_GOOD), Some(true)));
    assert_eq!(nightly["bytes"].as_u64(), Some(PACKAGE.len() as u64));
    covered.insert("check-for-updates".into());
    let listed = inst.json(&["+list-updates"]);
    let names = |ch: &str| {
        listed[ch].as_array().unwrap().iter().map(|v| v["version"].as_str().unwrap().to_string()).collect::<Vec<_>>()
    };
    assert_eq!(
        (names("stable"), names("nightly")),
        (vec![STABLE_VERSION.to_string()], vec![NIGHTLY_GOOD.to_string(), NIGHTLY_BAD.to_string()])
    );
    covered.insert("list-updates".into());
    let got = inst.json(&["+install-update", "--channel", "nightly", "--restart", "false"]);
    let file = PathBuf::from(got["file"].as_str().unwrap());
    assert_eq!(std::fs::read(&file).unwrap(), PACKAGE, "the downloaded package differs");
    let bad = inst.cli(&["+install-update", "--version", NIGHTLY_BAD, "--restart", "false"]);
    let err = String::from_utf8_lossy(&bad.stderr);
    assert!(!bad.status.success() && err.contains("SHA-256"), "a wrong SHA-256 was not refused: {err}");
    let bad_folder = file.parent().unwrap().with_file_name(format!("termoil-update-{NIGHTLY_BAD}"));
    assert!(!bad_folder.join(format!("pkg-{NIGHTLY_BAD}.bin")).exists(), "the refused package was kept");
    let missing = inst.cli(&["+install-update", "--version", "42.0.0", "--restart", "false"]);
    assert!(String::from_utf8_lossy(&missing.stderr).contains("not on the"), "an unknown version was not refused");
    covered.insert("install-update".into());
    assert_eq!(inst.json(&["+skip-update", "--version", NIGHTLY_GOOD])["skip"].as_str(), Some(NIGHTLY_GOOD));
    let text = std::fs::read_to_string(&inst.config).unwrap();
    assert!(text.contains(&format!("skip = \"{NIGHTLY_GOOD}\"")), "updates.skip was not written:\n{text}");
    covered.insert("skip-update".into());
    let _ = std::fs::remove_dir_all(file.parent().unwrap());
    let _ = std::fs::remove_dir_all(bad_folder);
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
    let access = termoil_app::ipc_server::describe_access(&inst.socket).expect("read the socket's access rules");
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
    update_commands(&inst, &mut covered);
    let status = inst.json(&["+telemetry-status"]);
    assert_eq!((status["enabled"].as_bool(), status["offBecause"].as_str()), (Some(false), Some("environment")));
    covered.insert("telemetry-status".into());
    every_action(&inst, &mut covered);
    let out = inst.cli(&["+run-action", "--name", "quit"]);
    assert!(out.status.success());
    covered.insert("run-action:quit".into());
    a1_coverage(&covered);
}

#[test]
fn e5_session_is_restored_after_a_restart() {
    let dir = std::env::temp_dir().join(format!("termoil-e5-restore-{}", std::process::id()));
    let config = format!(
        "{}
[session]
restore = true
",
        config_text(1_000_000)
    );
    let work = dir.join("work");
    std::fs::create_dir_all(&work).unwrap();
    let first = Instance::start_in(dir.clone(), "restore1", config.clone());
    let p1 = first.json(&["+new-window", "--cwd", work.to_str().unwrap()])["pane"].as_u64().unwrap().to_string();
    first.ok(&["+wait-prompt", "--pane", &p1, "--timeout", "30"]);
    first.ok(&["+send-text", "--pane", &p1, "echo RESTORE_ME\\n"]);
    first.wait_screen(&p1, "RESTORE_ME");
    first.ok(&["+new-tab", "--name", "kept tab"]);
    first.ok(&["+split", "--pane", &p1, "--direction", "down"]);
    first.ok(&["+run-action", "--name", "quit"]);
    let mut first = first;
    let _ = first.child.wait();
    let session_file = dir.join("data").join("session.json");
    assert!(session_file.is_file(), "quit did not write the session file");
    let second = Instance::start_in(dir.clone(), "restore2", config);
    assert!(!session_file.exists(), "the session file was not removed after restoring");
    let list = second.json(&["+list"]);
    assert_eq!(pane_ids(&list).len(), 3, "{list}");
    let panes = pane_ids(&list);
    let titles: Vec<String> =
        list["windows"][0]["tabs"].as_array().unwrap().iter().map(|t| t["title"].to_string()).collect();
    assert!(titles.iter().any(|t| t.contains("kept tab")), "{titles:?}");
    let pane = panes[0].to_string();
    let deadline = Instant::now() + Duration::from_secs(20);
    while !second.ok(&["+scrollback", "--pane", &pane, "--lines", "200"]).contains("RESTORE_ME") {
        assert!(Instant::now() < deadline, "the restored pane never showed the saved output");
        std::thread::sleep(Duration::from_millis(50));
    }
    second.ok(&["+run-action", "--name", "quit"]);
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

/// Runs `termoil --socket <socket>` with arguments.
fn cli_at(socket: &str, args: &[&str]) -> Output {
    Command::new(cli_exe())
        .arg("--socket")
        .arg(socket)
        .args(args)
        .env_remove("TERMOIL_SOCKET")
        .output()
        .expect("run termoil")
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
            "termoil --socket {socket} {} never gave the expected output; last: {text}{}",
            args.join(" "),
            String::from_utf8_lossy(&out.stderr)
        );
        std::thread::sleep(Duration::from_millis(100));
    }
}

/// docs/decisions/detach.md: `new-window --separate` starts an instance with its own socket; a
/// pane of it reaches that instance with `termoil +list`; it keeps running when the primary stops.
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

/// A scripted Measurement Protocol server: answers 204 to every POST and keeps each body it was sent.
fn collector() -> (String, std::sync::Arc<std::sync::Mutex<Vec<Value>>>) {
    use std::io::{BufRead, BufReader, Read, Write};
    let listener = std::net::TcpListener::bind("127.0.0.1:0").expect("bind the collector");
    let base = format!("http://{}/mp/collect", listener.local_addr().unwrap());
    let bodies = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
    let kept = bodies.clone();
    std::thread::spawn(move || {
        for mut stream in listener.incoming().flatten() {
            let mut reader = BufReader::new(stream.try_clone().unwrap());
            let mut length = 0usize;
            loop {
                let mut header = String::new();
                if reader.read_line(&mut header).map_or(true, |n| n == 0) || header.trim().is_empty() {
                    break;
                }
                if let Some(v) = header.to_ascii_lowercase().strip_prefix("content-length:") {
                    length = v.trim().parse().unwrap_or(0);
                }
            }
            let mut body = vec![0u8; length];
            if reader.read_exact(&mut body).is_ok()
                && let Ok(v) = serde_json::from_slice::<Value>(&body)
            {
                kept.lock().unwrap().push(v);
            }
            let _ = stream.write_all(
                b"HTTP/1.1 204 No Content
Connection: close

",
            );
        }
    });
    (base, bodies)
}

/// Every `(event name, feature)` pair the collector received, `feature` empty for other events.
fn received(bodies: &std::sync::Mutex<Vec<Value>>) -> Vec<(String, String)> {
    let mut out = Vec::new();
    for body in bodies.lock().unwrap().iter() {
        for e in body["events"].as_array().into_iter().flatten() {
            let feature = format!(
                "{}:{}",
                e["params"]["category"].as_str().unwrap_or(""),
                e["params"]["feature"].as_str().unwrap_or("")
            );
            out.push((e["name"].as_str().unwrap_or("").to_string(), feature));
        }
    }
    out
}

/// Waits for the instance to exit after `quit`, so its closing request has been made.
fn quit_and_wait(mut inst: Instance) {
    let _ = inst.cli(&["+run-action", "--name", "quit"]);
    let deadline = Instant::now() + Duration::from_secs(20);
    while inst.child.try_wait().ok().flatten().is_none() {
        assert!(Instant::now() < deadline, "the instance did not quit");
        std::thread::sleep(Duration::from_millis(50));
    }
}

#[test]
fn e5_telemetry_sends_counts_to_the_collector_and_nothing_when_off() {
    let (url, bodies) = collector();
    let env = [("TERMOIL_TELEMETRY", "on"), ("TERMOIL_TELEMETRY_URL", url.as_str())];
    let dir = std::env::temp_dir().join(format!("termoil-e5-telemetry-{}", std::process::id()));
    let inst = Instance::start_with(dir.clone(), "telemetry", config_text(1_000_000), &env);
    let pane = inst.json(&["+new-tab"])["pane"].as_u64().unwrap().to_string();
    inst.ok(&["+wait-prompt", "--pane", &pane, "--timeout", "30"]);
    inst.ok(&["+version"]);
    let status = inst.json(&["+telemetry-status", "--flush", "true"]);
    assert_eq!(status["enabled"].as_bool(), Some(true), "{status}");
    assert!(status["posted"].as_u64() >= Some(1) && status["failed"].as_u64() == Some(0), "{status}");
    quit_and_wait(inst);
    let got = received(&bodies);
    for want in [
        ("app_open", ":"),
        ("app_heartbeat", ":"),
        ("feature", "action:new-tab"),
        ("feature", "ipc:version"),
        ("app_close", ":"),
    ] {
        assert!(got.iter().any(|(e, f)| e == want.0 && f == want.1), "{want:?} was not sent: {got:?}");
    }
    let id = std::fs::read_to_string(dir.join("data/telemetry/id.txt")).unwrap();
    assert!(id.len() == 32 && id.chars().all(|c| c.is_ascii_hexdigit()), "client id {id:?}");
    for body in bodies.lock().unwrap().iter() {
        assert_eq!(body["client_id"].as_str(), Some(id.trim()));
        assert!(!body.to_string().contains(dir.to_string_lossy().as_ref()), "a path was sent: {body}");
    }
    bodies.lock().unwrap().clear();
    let off = format!(
        "{}
[telemetry]
enabled = false
",
        config_text(1_000_000)
    );
    let inst = Instance::start_with(dir.clone(), "telemetry-off", off, &env);
    inst.ok(&["+new-tab"]);
    let status = inst.json(&["+telemetry-status", "--flush", "true"]);
    assert_eq!((status["enabled"].as_bool(), status["offBecause"].as_str()), (Some(false), Some("setting")));
    quit_and_wait(inst);
    assert!(bodies.lock().unwrap().is_empty(), "telemetry off still sent: {:?}", received(&bodies));
    let _ = std::fs::remove_dir_all(&dir);
}
