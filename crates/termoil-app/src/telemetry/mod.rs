//! Anonymous usage statistics, sent to the `Termoil App` property in Google Analytics
//! (`docs/decisions/telemetry.md`, task-2530).
//!
//! It is on unless `telemetry.enabled` is false. What it sends is a closed list of names (catalogue action
//! names, shell kinds) and a few counts and durations. It never sends a path, a file name, a command, a
//! window title, anything typed or anything a program printed.
//!
//! - [`note`] counts one use of something, from any thread. Off, it is one relaxed atomic load.
//! - [`activity`] is the clock: open, focused and active time, driven by input and focus events because
//!   Termoil's event loop has no frame to count on.
//! - [`payload`] turns counts into GA4 Measurement Protocol bodies, and is where Google's limits live.
//! - [`sender`] is the thread that decides when to send, posts through curl, and keeps the spool.

pub mod activity;
pub mod payload;
pub mod sender;

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use activity::Activity;

/// The `Termoil App` web stream's measurement id.
pub const MEASUREMENT_ID: &str = "G-NDVVCGE4FD";
/// The stream's Measurement Protocol API secret, nicknamed `Termoil desktop app` in GA4's admin page. A
/// Measurement Protocol secret lets its holder send events to this one stream and read nothing; it is
/// rotated in the admin page if it is ever abused.
pub const API_SECRET: &str = "YjOL_kuHS0SLjcrJMp2Oig";
/// Where a request goes. `TERMOIL_TELEMETRY_URL` replaces it for a test's scripted server.
pub const ENDPOINT: &str = "https://www.google-analytics.com/mp/collect";
/// How long after start the first request goes, so it does not compete with the first window.
pub const FIRST_AFTER: Duration = Duration::from_secs(10);
/// The most requests one process sends in its life, so a fault cannot become a flood.
pub const MOST_REQUESTS: u32 = 200;

/// What kind of thing was used: the `category` parameter of a `feature` event.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Category {
    /// A catalogue action, from a key, the palette, the mouse, the settings view, the CLI or IPC.
    Action,
    /// The subset of actions that arrived over the socket: `termoil +command`, a script or an agent.
    Ipc,
    /// A session started, by the kind of shell.
    Shell,
    /// The configuration was reloaded.
    Config,
}

impl Category {
    /// The name GA4 receives.
    pub fn name(self) -> &'static str {
        match self {
            Category::Action => "action",
            Category::Ipc => "ipc",
            Category::Shell => "shell",
            Category::Config => "config",
        }
    }
}

/// The largest layout seen at once during the process, sent in `app_close`.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Peaks {
    pub windows: usize,
    pub tabs: usize,
    pub panes: usize,
}

/// Whether anything is being counted, in this process.
static ON: AtomicBool = AtomicBool::new(false);
/// Every use counted since the last request, by category and name.
static TALLY: Mutex<BTreeMap<(Category, String), u32>> = Mutex::new(BTreeMap::new());
/// The clock, made when telemetry starts.
static ACTIVITY: Mutex<Option<Activity>> = Mutex::new(None);
/// The largest layout seen.
static PEAKS: Mutex<Peaks> = Mutex::new(Peaks { windows: 0, tabs: 0, panes: 0 });

/// Whether telemetry is counting in this process.
pub fn is_on() -> bool {
    ON.load(Ordering::Relaxed)
}

/// Starts or stops counting. Stopping throws away what was counted and not sent, because a person who
/// turned the setting off did not agree to that being sent either.
pub(crate) fn set_on(on: bool) {
    ON.store(on, Ordering::Relaxed);
    if !on {
        if let Ok(mut tally) = TALLY.lock() {
            tally.clear();
        }
        if let Ok(mut clock) = ACTIVITY.lock()
            && let Some(clock) = clock.as_mut()
        {
            clock.forget_engagement(Instant::now());
        }
    }
}

