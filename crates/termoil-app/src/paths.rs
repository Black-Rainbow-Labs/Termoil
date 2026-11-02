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

/// The folder name Termoil used before it was renamed (1.1.0 and earlier).
const LEGACY_NAME: &str = "terminus";

/// Copies the settings and data a Terminus install left behind into the Termoil folders, the first
/// time Termoil starts. A folder is copied only when the Termoil one does not exist yet and no
/// environment variable points somewhere else; the old folders are never removed.
pub fn migrate_legacy_folders() {
    if env_path("TERMOIL_CONFIG").is_none()
        && let Some(base) = config_dir()
    {
        copy_legacy_folder(&base);
    }
    if env_path("TERMOIL_DATA_DIR").is_none()
        && let Some(base) = data_dir().and_then(|d| d.parent().map(PathBuf::from))
        && Some(&base) != config_dir().as_ref()
    {
        copy_legacy_folder(&base);
    }
}

/// Copies `<base>/terminus` to `<base>/termoil` when the first exists and the second does not.
/// Returns true when a copy was made. A failed copy is reported and leaves the old folder in place.
pub fn copy_legacy_folder(base: &std::path::Path) -> bool {
    let (old, new) = (base.join(LEGACY_NAME), base.join("termoil"));
    if !old.is_dir() || new.exists() {
        return false;
    }
    match copy_dir(&old, &new) {
        Ok(()) => true,
        Err(e) => {
            eprintln!("termoil: could not copy {} to {}: {e}", old.display(), new.display());
            false
        }
    }
}

/// Copies a folder and everything under it.
fn copy_dir(from: &std::path::Path, to: &std::path::Path) -> std::io::Result<()> {
    std::fs::create_dir_all(to)?;
    for entry in std::fs::read_dir(from)? {
        let entry = entry?;
        let target = to.join(entry.file_name());
        if entry.file_type()?.is_dir() {
            copy_dir(&entry.path(), &target)?;
        } else {
            std::fs::copy(entry.path(), &target)?;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A fresh empty folder under the system temp folder for one test.
    fn scratch(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("termoil-paths-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn legacy_folder_is_copied_once_and_kept() {
        let base = scratch("copy");
        std::fs::create_dir_all(base.join("terminus").join("themes")).unwrap();
        std::fs::write(base.join("terminus").join("config.toml"), "font_size = 13\n").unwrap();
        std::fs::write(base.join("terminus").join("themes").join("mine.toml"), "x").unwrap();
        assert!(copy_legacy_folder(&base));
        assert_eq!(std::fs::read_to_string(base.join("termoil").join("config.toml")).unwrap(), "font_size = 13\n");
        assert!(base.join("termoil").join("themes").join("mine.toml").is_file());
        assert!(base.join("terminus").join("config.toml").is_file(), "the old folder stays");
        std::fs::write(base.join("terminus").join("config.toml"), "font_size = 20\n").unwrap();
        assert!(!copy_legacy_folder(&base), "an existing termoil folder is never overwritten");
        assert_eq!(std::fs::read_to_string(base.join("termoil").join("config.toml")).unwrap(), "font_size = 13\n");
        let _ = std::fs::remove_dir_all(&base);
    }

    #[test]
    fn nothing_happens_without_a_legacy_folder() {
        let base = scratch("none");
        assert!(!copy_legacy_folder(&base));
        assert!(!base.join("termoil").exists());
        let _ = std::fs::remove_dir_all(&base);
    }
}
