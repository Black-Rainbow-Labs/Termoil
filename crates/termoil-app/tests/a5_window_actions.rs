//! A5: every action a person runs from the window is run and they are told what it answered
//! (`docs/decisions/window-actions.md`, `docs/TEST-SUITE.md` section 14). task-2586 found that the
//! palette dropped what `run_action` returned, so check-for-updates and list-updates never made their
//! request and every answer and error was lost. These cases run without a display: a headless `App`
//! with a window running a real shell, the update check pointed at a release server in this process,
//! and the overlays driven with key events.

use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use serde_json::{Map, Value, json};
use termoil_app::answer::{ACTS, ANSWERS, Answer, Asked};
use termoil_app::app::{App, Effect, MemoryClipboard};
use termoil_app::config::Config;
use termoil_app::gui::overlay::{ANSWER_ROWS, After, AnswerUi, Overlay, PaletteUi, key};
use termoil_input::{Key, KeyEvent, Modifiers, NamedKey};
use termoil_protocol::{ActionCall, CATALOGUE, VERSION};

/// The newer version the nightly channel offers.
const NEWER: &str = "99.0.0";
/// A version the nightly channel lists with a wrong SHA-256.
const BROKEN: &str = "98.0.0";
/// The package every release on the server points at.
const PACKAGE: &[u8] = b"the bytes the A5 manifest promises";

/// What the answer sink received, in order.
type Received = Arc<Mutex<Vec<Answer>>>;

