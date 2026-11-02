//! Configuration: the TOML file, its typed model, defaults, key bindings, themes and live reload.
//!
//! `Config::parse` reads text into a `Config` with warnings, `Config::load` reads the file, `Watcher` reports
//! changes, `Config::default_file_text` is the documented default file and `Config::effective_text` is what
//! `terminus +show-config` prints.
use std::fmt;
use std::path::{Path, PathBuf};

mod check;
mod keys;
mod model;
mod schema;
mod theme;
mod watch;

pub use check::RENAMES;
pub use keys::{
    Binding, KeyChord, Mods, Platform, default_keybindings, default_keybindings_for, default_keybindings_macos,
    normalize_keys, overlay_bindings, parse_keys,
};
pub use model::*;
pub use schema::{Entry, SECTIONS, Section};
pub use theme::{
    Chrome, DEFAULT_THEME, Theme, chrome_for, contrast_ratio, derived_chrome, relative_luminance, theme, themes,
};
pub use watch::{DEFAULT_DEBOUNCE, LoadResult, Watcher};

/// A problem in the file that did not stop it loading: an unknown or renamed key, a bad binding or a value that
/// was corrected. `line` is 1 based, or 0 when the file does not say where.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Warning {
    pub line: usize,
    pub message: String,
}

impl fmt::Display for Warning {
    /// Writes `line N: message`, or just the message when the line is unknown.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.line == 0 { write!(f, "{}", self.message) } else { write!(f, "line {}: {}", self.line, self.message) }
    }
}

/// An error that stops a file loading: a syntax error or a value of the wrong type. `line` and `column` are 1
/// based; both are 0 when the file could not be read at all.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ConfigError {
    pub line: usize,
    pub column: usize,
    pub message: String,
}

impl fmt::Display for ConfigError {
    /// Writes `line N, column M: message`.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.line == 0 {
            write!(f, "{}", self.message)
        } else {
            write!(f, "line {}, column {}: {}", self.line, self.column, self.message)
        }
    }
}

impl std::error::Error for ConfigError {}

impl ConfigError {
    /// Builds an error from a toml error, converting its byte span to a line and column in `text`.
    fn from_toml(text: &str, error: &toml::de::Error) -> ConfigError {
        let offset = error.span().map_or(0, |s| s.start);
        let (line, column) = if error.span().is_some() { check::line_col(text, offset) } else { (1, 1) };
        ConfigError { line, column, message: error.message().trim().to_string() }
    }
}

impl Config {
    /// Parses config text. Any subset of keys is accepted. Unknown keys and old names produce warnings (an old
    /// name is applied under its new name); a syntax error or a value of the wrong type is an error.
    pub fn parse(text: &str) -> Result<(Config, Vec<Warning>), ConfigError> {
        let doc = toml::de::DeTable::parse(text).map_err(|e| ConfigError::from_toml(text, &e))?;
        let table = doc.get_ref();
        let mut report = check::inspect(text, table);
        let patched = check::apply_renames(text, &report.renames);
        let mut config: Config = toml::from_str(&patched).map_err(|e| ConfigError::from_toml(&patched, &e))?;
        check::validate(&mut config, text, table, &mut report.warnings);
        report.warnings.sort_by_key(|w| w.line);
        Ok((config, report.warnings))
    }

    /// Reads and parses the file at `path`. A missing file gives the defaults and no warnings.
    pub fn load(path: &Path) -> Result<(Config, Vec<Warning>), ConfigError> {
        match std::fs::read_to_string(path) {
            Ok(text) => Config::parse(&text),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok((Config::default(), vec![])),
            Err(e) => Err(ConfigError { line: 0, column: 0, message: format!("cannot read {}: {e}", path.display()) }),
        }
    }

    /// The config file location: `TERMINUS_CONFIG` when set, otherwise `terminus/config.toml` under the
    /// platform's config folder (`%APPDATA%`, `~/Library/Application Support` or `$XDG_CONFIG_HOME`).
    pub fn path() -> PathBuf {
        if let Some(custom) = std::env::var_os("TERMINUS_CONFIG").filter(|v| !v.is_empty()) {
            return PathBuf::from(custom);
        }
        let base = crate::paths::config_dir().unwrap_or_else(|| PathBuf::from("."));
        base.join("terminus").join("config.toml")
    }

    /// The keybindings in effect on `platform`: the platform's defaults, then the file's bindings, then the
    /// platform section's. A later binding for the same keys replaces an earlier one and `action = "none"`
    /// removes it.
    pub fn resolved_keybindings(&self, platform: Platform) -> Vec<Binding> {
        let mut list = default_keybindings_for(platform);
        overlay_bindings(&mut list, &self.keybindings.bindings);
        let section = match platform {
            Platform::Windows => &self.keybindings.windows,
            Platform::MacOs => &self.keybindings.macos,
            Platform::Linux => &self.keybindings.linux,
        };
        overlay_bindings(&mut list, &section.bindings);
        list
    }
}