/// Counts one use of `name`. Nothing at all while telemetry is off.
///
/// @param category - what kind of thing it is
/// @param name - its name from one of the closed lists; [`clean_name`] drops anything else
pub fn note(category: Category, name: &str) {
    if !is_on() {
        return;
    }
    let name = clean_name(name);
    if name.is_empty() {
        return;
    }
    if let Ok(mut tally) = TALLY.lock() {
        *tally.entry((category, name)).or_insert(0) += 1;
    }
}

/// Counts a catalogue action, only when the catalogue has it.
///
/// @param category - `Action` or `Ipc`
/// @param name - the action's name
pub fn note_action(category: Category, name: &str) {
    if is_on() && termoil_protocol::action(name).is_some() {
        note(category, name);
    }
}

/// Counts a session started with `program`, by its shell kind; any other program is `other`.
///
/// @param program - the program the session runs
pub fn note_shell(program: &str) {
    if is_on() {
        note(Category::Shell, shell_name(crate::shell_integration::detect(program)));
    }
}

/// The name a shell kind is sent as.
///
/// @param shell - the kind `shell_integration::detect` found
pub fn shell_name(shell: crate::shell_integration::Shell) -> &'static str {
    use crate::shell_integration::Shell;
    match shell {
        Shell::PowerShell => "powershell",
        Shell::Pwsh => "pwsh",
        Shell::Bash => "bash",
        Shell::Zsh => "zsh",
        Shell::Fish => "fish",
        Shell::Nushell => "nushell",
        Shell::Cmd => "cmd",
        Shell::Unknown => "other",
    }
}

/// Records the layout after an action, keeping the largest seen.
///
/// @param windows - windows open now
/// @param tabs - tabs open now, across every window
/// @param panes - panes open now
pub fn note_layout(windows: usize, tabs: usize, panes: usize) {
    if !is_on() {
        return;
    }
    if let Ok(mut peaks) = PEAKS.lock() {
        peaks.windows = peaks.windows.max(windows);
        peaks.tabs = peaks.tabs.max(tabs);
        peaks.panes = peaks.panes.max(panes);
    }
}

/// Records a key, mouse button, wheel or IME event in a window.
pub fn input() {
    if is_on()
        && let Ok(mut clock) = ACTIVITY.lock()
        && let Some(clock) = clock.as_mut()
    {
        clock.input(Instant::now());
    }
}

/// Records a window gaining or losing the operating system's focus.
///
/// @param focused - whether a Termoil window has the focus now
pub fn focus(focused: bool) {
    if let Ok(mut clock) = ACTIVITY.lock()
        && let Some(clock) = clock.as_mut()
    {
        clock.focus(focused, Instant::now());
    }
}

/// Runs `f` on the clock, when telemetry was started.
pub(crate) fn with_activity<T>(f: impl FnOnce(&mut Activity) -> T) -> Option<T> {
    ACTIVITY.lock().ok()?.as_mut().map(f)
}

/// The largest layout seen so far.
pub(crate) fn peaks() -> Peaks {
    PEAKS.lock().map(|p| *p).unwrap_or_default()
}

/// Takes everything counted since the last request.
pub(crate) fn take_tally() -> BTreeMap<(Category, String), u32> {
    TALLY.lock().map(|mut tally| std::mem::take(&mut *tally)).unwrap_or_default()
}

/// Puts back what a request had no room for, so the next one carries it.
///
/// @param rows - the rows to add back
pub(crate) fn put_back(rows: BTreeMap<(Category, String), u32>) {
    if let Ok(mut tally) = TALLY.lock() {
        for (key, count) in rows {
            *tally.entry(key).or_insert(0) += count;
        }
    }
}

/// How many distinct names are waiting to be sent.
pub(crate) fn waiting() -> usize {
    TALLY.lock().map(|tally| tally.len()).unwrap_or(0)
}

