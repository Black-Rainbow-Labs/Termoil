//! Downloading a release file and checking it, finding where this Termoil is installed, and starting
//! the helper that installs the file once Termoil has quit (`docs/decisions/updates.md`, "Installing and
//! restarting"). Nothing unchecked is ever run: a file whose size or SHA-256 differs from the manifest is
//! deleted and refused.

use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::Duration;

use sha2::{Digest, Sha256};

use super::{Download, Release, fetch};

/// A downloaded file that matched its manifest entry.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Prepared {
    pub version: String,
    /// The checked file.
    pub file: PathBuf,
    /// The folder it is in, `termoil-update-<version>` in the temporary folder.
    pub folder: PathBuf,
}

/// Where the running Termoil was put by an installer, and so what the helper replaces.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Target {
    /// The Inno Setup folder on Windows, the one holding `unins000.exe`.
    Windows(PathBuf),
    /// The `.app` bundle on macOS.
    MacBundle(PathBuf),
    /// The AppImage file on Linux (`$APPIMAGE`).
    AppImage(PathBuf),
}

impl Target {
    /// The kind of release file this target installs.
    pub fn kind(&self) -> &'static str {
        match self {
            Target::Windows(_) => "windows",
            Target::MacBundle(_) => "macos",
            Target::AppImage(_) => "appimage",
        }
    }

    /// The path the helper is told to install into.
    pub fn path(&self) -> &Path {
        match self {
            Target::Windows(p) | Target::MacBundle(p) | Target::AppImage(p) => p,
        }
    }
}

/// Finds what an installer put the running program in. Fails with the reason when the program is a
/// build no installer placed, or a .deb install, which needs root to replace.
pub fn target() -> Result<Target, String> {
    let exe = std::env::current_exe().map_err(|e| format!("could not find this program: {e}"))?;
    let dir = exe.parent().map(Path::to_path_buf).unwrap_or_default();
    if cfg!(windows) {
        return if dir.join("unins000.exe").is_file() {
            Ok(Target::Windows(dir))
        } else {
            Err(format!("{} was not put there by the installer, so it is not updated in place", dir.display()))
        };
    }
    if cfg!(target_os = "macos") {
        return mac_bundle(&exe)
            .map(Target::MacBundle)
            .ok_or_else(|| format!("{} is not inside Termoil.app, so it is not updated in place", exe.display()));
    }
    match std::env::var_os("APPIMAGE").filter(|v| !v.is_empty()) {
        Some(image) => Ok(Target::AppImage(PathBuf::from(image))),
        None => {
            Err("this Termoil was installed from the .deb, which needs root to update; download the new package".into())
        }
    }
}

/// The `.app` folder holding `exe`, from `<bundle>/Contents/MacOS/<exe>`.
fn mac_bundle(exe: &Path) -> Option<PathBuf> {
    let bundle = exe.parent()?.parent()?.parent()?;
    bundle.extension().is_some_and(|e| e == "app").then(|| bundle.to_path_buf())
}

/// The folder a version is downloaded into.
pub fn folder_for(version: &str) -> PathBuf {
    std::env::temp_dir().join(format!("termoil-update-{version}"))
}

/// Downloads this platform's file of `release` and checks it. A file already there that matches is used
/// again; one that does not is replaced.
pub fn download_checked(release: &Release, kind: &str, timeout: Duration) -> Result<Prepared, String> {
    let d = release.files.get(kind).ok_or_else(|| format!("Termoil {} has no {kind} file", release.version))?;
    let folder = folder_for(&release.version);
    std::fs::create_dir_all(&folder).map_err(|e| format!("could not create {}: {e}", folder.display()))?;
    let name = safe_name(d).ok_or_else(|| format!("the manifest names a bad file name: {}", d.file))?;
    let file = folder.join(name);
    if verify(&file, d).is_ok() {
        return Ok(Prepared { version: release.version.clone(), file, folder });
    }
    let partial = folder.join(format!("{name}.part"));
    let _ = std::fs::remove_file(&partial);
    fetch::download(&d.url, &partial, timeout).inspect_err(|_| {
        let _ = std::fs::remove_file(&partial);
    })?;
    if let Err(e) = verify(&partial, d) {
        let _ = std::fs::remove_file(&partial);
        return Err(e);
    }
    let _ = std::fs::remove_file(&file);
    std::fs::rename(&partial, &file).map_err(|e| format!("could not move the download into place: {e}"))?;
    Ok(Prepared { version: release.version.clone(), file, folder })
}

