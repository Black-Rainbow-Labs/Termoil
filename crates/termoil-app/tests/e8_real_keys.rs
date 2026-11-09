//! E8: real keystrokes from the operating system into a real Termoil window (task-2589,
//! `docs/decisions/keyboard-shortcuts.md`, test K5).
//!
//! E6 and E7 enter at the `RawKey` step, because winit's key events cannot be built outside winit.
//! This test covers the step before that: it starts the built `termoil-gui` with its own socket and
//! configuration, waits for its window to be in front, and sends keys through the operating system
//! (`SendInput` on Windows, System Events through `osascript` on macOS, which needs the Accessibility
//! permission for the program running the test). It reads the pane over IPC: the font size keys
//! change the column count, and the prompt line shows nothing a shortcut sent.
//!
//! It takes the foreground of the desktop it runs on, so it runs only with `TERMOIL_REAL_KEYS=1`.
//! Before each key it checks that the Termoil window is still in front and stops if not, so a key is
//! never sent to another program's window.

use std::process::{Child, Command};
use std::time::{Duration, Instant};

use serde_json::{Map, Value, json};
use termoil_protocol::Client;

/// A key the test presses: the modifiers held and the key, by name.
#[derive(Clone, Copy, Debug)]
struct Press {
    ctrl_or_cmd: bool,
    shift: bool,
    key: char,
}

/// The platform modifier and `key`, with Shift when `shift`.
fn chord(key: char, shift: bool) -> Press {
    Press { ctrl_or_cmd: true, shift, key }
}

/// A started `termoil-gui` with its own socket, configuration and data folder.
struct Instance {
    child: Child,
    socket: String,
}

impl Drop for Instance {
    /// Stops the instance this test started.
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

impl Instance {
    /// Starts `termoil-gui` in a window with the default shell and waits for its socket.
    fn start(dir: &std::path::Path) -> Instance {
        std::fs::create_dir_all(dir).unwrap();
        let config = dir.join("config.toml");
        std::fs::write(&config, "[updates]\ncheck = \"off\"\n").unwrap();
        let socket = if cfg!(windows) {
            format!(r"\\.\pipe\termoil-e8-{}", std::process::id())
        } else {
            dir.join("e8.sock").to_string_lossy().into_owned()
        };
        let child = Command::new(env!("CARGO_BIN_EXE_termoil-gui"))
            .env("TERMOIL_SOCKET", &socket)
            .env("TERMOIL_CONFIG", &config)
            .env("TERMOIL_DATA_DIR", dir.join("data"))
            .env("TERMOIL_TELEMETRY", "off")
            .spawn()
            .expect("start termoil-gui");
        let inst = Instance { child, socket };
        wait_for(Duration::from_secs(20), "the instance to answer", || inst.call("list", json!({})).ok());
        inst
    }

    /// Runs one action against the instance.
    fn call(&self, action: &str, args: Value) -> Result<Value, String> {
        let mut client = Client::connect_to(&self.socket, Duration::from_secs(2)).map_err(|e| e.to_string())?;
        let args: Map<String, Value> = args.as_object().cloned().unwrap_or_default();
        let response = client.call(action, args).map_err(|e| e.to_string())?;
        if response.ok { Ok(response.result) } else { Err(response.error.unwrap_or_default()) }
    }

    /// The first pane's id and its column count.
    fn first_pane(&self) -> Option<(u64, u64)> {
        let list = self.call("list", json!({})).ok()?;
        let pane = find_first_pane(&list)?;
        Some((pane["id"].as_u64()?, pane["cols"].as_u64()?))
    }

