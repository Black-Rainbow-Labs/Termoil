//! The helper: `termoil --apply-update <file> --kind <kind> --into <path> [--pid N] [--session F]
//! [--launch]`, started by [`super::install::start_helper`] from a copy in the download folder. It waits
//! for Termoil to quit, installs the checked file, and starts the new version with the session it saved.
//! It has no window and no console, so it writes `apply-update.log` beside the file.
//!
//! The new `termoil-gui` finds the session through `TERMOIL_RESTORE_SESSION` in its environment rather
//! than an argument, because an older version chosen from the list does not know the argument and would
//! refuse to start.

use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::OnceLock;
use std::time::{Duration, Instant};

/// The environment variable naming the session file a new version reopens once.
pub const RESTORE_VAR: &str = "TERMOIL_RESTORE_SESSION";
/// How long the helper waits for Termoil to quit before it installs anyway.
const WAIT_FOR_EXIT: Duration = Duration::from_secs(60);

/// The helper's arguments.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct HelperArgs {
    pub file: PathBuf,
    pub kind: String,
    pub into: PathBuf,
    pub pid: Option<u32>,
    pub session: Option<PathBuf>,
    pub launch: bool,
}

/// Reads the arguments after `--apply-update`.
pub fn parse(args: &[String]) -> Result<HelperArgs, String> {
    let mut it = args.iter();
    let mut a =
        HelperArgs { file: PathBuf::from(it.next().ok_or("--apply-update needs a file")?), ..Default::default() };
    while let Some(arg) = it.next() {
        let mut value = || it.next().cloned().ok_or(format!("{arg} needs a value"));
        match arg.as_str() {
            "--kind" => a.kind = value()?,
            "--into" => a.into = PathBuf::from(value()?),
            "--pid" => a.pid = Some(value()?.parse().map_err(|_| "--pid needs a number")?),
            "--session" => a.session = Some(PathBuf::from(value()?)),
            "--launch" => a.launch = true,
            other => return Err(format!("unknown argument '{other}'")),
        }
    }
    if a.kind.is_empty() || a.into.as_os_str().is_empty() {
        return Err("--apply-update needs --kind and --into".into());
    }
    Ok(a)
}

/// Runs the helper and answers whether the install worked. Every step goes to the log.
pub fn run(args: &[String]) -> bool {
    let a = match parse(args) {
        Ok(a) => a,
        Err(e) => {
            eprintln!("termoil: {e}");
            return false;
        }
    };
    let log_path =
        a.file.parent().map(|d| d.join("apply-update.log")).unwrap_or_else(|| PathBuf::from("apply-update.log"));
    let mut log = Log::open(&log_path);
    log.line(&format!("installing {} ({}) into {}", a.file.display(), a.kind, a.into.display()));
    if let Some(pid) = a.pid {
        let ended = wait_for_exit(pid, WAIT_FOR_EXIT);
        log.line(&format!("process {pid} {}", if ended { "ended" } else { "was still running after 60 s" }));
    }
    let installed = install(&a, &mut log);
    match &installed {
        Ok(program) => {
            log.line(&format!("installed; the program is {}", program.display()));
            if a.launch {
                log.line(&match launch(program, a.session.as_deref()) {
                    Ok(()) => "started the new version".to_string(),
                    Err(e) => format!("could not start the new version: {e}"),
                });
            }
        }
        Err(e) => log.line(&format!("the install failed: {e}")),
    }
    installed.is_ok()
}

/// Installs by kind and answers the `termoil-gui` program to start afterwards.
fn install(a: &HelperArgs, log: &mut Log) -> Result<PathBuf, String> {
    match a.kind.as_str() {
        "windows" => install_windows(&a.file, &a.into, log),
        "macos" => install_mac(&a.file, &a.into, log),
        "appimage" => install_appimage(&a.file, &a.into, log),
        other => Err(format!("{other} files are not installed in place")),
    }
}

/// Runs the Inno Setup installer silently into the folder Termoil runs from, for the current user, or
/// for all users when that folder is under Program Files.
fn install_windows(file: &Path, into: &Path, log: &mut Log) -> Result<PathBuf, String> {
    let program_files = std::env::var_os("ProgramFiles").map(PathBuf::from);
    let all_users = program_files.is_some_and(|pf| into.starts_with(pf));
    let mut cmd = Command::new(file);
    cmd.args(["/VERYSILENT", "/SUPPRESSMSGBOXES", "/NORESTART", "/CLOSEAPPLICATIONS"]);
    cmd.arg(format!("/DIR={}", into.display()));
    cmd.arg(if all_users { "/ALLUSERS" } else { "/CURRENTUSER" });
    log.line(&format!("running {:?}", cmd));
    run_checked(cmd)?;
    Ok(into.join("termoil-gui.exe"))
}

/// Attaches the dmg, copies `Termoil.app` beside the running bundle, swaps the two, and detaches.
fn install_mac(file: &Path, bundle: &Path, log: &mut Log) -> Result<PathBuf, String> {
    let mount = file.with_extension("mount");
    let _ = std::fs::create_dir_all(&mount);
    let mut attach = Command::new("hdiutil");
    attach.arg("attach").arg("-nobrowse").arg("-readonly").arg("-noautoopen").arg("-mountpoint").arg(&mount).arg(file);
    log.line(&format!("running {:?}", attach));
    run_checked(attach)?;
    let result = swap_bundle(&mount, bundle, log);
    let mut detach = Command::new("hdiutil");
    detach.arg("detach").arg(&mount).arg("-force");
    let _ = run_checked(detach);
    result.map(|_| bundle.join("Contents").join("MacOS").join("termoil-gui"))
}