/// A fresh folder for one test.
fn scratch(tag: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("termoil-a5-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

/// An app whose panes run a fast shell, with its configuration file in `dir`, and the list its answer
/// sink fills.
fn new_app(dir: &std::path::Path) -> (App, Received) {
    let shell = if cfg!(windows) { "cmd.exe" } else { "bash" };
    let text = format!("[shell]\nprogram = \"{shell}\"\nintegration = false\n\n[updates]\ncheck = \"off\"\n");
    let path = dir.join("config.toml");
    std::fs::write(&path, &text).unwrap();
    let config: Config = toml::from_str(&text).expect("test config");
    let wakers: termoil_app::app::WakerFactory = Box::new(|_| Arc::new(|| {}));
    let mut app = App::new(config, path, Box::new(MemoryClipboard::default()), wakers);
    let received: Received = Arc::default();
    let sink = received.clone();
    app.answers = Some(Arc::new(move |a| sink.lock().unwrap().push(a)));
    (app, received)
}

/// The answers queued as effects since the last call, dropping the other effects.
fn shown(app: &mut App) -> Vec<Answer> {
    app.take_effects()
        .into_iter()
        .filter_map(|e| match e {
            Effect::ShowAnswer(a) => Some(a),
            _ => None,
        })
        .collect()
}

/// Runs an action the way the palette does and returns the answers it queued at once.
fn from_palette(app: &mut App, name: &str, args: Value) -> Vec<Answer> {
    let args: Map<String, Value> = args.as_object().cloned().unwrap_or_default();
    let _ = app.run_for_person(&ActionCall { action: name.into(), args }, Asked::Palette);
    shown(app)
}

/// Waits for the job started from the window to send its answer, and returns it.
fn job_answer(received: &Received, action: &str) -> Answer {
    let deadline = Instant::now() + Duration::from_secs(30);
    loop {
        let found = received.lock().unwrap().iter().position(|a| a.action == action);
        if let Some(i) = found {
            return received.lock().unwrap().remove(i);
        }
        assert!(Instant::now() < deadline, "{action} never sent its answer");
        std::thread::sleep(Duration::from_millis(20));
    }
}

/// Runs an action that starts a job: checks the waiting answer comes first, then returns the job's.
fn from_palette_later(app: &mut App, received: &Received, name: &str, args: Value) -> Answer {
    let now = from_palette(app, name, args);
    assert_eq!(now.len(), 1, "{name} queued {now:?}");
    assert!(now[0].pending && !now[0].title.is_empty(), "{name} did not show a waiting answer first: {:?}", now[0]);
    let answer = job_answer(received, name);
    assert!(!answer.pending, "{name}'s job answered a waiting answer");
    answer
}

/// Points the update check at a release server in this process and answers its base URL. The stable
/// channel offers the running version, the nightly channel a newer one and one with a wrong SHA-256.
fn release_server() -> String {
    let listener = std::net::TcpListener::bind("127.0.0.1:0").expect("bind the release server");
    let base = format!("http://{}", listener.local_addr().unwrap());
    let sha = format!("{:x}", <sha2::Sha256 as sha2::Digest>::digest(PACKAGE));
    let entry = |version: &str, sha: &str| {
        let file = json!({ "file": format!("pkg-{version}.bin"), "url": format!("{base}/pkg.bin"), "bytes": PACKAGE.len(), "sha256": sha });
        json!({ "version": version, "date": "2026-11-09", "notes": format!("{base}/notes/{version}"),
            "files": { "windows": file, "macos": file, "deb": file, "appimage": file } })
    };
    let manifest = |channel: &str, entries: Vec<Value>| {
        let mut top = entries[0].clone();
        top["product"] = json!("Termoil");
        top["channel"] = json!(channel);
        top["versions"] = json!(entries);
        top.to_string()
    };
    let stable = manifest("stable", vec![entry(VERSION, &sha)]);
    let nightly = manifest("nightly", vec![entry(NEWER, &sha), entry(BROKEN, &"0".repeat(64))]);
    std::thread::spawn(move || {
        for stream in listener.incoming().flatten() {
            serve_one(stream, &stable, &nightly);
        }
    });
    base
}

/// Answers one HTTP request from the release server's files, or 404.
fn serve_one(mut stream: std::net::TcpStream, stable: &str, nightly: &str) {
    use std::io::{BufRead, BufReader, Write};
    let mut reader = BufReader::new(stream.try_clone().unwrap());
    let mut line = String::new();
    if reader.read_line(&mut line).is_err() {
        return;
    }
    loop {
        let mut header = String::new();
        if reader.read_line(&mut header).map_or(true, |n| n == 0) || header.trim().is_empty() {
            break;
        }
    }
    let (status, body): (&str, &[u8]) = match line.split_whitespace().nth(1).unwrap_or("/") {
        "/stable.json" => ("200 OK", stable.as_bytes()),
        "/nightly.json" => ("200 OK", nightly.as_bytes()),
        "/pkg.bin" => ("200 OK", PACKAGE),
        _ => ("404 Not Found", b"not here"),
    };
    let head = format!("HTTP/1.1 {status}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n", body.len());
    let _ = stream.write_all(head.as_bytes()).and_then(|_| stream.write_all(body));
}

/// Points both channels at the server's files, or at a file it does not have.
fn point_releases(base: &str, stable: &str, nightly: &str) {
    // SAFETY: only the one test in this binary that runs update actions sets these, and it reads them on
    // its own threads after setting them.
    unsafe {
        std::env::set_var("TERMOIL_RELEASES_STABLE", format!("{base}/{stable}"));
        std::env::set_var("TERMOIL_RELEASES_NIGHTLY", format!("{base}/{nightly}"));
    }
}

/// Opens a window with a shell in it and waits for the shell to print something.
fn open_window(app: &mut App) -> u64 {
    let v = termoil_app::actions::new_window(app, None, None).expect("new-window");
    let _ = app.take_effects();
    let pane = v["pane"].as_u64().unwrap();
    let deadline = Instant::now() + Duration::from_secs(30);
    while termoil_app::inspect::scrollback_text(&app.panes[&pane].session.lock(), 50)
        .iter()
        .all(|l| l.trim().is_empty())
    {
        assert!(Instant::now() < deadline, "the shell in pane {pane} printed nothing");
        std::thread::sleep(Duration::from_millis(50));
    }
    pane
}

/// A key press with the text it types.
fn typed(c: char) -> KeyEvent {
    KeyEvent::press(Key::Char(c), Modifiers::NONE).with_text(&c.to_string())
}

/// A named key press.
fn named(k: NamedKey) -> KeyEvent {
    KeyEvent::press(Key::Named(k), Modifiers::NONE)
}

/// Types text into an overlay.
fn type_into(ov: &mut Overlay, app: &mut App, text: &str) {
    for c in text.chars() {
        assert!(key(ov, app, &typed(c)) == After::Open, "typing {c:?} closed the overlay");
    }
}

#[test]
fn a5_every_action_is_classified() {
    let mut problems = Vec::new();
    for a in CATALOGUE {
        match (ANSWERS.contains(&a.name), ACTS.contains(&a.name)) {
            (false, false) => problems.push(format!("{}: in neither answer::ANSWERS nor answer::ACTS", a.name)),
            (true, true) => problems.push(format!("{}: in both answer::ANSWERS and answer::ACTS", a.name)),
            _ => {}
        }
    }
    for name in ANSWERS.iter().chain(ACTS) {
        if termoil_protocol::action(name).is_none() {
            problems.push(format!("{name}: listed in crate::answer but not a catalogue action"));
        }
    }
    assert!(problems.is_empty(), "A5 classification:\n{}", problems.join("\n"));
}

#[test]
fn a5_answer_overlay_keys() {
    let dir = scratch("keys");
    let (mut app, _) = new_app(&dir);
    let lines = (0..ANSWER_ROWS + 5).map(|i| format!("line {i}")).collect();
    let answer =
        Answer { window: None, action: "list".into(), title: "list".into(), lines, failed: false, pending: false };
    let mut ov = Overlay::Answer(AnswerUi::new(answer.clone()));
    let top = |ov: &Overlay| match ov {
        Overlay::Answer(a) => a.top,
        _ => usize::MAX,
    };
    assert!(key(&mut ov, &mut app, &named(NamedKey::Up)) == After::Open);
    assert_eq!(top(&ov), 0, "Up scrolled above the first line");
    for _ in 0..50 {
        assert!(key(&mut ov, &mut app, &named(NamedKey::Down)) == After::Open);
    }
    assert_eq!(top(&ov), 5, "Down did not stop with the last line at the bottom");
    assert!(key(&mut ov, &mut app, &named(NamedKey::PageUp)) == After::Open);
    assert_eq!(top(&ov), 0);
    assert!(key(&mut ov, &mut app, &named(NamedKey::Escape)) == After::Close, "Escape did not close the answer");
    for k in [named(NamedKey::Enter), typed('q')] {
        let mut ov = Overlay::Answer(AnswerUi::new(answer.clone()));
        assert!(key(&mut ov, &mut app, &k) == After::Close, "{:?} did not close the answer", k.key);
    }
    let colors = termoil_render::Colors::dark_default();
    let short = AnswerUi::new(Answer { lines: vec!["one".into()], ..answer.clone() });
    let (bytes, rows) = termoil_app::gui::overlay::answer_content(&short, 60, &colors);
    let text = String::from_utf8_lossy(&bytes);
    assert_eq!(rows, 3, "title, one line and the closing hint");
    assert!(text.contains(" list") && text.contains(" one") && text.contains("any key closes"), "{text}");
    let (_, rows) = termoil_app::gui::overlay::answer_content(&AnswerUi::new(answer), 60, &colors);
    assert_eq!(rows as usize, ANSWER_ROWS + 2, "a long answer is not cut to ANSWER_ROWS");
}

#[test]
fn a5_palette_asks_for_every_required_parameter() {
    let dir = scratch("ask");
    let (mut app, _) = new_app(&dir);
    let pane = open_window(&mut app);
    let mut ov = Overlay::Palette(PaletteUi::new(termoil_app::palette::entries(&[])));
    type_into(&mut ov, &mut app, "resize");
    let Overlay::Palette(p) = &mut ov else { unreachable!() };
    let at = p.visible().iter().position(|e| e.action == "resize").expect("the palette lists resize");
    p.selected = at;
    assert!(key(&mut ov, &mut app, &named(NamedKey::Enter)) == After::Open, "Enter ran resize without asking");
    assert!(prompt(&ov).contains("resize --cols"), "the palette asked {:?}", prompt(&ov));
    type_into(&mut ov, &mut app, "70");
    assert!(key(&mut ov, &mut app, &named(NamedKey::Enter)) == After::Open, "resize ran with --cols alone");
    assert!(prompt(&ov).contains("resize --rows"), "the palette asked {:?} second", prompt(&ov));
    type_into(&mut ov, &mut app, "20");
    assert!(key(&mut ov, &mut app, &named(NamedKey::Enter)) == After::Close, "resize did not run after --rows");
    let effects = app.take_effects();
    assert!(
        effects.iter().any(|e| matches!(e, Effect::ResizeWindow(_, p, 70, 20) if *p == pane)),
        "resize did not ask for 70 by 20"
    );
    assert!(!effects.iter().any(|e| matches!(e, Effect::ShowAnswer(_))), "resize showed an answer or a failure");
}

/// The palette's first line, the prompt.
fn prompt(ov: &Overlay) -> String {
    let Overlay::Palette(p) = ov else { return String::new() };
    let colors = termoil_render::Colors::dark_default();
    let (bytes, _) = termoil_app::gui::overlay::palette_content(p, 96, &colors);
    String::from_utf8_lossy(&bytes).lines().next().unwrap_or_default().to_string()
}

/// The arguments a person types when the palette asks for an action's required parameters, or gives
/// through a keybinding's args, keeping each action inside the test folder.
fn person_args(name: &str, dir: &std::path::Path) -> Value {
    match name {
        "send-text" => json!({ "text": "echo A5_SENT\r" }),
        "send-key" => json!({ "key": "escape" }),
        "resize" => json!({ "cols": 80, "rows": 24 }),
        "resize-split" => json!({ "direction": "right" }),
        "set-title" => json!({ "title": "a5 title" }),
        "set-theme" => json!({ "name": "Termoil Light" }),
        "set-shader" => json!({ "name": "none" }),
        "set-cursor-effect" => json!({ "name": "none" }),
        "activate-key-table" => json!({ "name": "a5", "once": true }),
        "run-action" => json!({ "name": "version" }),
        "wait-prompt" => json!({ "timeout": 1 }),
        "search" => json!({ "pattern": "A5_SENT" }),
        "install-shell-integration" => json!({ "shells": "bash", "dry-run": true }),
        "install-update" => json!({ "restart": false }),
        "skip-update" => json!({ "version": "97.0.0" }),
        "save-session" => json!({ "path": dir.join("a5-session.json").to_string_lossy() }),
        "check-for-updates" | "list-updates" => json!({ "channel": "nightly" }),
        _ => json!({}),
    }
}

/// The actions run from the palette that fail in this test's window, and why. Every other action must
/// succeed.
fn expected_failures() -> BTreeMap<&'static str, &'static str> {
    BTreeMap::from([
        ("wait-prompt", "the shell runs without integration, so no OSC 133 prompt mark arrives"),
        ("previous-prompt", "no OSC 133 prompt mark arrives without integration"),
        ("next-prompt", "no OSC 133 prompt mark arrives without integration"),
        ("select-last-output", "no command finished with OSC 133 marks"),
        ("copy", "nothing is selected when it runs"),
        ("open-config", "no editor is opened in a test; it queues OpenPath, checked below"),
    ])
}

/// Runs every catalogue action from the palette in a window with a split, in catalogue order with
/// `quit` last, and records what each one showed.
fn run_every_action(app: &mut App, received: &Received, dir: &std::path::Path) -> Vec<String> {
    let mut problems = Vec::new();
    let expected = expected_failures();
    for a in CATALOGUE.iter().filter(|a| a.name != "quit" && a.name != "close" && a.name != "close-tab") {
        if a.name == "open-config" {
            continue;
        }
        let now = from_palette(app, a.name, person_args(a.name, dir));
        let answer = match now.as_slice() {
            [one] if one.pending => job_answer(received, a.name),
            [one] => one.clone(),
            [] => {
                if ANSWERS.contains(&a.name) {
                    problems.push(format!("{}: answers information but showed nothing", a.name));
                }
                continue;
            }
            many => {
                problems.push(format!("{}: showed {} answers", a.name, many.len()));
                continue;
            }
        };
        if answer.lines.iter().chain([&answer.title]).any(|l| l.contains("unknown action")) {
            problems.push(format!("{}: did not reach run_action", a.name));
        }
        match (answer.failed, expected.get(a.name)) {
            (true, None) => problems.push(format!("{}: failed: {}", a.name, answer.lines.join(" "))),
            (false, Some(why)) => {
                problems.push(format!("{}: succeeded, but the test expects it to fail ({why})", a.name))
            }
            (false, None) if ACTS.contains(&a.name) => {
                problems.push(format!("{}: acts, but showed {answer:?}", a.name))
            }
            (false, None) if answer.lines.is_empty() && answer.title.is_empty() => {
                problems.push(format!("{}: showed an empty answer", a.name))
            }
            _ => {}
        }
    }
    problems
}

/// check-for-updates and list-updates from the window, on both channels, against the release server.
fn check_and_list(app: &mut App, received: &Received) {
    let stable = from_palette_later(app, received, "check-for-updates", json!({}));
    assert_eq!(stable.title, "Termoil is up to date", "{stable:?}");
    assert_eq!(stable.lines[0], format!("{VERSION} is the newest version on the stable channel."));
    let nightly = from_palette_later(app, received, "check-for-updates", json!({ "channel": "nightly" }));
    assert_eq!(nightly.title, format!("Termoil {NEWER} is available"), "{nightly:?}");
    assert!(nightly.lines[0].starts_with(&format!("You have {VERSION}.")), "{nightly:?}");
    assert!(nightly.lines.iter().any(|l| l.ends_with(&format!("/notes/{NEWER}"))), "no release notes: {nightly:?}");
    let both = from_palette_later(app, received, "list-updates", json!({}));
    assert_eq!(both.title, format!("Termoil versions (you have {VERSION})"));
    let want = [
        "stable:".to_string(),
        format!("  {VERSION:<12}2026-11-09  running"),
        String::new(),
        "nightly:".into(),
        format!("  {NEWER:<12}2026-11-09"),
        format!("  {BROKEN:<12}2026-11-09"),
    ];
    assert_eq!(both.lines, want);
    let one = from_palette_later(app, received, "list-updates", json!({ "channel": "nightly" }));
    assert_eq!(one.lines.first().map(String::as_str), Some("nightly:"), "{one:?}");
    assert!(!one.lines.iter().any(|l| l == "stable:"), "--channel nightly listed stable: {one:?}");
}

/// The report in task-2586: the palette opened, `check-for-updates` typed and Enter pressed. The palette
/// closes, the waiting answer is queued, and the job's answer arrives.
fn palette_keys_check(app: &mut App, received: &Received) {
    let mut ov = Overlay::Palette(PaletteUi::new(termoil_app::palette::entries(&[])));
    type_into(&mut ov, app, "check-for-updates");
    let Overlay::Palette(p) = &ov else { unreachable!() };
    assert_eq!(p.visible().first().map(|e| e.action), Some("check-for-updates"), "the palette's first match");
    assert!(key(&mut ov, app, &named(NamedKey::Enter)) == After::Close, "Enter did not run check-for-updates");
    let now = shown(app);
    assert!(now.len() == 1 && now[0].pending && now[0].title == "Checking for updates", "Enter queued {now:?}");
    let answer = job_answer(received, "check-for-updates");
    assert_eq!(answer.title, "Termoil is up to date", "{answer:?}");
    let mut ov = Overlay::Palette(PaletteUi::new(termoil_app::palette::entries(&[])));
    type_into(&mut ov, app, "list-updates");
    assert!(key(&mut ov, app, &named(NamedKey::Enter)) == After::Close, "Enter did not run list-updates");
    assert!(shown(app).iter().any(|a| a.pending), "list-updates queued no waiting answer");
    let listed = job_answer(received, "list-updates");
    assert!(listed.lines.iter().any(|l| l == "nightly:"), "{listed:?}");
}

/// install-update and skip-update from the window: a package downloaded and checked, a wrong SHA-256
/// and an unknown version shown as failures, and a skip of the version the last check found.
fn install_and_skip(app: &mut App, received: &Received) {
    let got = from_palette_later(app, received, "install-update", json!({ "channel": "nightly", "restart": false }));
    assert_eq!(got.title, format!("Termoil {NEWER} was downloaded"), "{got:?}");
    let file = got.lines[0].rsplit(": ").next().unwrap().to_string();
    assert_eq!(std::fs::read(&file).unwrap(), PACKAGE, "the downloaded package differs");
    let _ = std::fs::remove_dir_all(PathBuf::from(&file).parent().unwrap());
    let bad = from_palette_later(app, received, "install-update", json!({ "version": BROKEN, "restart": false }));
    assert!(bad.failed && bad.lines.join(" ").contains("SHA-256"), "a wrong SHA-256 was not shown: {bad:?}");
    let missing = from_palette_later(app, received, "install-update", json!({ "version": "42.0.0", "restart": false }));
    assert!(missing.failed && missing.lines.join(" ").contains("is not on the"), "{missing:?}");
    let skipped = from_palette(app, "skip-update", json!({}));
    assert_eq!(skipped.len(), 1);
    assert_eq!(skipped[0].title, format!("Termoil {NEWER} will not be announced"), "{skipped:?}");
    let text = std::fs::read_to_string(&app.config_path).unwrap();
    assert!(text.contains(&format!("skip = \"{NEWER}\"")), "updates.skip was not written:\n{text}");
}

/// A channel the server does not have is shown as a failure, and so is skip-update before any check.
fn update_failures(base: &str) {
    point_releases(base, "missing-stable.json", "missing-nightly.json");
    let dir = scratch("fail");
    let (mut app, received) = new_app(&dir);
    let fresh = from_palette(&mut app, "skip-update", json!({}));
    assert!(
        fresh.len() == 1 && fresh[0].failed && fresh[0].lines[0].contains("no check has found a version yet"),
        "skip-update before a check: {fresh:?}"
    );
    let check = from_palette_later(&mut app, &received, "check-for-updates", json!({}));
    assert!(check.failed && check.title == "check-for-updates failed", "{check:?}");
    let list = from_palette_later(&mut app, &received, "list-updates", json!({}));
    assert!(!list.failed, "list-updates fails only per channel: {list:?}");
    assert!(list.lines.iter().filter(|l| l.starts_with("  could not be read")).count() == 2, "{list:?}");
    let _ = std::fs::remove_dir_all(&dir);
}

/// Errors are shown from the palette and kept quiet from a key, and a result that is information is
/// shown from both.
fn errors_and_quiet_keys(app: &mut App) {
    let bad = from_palette(app, "set-theme", json!({ "name": "No Such Theme A5" }));
    assert!(bad.len() == 1 && bad[0].failed && bad[0].title == "set-theme failed", "{bad:?}");
    let missing = from_palette(app, "focus", json!({ "pane": 999_999 }));
    assert!(missing.len() == 1 && missing[0].failed && missing[0].lines[0].contains("999999"), "{missing:?}");
    let call = |name: &str, args: Value| ActionCall { action: name.into(), args: args.as_object().cloned().unwrap() };
    let _ = app.run_for_person(&call("focus", json!({ "pane": 999_999 })), Asked::Binding);
    assert!(shown(app).is_empty(), "a key whose action failed showed an answer");
    let _ = app.run_for_person(&call("version", json!({})), Asked::Binding);
    let v = shown(app);
    assert!(v.len() == 1 && v[0].title == format!("Termoil {VERSION}"), "version from a key: {v:?}");
    assert!(app.run_named("list").is_ok());
    assert_eq!(shown(app).len(), 1, "run_named did not show the answer of list");
}

#[test]
fn a5_every_action_from_the_window() {
    let base = release_server();
    point_releases(&base, "stable.json", "nightly.json");
    let dir = scratch("all");
    let (mut app, received) = new_app(&dir);
    let first = open_window(&mut app);
    termoil_app::actions::focus_pane(&mut app, first).unwrap();
    let _ = from_palette(&mut app, "split-right", json!({}));
    palette_keys_check(&mut app, &received);
    check_and_list(&mut app, &received);
    install_and_skip(&mut app, &received);
    errors_and_quiet_keys(&mut app);
    let problems = run_every_action(&mut app, &received, &dir);
    assert!(problems.is_empty(), "A5, every action from the palette:\n{}", problems.join("\n"));
    let _ = app.run_for_person(&ActionCall { action: "open-config".into(), args: Map::new() }, Asked::Palette);
    let effects = app.take_effects();
    assert!(
        effects.iter().any(|e| matches!(e, Effect::OpenPath(p) if *p == app.config_path))
            || effects.iter().any(|e| matches!(e, Effect::ShowAnswer(a) if a.failed)),
        "open-config neither opened the file nor said why"
    );
    for name in ["close", "close-tab", "quit"] {
        let answers = from_palette(&mut app, name, json!({}));
        assert!(answers.is_empty(), "{name} showed {answers:?}");
    }
    update_failures(&base);
    for id in app.panes.keys().copied().collect::<Vec<_>>() {
        app.close_pane(id);
    }
    let _ = std::fs::remove_dir_all(&dir);
}