    /// The first pane's screen text.
    fn screen(&self, pane: u64) -> String {
        let reply = self.call("screen", json!({ "pane": pane })).unwrap_or_default();
        reply.as_str().unwrap_or_default().to_string()
    }
}

/// The first pane object anywhere in a `list` reply.
fn find_first_pane(v: &Value) -> Option<&Value> {
    match v {
        Value::Object(o) if o.contains_key("cols") && o.contains_key("id") => Some(v),
        Value::Object(o) => o.values().find_map(find_first_pane),
        Value::Array(a) => a.iter().find_map(find_first_pane),
        _ => None,
    }
}

/// Polls `f` every 50 ms until it returns a value, failing after `limit`.
fn wait_for<T>(limit: Duration, what: &str, mut f: impl FnMut() -> Option<T>) -> T {
    let deadline = Instant::now() + limit;
    loop {
        if let Some(v) = f() {
            return v;
        }
        assert!(Instant::now() < deadline, "timed out waiting for {what}");
        std::thread::sleep(Duration::from_millis(50));
    }
}

/// Waits until the column count is `want(cols)` and returns it.
fn wait_cols(inst: &Instance, what: &str, want: impl Fn(u64) -> bool) -> u64 {
    wait_for(Duration::from_secs(5), what, || inst.first_pane().map(|(_, c)| c).filter(|c| want(*c)))
}

/// The prompt line: the last line with anything on it.
fn prompt_line(screen: &str) -> String {
    screen.lines().rev().find(|l| !l.trim().is_empty()).unwrap_or("").to_string()
}

#[test]
fn e8_real_keystrokes_reach_the_bindings_and_not_the_shell() {
    if std::env::var("TERMOIL_REAL_KEYS").as_deref() != Ok("1") {
        return println!("E8 sends real keystrokes and takes the foreground; set TERMOIL_REAL_KEYS=1 to run it");
    }
    if !cfg!(any(windows, target_os = "macos")) {
        return println!("E8 has a keystroke source for Windows and macOS only; skipping");
    }
    let dir = std::env::temp_dir().join(format!("termoil-e8-{}", std::process::id()));
    let inst = Instance::start(&dir);
    let pid = inst.child.id();
    let _previous = os::wait_until_in_front(pid);
    let (pane, start) = wait_for(Duration::from_secs(20), "a pane", || inst.first_pane());
    wait_for(Duration::from_secs(20), "the prompt", || {
        let line = prompt_line(&inst.screen(pane));
        line.trim_end().ends_with(['>', '$', '%', '#']).then_some(())
    });
    let before = prompt_line(&inst.screen(pane));
    println!("E8: the prompt is {before:?} at {start} columns");

    os::press(pid, chord('=', false));
    let bigger = wait_cols(&inst, "the platform modifier and = to make the font larger", |c| c < start);
    os::press(pid, chord('=', true));
    let biggest = wait_cols(&inst, "the platform modifier, Shift and = to make the font larger", |c| c < bigger);
    os::press(pid, chord('-', false));
    wait_cols(&inst, "the platform modifier and - to make the font smaller", |c| c > biggest);
    os::press(pid, chord('0', false));
    wait_cols(&inst, "the platform modifier and 0 to reset the font", |c| c == start);
    println!("E8: columns {start} -> {bigger} -> {biggest} -> back to {start}");

    std::thread::sleep(Duration::from_millis(300));
    let after = prompt_line(&inst.screen(pane));
    assert_eq!(after, before, "a shortcut reached the shell; the prompt line changed");
    os::type_text(pid, "echo e8typed\n");
    wait_for(Duration::from_secs(10), "typed text to run", || {
        inst.screen(pane).lines().any(|l| l.trim() == "e8typed").then_some(())
    });
    drop(inst);
    let _ = std::fs::remove_dir_all(&dir);
}

#[cfg(windows)]
mod os {
    //! Keystrokes through `SendInput`, sent only while the Termoil window is in front.

    use super::Press;
    use std::time::{Duration, Instant};
    use windows::Win32::Foundation::{HWND, LPARAM};
    use windows::Win32::System::Threading::{AttachThreadInput, GetCurrentThreadId};
    use windows::Win32::UI::Input::KeyboardAndMouse::{
        INPUT, INPUT_0, INPUT_KEYBOARD, KEYBD_EVENT_FLAGS, KEYBDINPUT, KEYEVENTF_KEYUP, MAPVK_VK_TO_VSC,
        MapVirtualKeyW, SendInput, VIRTUAL_KEY, VK_CONTROL, VK_OEM_MINUS, VK_OEM_PLUS, VK_RETURN, VK_SHIFT, VK_SPACE,
    };
    use windows::Win32::UI::WindowsAndMessaging::{
        BringWindowToTop, EnumWindows, GetForegroundWindow, GetWindowThreadProcessId, IsWindowVisible,
        SetForegroundWindow,
    };
    use windows::core::BOOL;

    /// The process that owns the window in front.
    fn foreground_pid() -> u32 {
        let mut pid = 0u32;
        // SAFETY: GetForegroundWindow has no preconditions; the out pointer is a live local.
        unsafe { GetWindowThreadProcessId(GetForegroundWindow(), Some(&mut pid)) };
        pid
    }

    /// The search `EnumWindows` fills in: a process id and its visible window.
    struct Search {
        pid: u32,
        found: Option<HWND>,
    }

