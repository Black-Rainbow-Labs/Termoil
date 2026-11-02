//! The per user folders Termoil reads and writes. Written here rather than taken from the `dirs`
//! crate, whose dependency `option-ext` is MPL 2.0 and outside the licence allow list.

use std::path::PathBuf;

/// An environment variable as a path, when it is set and not empty.
fn env_path(name: &str) -> Option<PathBuf> {
    std::env::var_os(name).filter(|v| !v.is_empty()).map(PathBuf::from)
}

/// The folder for configuration files: `%APPDATA%` on Windows, `~/Library/Application Support` on
/// macOS, `$XDG_CONFIG_HOME` or `~/.config` elsewhere.
pub fn config_dir() -> Option<PathBuf> {
    if cfg!(windows) {
        env_path("APPDATA")
    } else if cfg!(target_os = "macos") {
        env_path("HOME").map(|h| h.join("Library").join("Application Support"))
    } else {
        env_path("XDG_CONFIG_HOME").or_else(|| env_path("HOME").map(|h| h.join(".config")))
    }
}

/// The folder for data Termoil keeps (crash reports, logs): `%LOCALAPPDATA%\termoil` on Windows,
/// `~/Library/Application Support/termoil` on macOS, `$XDG_DATA_HOME/termoil` or
/// `~/.local/share/termoil` elsewhere. `TERMOIL_DATA_DIR` replaces it when set (the tests use it).
pub fn data_dir() -> Option<PathBuf> {
    if let Some(custom) = env_path("TERMOIL_DATA_DIR") {
        return Some(custom);
    }
    let base = if cfg!(windows) {
        env_path("LOCALAPPDATA")
    } else if cfg!(target_os = "macos") {
        env_path("HOME").map(|h| h.join("Library").join("Application Support"))
    } else {
        env_path("XDG_DATA_HOME").or_else(|| env_path("HOME").map(|h| h.join(".local").join("share")))
    };
    base.map(|b| b.join("termoil"))
}