/// A name safe to send: itself when it is a plain identifier, cut to GA4's 100 characters, and nothing
/// when it holds any other character. Every name counted comes from a list compiled into Termoil, so this
/// is the second line: a character outside the set means something unexpected reached it.
///
/// @param name - the name as counted
pub fn clean_name(name: &str) -> String {
    let plain = name.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | ':' | '.' | '+'));
    if plain { name.chars().take(100).collect() } else { String::new() }
}

/// Why telemetry is not running, or that it may.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Permission {
    /// It may run while the setting is on.
    Allowed,
    /// `TERMOIL_TELEMETRY=off`.
    Environment,
    /// A `--headless` instance, which a script or a test is driving.
    Headless,
    /// A latency benchmark run.
    Benchmark,
}

impl Permission {
    /// The binary's answer: `off` in the environment wins, `on` or `debug` allows a headless instance or a
    /// benchmark, and otherwise those two send nothing.
    ///
    /// @param headless - whether this is a `--headless` instance
    /// @param benchmark - whether this is a latency benchmark run
    /// @param environment - `TERMOIL_TELEMETRY`, when it is set
    pub fn for_the_binary(headless: bool, benchmark: bool, environment: Option<&str>) -> Permission {
        match environment.map(str::trim).map(str::to_ascii_lowercase).as_deref() {
            Some("off" | "0" | "false" | "no") => Permission::Environment,
            Some("on" | "1" | "true" | "yes" | "debug") => Permission::Allowed,
            _ if benchmark => Permission::Benchmark,
            _ if headless => Permission::Headless,
            _ => Permission::Allowed,
        }
    }

    /// What `telemetry-status` calls it.
    pub fn name(self) -> &'static str {
        match self {
            Permission::Allowed => "allowed",
            Permission::Environment => "environment",
            Permission::Headless => "headless",
            Permission::Benchmark => "benchmark",
        }
    }
}

/// `TERMOIL_TELEMETRY`, when it is set.
pub fn environment() -> Option<String> {
    std::env::var("TERMOIL_TELEMETRY").ok()
}

/// Whether `TERMOIL_TELEMETRY=debug` asked for GA4's DebugView and a copy of every body on disk.
pub fn debug_from_the_environment() -> bool {
    environment().is_some_and(|v| v.trim().eq_ignore_ascii_case("debug"))
}

/// The telemetry folder in the data folder.
pub fn folder() -> Option<PathBuf> {
    crate::paths::data_dir().map(|d| d.join("telemetry"))
}

/// This installation's client id, made the first time and kept in `id.txt`, and whether it was just made.
/// 32 hexadecimal characters from the operating system's random source; nothing about the machine or the
/// person goes into it, and deleting the file makes a new one.
///
/// @param folder - the telemetry folder
pub fn client_id(folder: &Path) -> (String, bool) {
    let file = folder.join("id.txt");
    if let Ok(text) = std::fs::read_to_string(&file) {
        let id = text.trim();
        if id.len() >= 16 && id.chars().all(|c| c.is_ascii_hexdigit()) {
            return (id.to_owned(), false);
        }
    }
    let mut bytes = [0u8; 16];
    if getrandom::fill(&mut bytes).is_err() {
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|since| since.as_nanos())
            .unwrap_or_default();
        bytes[..8].copy_from_slice(&(nanos as u64).to_le_bytes());
        bytes[8..12].copy_from_slice(&std::process::id().to_le_bytes());
    }
    let id: String = bytes.iter().map(|byte| format!("{byte:02x}")).collect();
    let _ = std::fs::create_dir_all(folder);
    let _ = std::fs::write(&file, id.as_bytes());
    (id, true)
}

/// Whether the crash folder holds more reports than at the last start, and remembers the count.
///
/// @param crashes - the folder crash reports are written to
/// @param folder - the telemetry folder, where the count is kept
pub fn crashed_since_last_start(crashes: &Path, folder: &Path) -> bool {
    let now = std::fs::read_dir(crashes).map(|d| d.flatten().count()).unwrap_or(0);
    let file = folder.join("crashes.txt");
    let before = std::fs::read_to_string(&file).ok().and_then(|t| t.trim().parse::<usize>().ok());
    let _ = std::fs::create_dir_all(folder);
    let _ = std::fs::write(&file, now.to_string());
    before.is_some_and(|before| now > before)
}