/// The file name to save as: the manifest's name, or the last part of the URL, with no folder in it.
fn safe_name(d: &Download) -> Option<&str> {
    let name = if d.file.is_empty() { d.url.rsplit('/').next()? } else { d.file.as_str() };
    let ok = !name.is_empty() && !name.contains(['/', '\\', ':']) && name != "." && name != "..";
    ok.then_some(name)
}

/// Checks a file's size and SHA-256 against the manifest.
pub fn verify(path: &Path, d: &Download) -> Result<(), String> {
    let bytes = std::fs::metadata(path).map_err(|e| format!("{}: {e}", path.display()))?.len();
    if bytes != d.bytes {
        return Err(format!("{} is {bytes} bytes but the manifest says {}", d.file, d.bytes));
    }
    let sha = sha256_file(path)?;
    if !sha.eq_ignore_ascii_case(&d.sha256) {
        return Err(format!("{} has SHA-256 {sha} but the manifest says {}", d.file, d.sha256));
    }
    Ok(())
}

/// The SHA-256 of a file as lowercase hex.
pub fn sha256_file(path: &Path) -> Result<String, String> {
    let mut f = std::fs::File::open(path).map_err(|e| format!("{}: {e}", path.display()))?;
    let mut hasher = Sha256::new();
    let mut buf = vec![0u8; 1 << 16];
    loop {
        let n = f.read(&mut buf).map_err(|e| format!("{}: {e}", path.display()))?;
        if n == 0 {
            break;
        }
        hasher.update(&buf[..n]);
    }
    Ok(hasher.finalize().iter().map(|b| format!("{b:02x}")).collect())
}

/// The command line program beside the running one: `termoil` beside `termoil-gui`, or this program
/// when it is the command line itself.
fn cli_program() -> Result<PathBuf, String> {
    let exe = std::env::current_exe().map_err(|e| format!("could not find this program: {e}"))?;
    let name = if cfg!(windows) { "termoil.exe" } else { "termoil" };
    let beside = exe.with_file_name(name);
    if beside.is_file() { Ok(beside) } else { Err(format!("{} was not found", beside.display())) }
}

/// What the helper needs besides the file: who to wait for, and the session to reopen.
#[derive(Clone, Debug, Default)]
pub struct HelperOptions {
    /// The process to wait for before installing; None installs at once.
    pub wait_pid: Option<u32>,
    /// The session file the new version reopens.
    pub session: Option<PathBuf>,
    /// Start the new version when the install is done.
    pub launch: bool,
}

/// Copies the command line program into the download folder (the installer replaces the one in the
/// install folder) and starts it as the helper, detached from this process.
pub fn start_helper(prepared: &Prepared, target: &Target, opts: &HelperOptions) -> Result<PathBuf, String> {
    let helper =
        prepared.folder.join(if cfg!(windows) { "termoil-update-helper.exe" } else { "termoil-update-helper" });
    std::fs::copy(cli_program()?, &helper).map_err(|e| format!("could not copy the helper: {e}"))?;
    let mut cmd = Command::new(&helper);
    cmd.arg("--apply-update").arg(&prepared.file).arg("--kind").arg(target.kind()).arg("--into").arg(target.path());
    if let Some(pid) = opts.wait_pid {
        cmd.arg("--pid").arg(pid.to_string());
    }
    if let Some(session) = &opts.session {
        cmd.arg("--session").arg(session);
    }
    if opts.launch {
        cmd.arg("--launch");
    }
    cmd.stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::null());
    detach(&mut cmd);
    cmd.spawn().map_err(|e| format!("could not start the helper: {e}"))?;
    Ok(prepared.folder.join("apply-update.log"))
}

/// Starts a process that outlives this one: no console window on Windows, its own process group on Unix
/// so a terminal closing does not end it.
fn detach(cmd: &mut Command) {
    fetch::no_window(cmd);
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        cmd.process_group(0);
    }
}