    /// `EnumWindows` callback: stops at the first visible window of the process.
    unsafe extern "system" fn each_window(h: HWND, l: LPARAM) -> BOOL {
        // SAFETY: `l` is the address of the `Search` that `window_of` passed, alive for the call.
        let search = unsafe { &mut *(l.0 as *mut Search) };
        let mut pid = 0u32;
        // SAFETY: `h` is a window handle EnumWindows gave us.
        unsafe { GetWindowThreadProcessId(h, Some(&mut pid)) };
        // SAFETY: as above.
        if pid == search.pid && unsafe { IsWindowVisible(h) }.as_bool() {
            search.found = Some(h);
            return BOOL(0);
        }
        BOOL(1)
    }

    /// The visible window of a process, when it has one.
    fn window_of(pid: u32) -> Option<HWND> {
        let mut search = Search { pid, found: None };
        // SAFETY: the callback only reads and writes `search` during the call. EnumWindows reports
        // an error when the callback stops early, which is the found case.
        let _ = unsafe { EnumWindows(Some(each_window), LPARAM(&mut search as *mut Search as isize)) };
        search.found
    }

    /// Moves `h` to the front. Windows lets a process do that only while its input is attached to
    /// the thread that owns the window in front, so this attaches for the call and detaches. No key
    /// is sent to the window that was in front.
    fn move_to_front(h: HWND) {
        // SAFETY: plain Win32 calls on handles and thread ids that are alive for the call.
        unsafe {
            let front_thread = GetWindowThreadProcessId(GetForegroundWindow(), None);
            let me = GetCurrentThreadId();
            let attached = front_thread != me && AttachThreadInput(me, front_thread, true).as_bool();
            let _ = SetForegroundWindow(h);
            let _ = BringWindowToTop(h);
            if attached {
                let _ = AttachThreadInput(me, front_thread, false);
            }
        }
    }

    /// The window that was in front before the test; it is put back in front when dropped, so the
    /// program that had the desktop gets it back even when the test fails.
    pub struct Previous(usize);

    impl Drop for Previous {
        /// Gives the foreground back.
        fn drop(&mut self) {
            if self.0 != 0 {
                move_to_front(HWND(self.0 as *mut core::ffi::c_void));
            }
        }
    }

    /// Waits for the instance's window, moves it to the front and waits until it is there.
    pub fn wait_until_in_front(pid: u32) -> Previous {
        // SAFETY: no preconditions.
        let previous = Previous(unsafe { GetForegroundWindow() }.0 as usize);
        let deadline = Instant::now() + Duration::from_secs(15);
        let window = loop {
            if let Some(h) = window_of(pid) {
                break h;
            }
            assert!(Instant::now() < deadline, "termoil-gui never showed a window");
            std::thread::sleep(Duration::from_millis(50));
        };
        while foreground_pid() != pid {
            assert!(
                Instant::now() < deadline,
                "the Termoil window never came to the front (pid {} is in front)",
                foreground_pid()
            );
            move_to_front(window);
            std::thread::sleep(Duration::from_millis(100));
        }
        previous
    }

    /// One key event for `vk`, with its scan code, down or up.
    fn key(vk: VIRTUAL_KEY, up: bool) -> INPUT {
        // SAFETY: MapVirtualKeyW has no preconditions.
        let scan = unsafe { MapVirtualKeyW(u32::from(vk.0), MAPVK_VK_TO_VSC) } as u16;
        let flags = if up { KEYEVENTF_KEYUP } else { KEYBD_EVENT_FLAGS(0) };
        INPUT {
            r#type: INPUT_KEYBOARD,
            Anonymous: INPUT_0 { ki: KEYBDINPUT { wVk: vk, wScan: scan, dwFlags: flags, time: 0, dwExtraInfo: 0 } },
        }
    }

    /// The virtual key of a character on a US layout, and whether it needs Shift.
    fn vk_of(c: char) -> (VIRTUAL_KEY, bool) {
        match c {
            '=' => (VK_OEM_PLUS, false),
            '-' => (VK_OEM_MINUS, false),
            ' ' => (VK_SPACE, false),
            '\n' => (VK_RETURN, false),
            'a'..='z' => (VIRTUAL_KEY(c.to_ascii_uppercase() as u16), false),
            '0'..='9' => (VIRTUAL_KEY(c as u16), false),
            other => panic!("E8 has no virtual key for {other:?}"),
        }
    }