/// What `app_open` says about this process.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Opening {
    /// `plain`, `folder` (`--cwd`) or `command` (`--command`).
    pub launch: &'static str,
    /// `auto`, `gpu` or `cpu`, as configured.
    pub renderer: &'static str,
    /// The theme's name when Termoil bundles it, `other` otherwise.
    pub theme: String,
    /// `stable` or `nightly`.
    pub channel: &'static str,
    pub first_run: bool,
    pub crashed: bool,
}

/// The `launch` value for the arguments a window was started with.
///
/// @param cwd - whether `--cwd` was given
/// @param command - whether `--command` was given
pub fn launch_kind(cwd: bool, command: bool) -> &'static str {
    match (cwd, command) {
        (_, true) => "command",
        (true, false) => "folder",
        (false, false) => "plain",
    }
}

/// The theme name to send: a bundled theme's name in lower case with spaces as hyphens (`unluminous-dark`),
/// and `other` for any theme Termoil does not bundle. Bundled names have spaces, which [`clean_name`] would
/// otherwise drop.
///
/// @param name - `theme.name` from the configuration
pub fn theme_name(name: &str) -> String {
    let bundled = crate::config::themes().iter().any(|t| t.name == name);
    let slug = clean_name(&name.to_ascii_lowercase().replace(' ', "-"));
    if bundled && !slug.is_empty() { slug } else { "other".into() }
}

/// Starts telemetry for this process: the clock, the switch and the sender thread. Does nothing when
/// `permission` says the process must not send.
///
/// @param config - the configuration in effect
/// @param permission - whether this process may send at all
/// @param launch - the `launch` value for `app_open`
pub fn start(config: &crate::config::Config, permission: Permission, launch: &'static str) {
    if permission != Permission::Allowed {
        sender::refuse(permission);
        return;
    }
    let Some(folder) = folder() else { return };
    let (client, first_run) = client_id(&folder);
    let crashed = crate::paths::data_dir().is_some_and(|d| crashed_since_last_start(&d.join("crashes"), &folder));
    let opening = Opening {
        launch,
        renderer: renderer_name(config),
        theme: theme_name(&config.theme.name),
        channel: crate::update::channel_name(config.updates.channel),
        first_run,
        crashed,
    };
    if let Ok(mut clock) = ACTIVITY.lock() {
        *clock = Some(Activity::new(Instant::now()));
    }
    set_on(config.telemetry.enabled);
    let debug = debug_from_the_environment();
    let context = payload::Context::for_this_process(client, debug);
    let target = sender::Target::for_the_binary(folder, debug);
    sender::start(target, context, opening, config.telemetry.interval());
}

/// The configured renderer's name.
fn renderer_name(config: &crate::config::Config) -> &'static str {
    match config.window.renderer {
        crate::config::Renderer::Auto => "auto",
        crate::config::Renderer::Gpu => "gpu",
        crate::config::Renderer::Cpu => "cpu",
    }
}

/// Applies a changed configuration: the switch from the next event on, and the interval.
///
/// @param config - the configuration now in effect
pub fn configure(config: &crate::config::Config) {
    if !sender::is_running() {
        return;
    }
    if config.telemetry.enabled != is_on() {
        set_on(config.telemetry.enabled);
    }
    if config.telemetry.enabled {
        note(Category::Config, "reload");
    }
    sender::set_interval(config.telemetry.interval());
}

/// Sends the closing request and waits at most `wait` for it. Called once, as the process ends.
///
/// @param wait - the longest the process waits for the network
pub fn close(wait: Duration) {
    sender::close(wait);
}

#[cfg(test)]
mod tests;
