//! Spawn parameters shared by every platform.

use std::path::PathBuf;

/// The terminal size in cells and, when known, pixels.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PtySize {
    pub cols: u16,
    pub rows: u16,
    pub pixel_width: u16,
    pub pixel_height: u16,
}

/// Everything needed to start a child on a pseudoterminal.
#[derive(Debug, Clone)]
pub struct SpawnOptions {
    pub program: String,
    pub args: Vec<String>,
    pub cwd: Option<PathBuf>,
    /// Variables added to (or replacing) the inherited environment.
    pub env: Vec<(String, String)>,
    pub size: PtySize,
    /// Start the program as a login shell (Unix only).
    pub login: bool,
}

impl SpawnOptions {
    /// Options for the user's shell: pwsh.exe or powershell.exe on Windows, `$SHELL` or `/bin/sh` elsewhere, with the Terminus environment set.
    pub fn default_shell(size: PtySize) -> Self {
        SpawnOptions {
            program: default_shell_program(),
            args: Vec::new(),
            cwd: None,
            env: terminus_environment(),
            size,
            login: cfg!(unix),
        }
    }
}

/// The variables Terminus sets for every child: TERM (Unix only), COLORTERM, TERM_PROGRAM and its version.
fn terminus_environment() -> Vec<(String, String)> {
    let mut env = Vec::new();
    if cfg!(unix) {
        env.push(("TERM".to_string(), "xterm-terminus".to_string()));
    }
    env.push(("COLORTERM".to_string(), "truecolor".to_string()));
    env.push(("TERM_PROGRAM".to_string(), "terminus".to_string()));
    env.push(("TERM_PROGRAM_VERSION".to_string(), env!("CARGO_PKG_VERSION").to_string()));
    env
}

/// Picks the shell program: pwsh.exe when on PATH else powershell.exe on Windows, `$SHELL` or /bin/sh on Unix.
#[cfg(windows)]
fn default_shell_program() -> String {
    match find_on_path("pwsh.exe") {
        Some(path) => path.to_string_lossy().into_owned(),
        None => "powershell.exe".to_string(),
    }
}

/// Picks the shell program: pwsh.exe when on PATH else powershell.exe on Windows, `$SHELL` or /bin/sh on Unix.
#[cfg(not(windows))]
fn default_shell_program() -> String {
    match std::env::var("SHELL") {
        Ok(shell) if !shell.is_empty() => shell,
        _ => "/bin/sh".to_string(),
    }
}

/// Searches the PATH environment variable for a file name and returns its full path.
#[cfg(windows)]
fn find_on_path(name: &str) -> Option<PathBuf> {
    let path = std::env::var_os("PATH")?;
    std::env::split_paths(&path).map(|dir| dir.join(name)).find(|candidate| candidate.is_file())
}