/// Copies the bundle out of the mounted image next to `bundle` and swaps it in, keeping the old one
/// until the new one is in place.
fn swap_bundle(mount: &Path, bundle: &Path, log: &mut Log) -> Result<(), String> {
    let source = std::fs::read_dir(mount)
        .map_err(|e| format!("{}: {e}", mount.display()))?
        .filter_map(Result::ok)
        .map(|e| e.path())
        .find(|p| p.extension().is_some_and(|e| e == "app"))
        .ok_or("the disk image holds no .app")?;
    let fresh = bundle.with_extension("app.new");
    let old = bundle.with_extension("app.old");
    let _ = std::fs::remove_dir_all(&fresh);
    let _ = std::fs::remove_dir_all(&old);
    let mut copy = Command::new("ditto");
    copy.arg(&source).arg(&fresh);
    log.line(&format!("running {:?}", copy));
    run_checked(copy)?;
    std::fs::rename(bundle, &old).map_err(|e| format!("could not move the old bundle aside: {e}"))?;
    if let Err(e) = std::fs::rename(&fresh, bundle) {
        let _ = std::fs::rename(&old, bundle);
        return Err(format!("could not move the new bundle into place: {e}"));
    }
    let _ = std::fs::remove_dir_all(&old);
    Ok(())
}

/// Replaces the AppImage file with the new one and makes it executable.
fn install_appimage(file: &Path, image: &Path, log: &mut Log) -> Result<PathBuf, String> {
    let fresh = image.with_extension("AppImage.new");
    std::fs::copy(file, &fresh).map_err(|e| format!("could not copy {}: {e}", file.display()))?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&fresh, std::fs::Permissions::from_mode(0o755)).map_err(|e| e.to_string())?;
    }
    std::fs::rename(&fresh, image).map_err(|e| format!("could not replace {}: {e}", image.display()))?;
    log.line(&format!("replaced {}", image.display()));
    Ok(image.to_path_buf())
}

/// Runs a program and fails with its exit code when it does not succeed.
fn run_checked(mut cmd: Command) -> Result<(), String> {
    super::fetch::no_window(&mut cmd);
    let status = cmd.status().map_err(|e| format!("could not run {:?}: {e}", cmd.get_program()))?;
    if status.success() { Ok(()) } else { Err(format!("{:?} exited with {status}", cmd.get_program())) }
}

/// Starts the new version with the session file in its environment.
fn launch(program: &Path, session: Option<&Path>) -> Result<(), String> {
    let mut cmd = Command::new(program);
    if let Some(s) = session {
        cmd.env(RESTORE_VAR, s);
    }
    if let Some(dir) = program.parent() {
        cmd.current_dir(dir);
    }
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        cmd.process_group(0);
    }
    cmd.spawn().map(|_| ()).map_err(|e| e.to_string())
}

/// Waits until a process has ended, or the limit passes. Answers whether it ended.
fn wait_for_exit(pid: u32, limit: Duration) -> bool {
    let start = Instant::now();
    while start.elapsed() < limit {
        if !is_running(pid) {
            return true;
        }
        std::thread::sleep(Duration::from_millis(200));
    }
    !is_running(pid)
}

/// Whether a process is still running.
#[cfg(windows)]
pub(crate) fn is_running(pid: u32) -> bool {
    use windows::Win32::Foundation::{CloseHandle, WAIT_TIMEOUT};
    use windows::Win32::System::Threading::{OpenProcess, PROCESS_SYNCHRONIZE, WaitForSingleObject};
    // SAFETY: the handle is checked and closed here; a zero wait only reads the process's state.
    unsafe {
        let Ok(handle) = OpenProcess(PROCESS_SYNCHRONIZE, false, pid) else { return false };
        let waiting = WaitForSingleObject(handle, 0) == WAIT_TIMEOUT;
        let _ = CloseHandle(handle);
        waiting
    }
}

/// Whether a process is still running, from `kill -0`.
#[cfg(not(windows))]
pub(crate) fn is_running(pid: u32) -> bool {
    Command::new("kill").arg("-0").arg(pid.to_string()).status().is_ok_and(|s| s.success())
}

/// The helper's log file, opened for appending.
struct Log(Option<std::fs::File>);

impl Log {
    /// Opens the log; a log that cannot be opened writes nothing.
    fn open(path: &Path) -> Log {
        Log(std::fs::OpenOptions::new().create(true).append(true).open(path).ok())
    }

    /// Writes one line with the time in seconds since the epoch.
    fn line(&mut self, text: &str) {
        if let Some(f) = self.0.as_mut() {
            let _ = writeln!(f, "{} {text}", super::now_seconds());
        }
    }
}

/// The session file the update helper asked this process to reopen, taken from the environment once.
static PENDING: OnceLock<Option<PathBuf>> = OnceLock::new();

/// Reads `TERMOIL_RESTORE_SESSION` and removes it from the environment so the shells this process starts
/// do not inherit it. Called first thing in `termoil-gui`'s main, before any thread starts.
pub fn take_restore_from_env() {
    let value = std::env::var_os(RESTORE_VAR).filter(|v| !v.is_empty()).map(PathBuf::from);
    if value.is_some() {
        // SAFETY: called from main before any other thread exists.
        unsafe { std::env::remove_var(RESTORE_VAR) };
    }
    let _ = PENDING.set(value);
}

/// The session file to reopen at start, when the update helper named one.
pub fn pending_restore() -> Option<PathBuf> {
    PENDING.get().cloned().flatten()
}
