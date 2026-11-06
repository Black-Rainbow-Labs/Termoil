//! HTTP through the system's curl. Windows 10 and 11 ship `curl.exe` in System32, macOS ships
//! `/usr/bin/curl`, and Linux distributions have it on the path. Using it keeps an HTTP client and a TLS
//! stack out of the build (`docs/decisions/updates.md`, "Where a check asks").

use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::Duration;

/// The curl program: System32's on Windows, so a curl from another tool on the path is not used.
pub fn curl_program() -> PathBuf {
    if cfg!(windows) {
        let root = std::env::var_os("SystemRoot").map(PathBuf::from).unwrap_or_else(|| PathBuf::from(r"C:\Windows"));
        let system = root.join("System32").join("curl.exe");
        if system.is_file() {
            return system;
        }
        PathBuf::from("curl.exe")
    } else if Path::new("/usr/bin/curl").is_file() {
        PathBuf::from("/usr/bin/curl")
    } else {
        PathBuf::from("curl")
    }
}

/// A curl command with the options every request shares: fail on an HTTP error, follow redirects,
/// quiet but with errors, the time limit, and no console window on Windows.
fn curl(timeout: Duration) -> Command {
    let mut cmd = Command::new(curl_program());
    cmd.args(["--fail", "--location", "--silent", "--show-error", "--proto", "=https,http"])
        .arg("--connect-timeout")
        .arg(timeout.as_secs().max(1).to_string())
        .arg("--user-agent")
        .arg(format!("Termoil/{}", termoil_protocol::VERSION))
        .stdin(Stdio::null());
    no_window(&mut cmd);
    cmd
}

/// Keeps curl from opening a console window when the GUI, which has none, starts it.
#[cfg(windows)]
pub fn no_window(cmd: &mut Command) {
    use std::os::windows::process::CommandExt;
    const CREATE_NO_WINDOW: u32 = 0x0800_0000;
    cmd.creation_flags(CREATE_NO_WINDOW);
}

/// Nothing to do outside Windows.
#[cfg(not(windows))]
pub fn no_window(_cmd: &mut Command) {}

/// Fetches a small text file such as a manifest, with `timeout` for the whole request.
pub fn get_text(url: &str, timeout: Duration) -> Result<String, String> {
    let mut cmd = curl(timeout);
    cmd.arg("--max-time").arg(timeout.as_secs().max(1).to_string()).arg("--max-filesize").arg("4000000");
    cmd.arg("--header").arg("Accept: application/json").arg("--url").arg(url);
    let out = run(cmd)?;
    String::from_utf8(out).map_err(|_| "the answer was not UTF-8 text".to_string())
}

/// Downloads `url` into `to`. The connection must start within `timeout`; the transfer itself may take
/// as long as it needs, but it fails when it stalls for that long.
pub fn download(url: &str, to: &Path, timeout: Duration) -> Result<(), String> {
    let mut cmd = curl(timeout);
    cmd.arg("--speed-limit").arg("1").arg("--speed-time").arg(timeout.as_secs().max(1).to_string());
    cmd.arg("--output").arg(to).arg("--url").arg(url);
    run(cmd).map(|_| ())
}

/// Runs curl and answers its output, or its error message with the exit code.
fn run(mut cmd: Command) -> Result<Vec<u8>, String> {
    let out = cmd.stdout(Stdio::piped()).stderr(Stdio::piped()).output().map_err(|e| {
        if e.kind() == std::io::ErrorKind::NotFound {
            "curl was not found, so Termoil cannot check for updates".to_string()
        } else {
            format!("could not run curl: {e}")
        }
    })?;
    if out.status.success() {
        return Ok(out.stdout);
    }
    let message = String::from_utf8_lossy(&out.stderr).trim().to_string();
    let code = out.status.code().map(|c| c.to_string()).unwrap_or_else(|| "a signal".into());
    Err(if message.is_empty() { format!("curl failed with exit code {code}") } else { message })
}