    /// Sends keys down in order and up in reverse, after checking the instance is still in front.
    fn send(pid: u32, keys: &[VIRTUAL_KEY]) {
        assert_eq!(foreground_pid(), pid, "another window took the foreground; stopping before sending a key");
        let mut inputs: Vec<INPUT> = keys.iter().map(|k| key(*k, false)).collect();
        inputs.extend(keys.iter().rev().map(|k| key(*k, true)));
        // SAFETY: the slice holds initialised INPUT values and the size is the size of one.
        let sent = unsafe { SendInput(&inputs, std::mem::size_of::<INPUT>() as i32) };
        assert_eq!(sent as usize, inputs.len(), "SendInput was blocked");
        std::thread::sleep(Duration::from_millis(80));
    }

    /// Presses a chord with Ctrl, the platform modifier on Windows.
    pub fn press(pid: u32, p: Press) {
        let (vk, _) = vk_of(p.key);
        let mut keys = Vec::new();
        if p.ctrl_or_cmd {
            keys.push(VK_CONTROL);
        }
        if p.shift {
            keys.push(VK_SHIFT);
        }
        keys.push(vk);
        send(pid, &keys);
    }

    /// Types text one key at a time.
    pub fn type_text(pid: u32, text: &str) {
        for c in text.chars() {
            send(pid, &[vk_of(c).0]);
        }
    }
}

#[cfg(target_os = "macos")]
mod os {
    //! Keystrokes through System Events, sent only while the Termoil window is in front.

    use super::Press;
    use std::process::Command;
    use std::time::{Duration, Instant};

    /// Runs an AppleScript and returns what it printed.
    fn osascript(script: &str) -> String {
        let out = Command::new("osascript").arg("-e").arg(script).output().expect("run osascript");
        assert!(out.status.success(), "osascript failed: {}", String::from_utf8_lossy(&out.stderr));
        String::from_utf8_lossy(&out.stdout).trim().to_string()
    }

    /// The process id of the frontmost application.
    fn foreground_pid() -> u32 {
        let script = "tell application \"System Events\" to get unix id of first process whose frontmost is true";
        osascript(script).parse().unwrap_or(0)
    }

    /// Nothing to give back on macOS: the person's application comes back with Cmd and Tab.
    pub struct Previous;

    /// Brings the instance to the front and waits up to 15 seconds for it to be there.
    pub fn wait_until_in_front(pid: u32) -> Previous {
        let _ = osascript(&format!(
            "tell application \"System Events\" to set frontmost of (first process whose unix id is {pid}) to true"
        ));
        let deadline = Instant::now() + Duration::from_secs(15);
        while foreground_pid() != pid {
            assert!(Instant::now() < deadline, "the Termoil window never came to the front");
            std::thread::sleep(Duration::from_millis(100));
        }
        Previous
    }

    /// Sends one keystroke with modifiers, after checking the instance is still in front.
    fn keystroke(pid: u32, key: &str, modifiers: &[&str]) {
        assert_eq!(foreground_pid(), pid, "another window took the foreground; stopping before sending a key");
        let using = if modifiers.is_empty() {
            String::new()
        } else {
            format!(" using {{{}}}", modifiers.iter().map(|m| format!("{m} down")).collect::<Vec<_>>().join(", "))
        };
        let _ = osascript(&format!("tell application \"System Events\" to keystroke \"{key}\"{using}"));
        std::thread::sleep(Duration::from_millis(80));
    }

    /// Presses a chord with Cmd, the platform modifier on macOS.
    pub fn press(pid: u32, p: Press) {
        let mut mods = Vec::new();
        if p.ctrl_or_cmd {
            mods.push("command");
        }
        if p.shift {
            mods.push("shift");
        }
        keystroke(pid, &p.key.to_string(), &mods);
    }

    /// Types text one key at a time; a newline is Return.
    pub fn type_text(pid: u32, text: &str) {
        for c in text.chars() {
            if c == '\n' {
                assert_eq!(foreground_pid(), pid, "another window took the foreground");
                let _ = osascript("tell application \"System Events\" to key code 36");
            } else {
                keystroke(pid, &c.to_string(), &[]);
            }
        }
    }
}

#[cfg(not(any(windows, target_os = "macos")))]
mod os {
    //! No keystroke source on this platform; the test returns before it reaches these.

    use super::Press;

    /// Nothing to give back.
    pub struct Previous;

    /// Never called.
    pub fn wait_until_in_front(_pid: u32) -> Previous {
        Previous
    }

    /// Never called.
    pub fn press(_pid: u32, _p: Press) {}

    /// Never called.
    pub fn type_text(_pid: u32, _text: &str) {}
}
