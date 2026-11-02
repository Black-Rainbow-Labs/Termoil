//! A window's custom shader (`window.shader`): loading the WGSL file, compiling it on the window's
//! GPU renderer, reloading it when the configuration changes or the file's modified time changes,
//! and keeping the error text the window's banner shows.

use std::path::{Path, PathBuf};
use std::time::{Duration, Instant, SystemTime};

use crate::config::Config;
use crate::gui::paint::Painter;

/// The state of one window's shader.
#[derive(Default)]
pub struct ShaderState {
    /// The file `window.shader` names, made absolute against the configuration file's folder.
    path: Option<PathBuf>,
    /// The source last handed to the renderer (or that failed), to skip a recompile of the same text.
    source: Option<String>,
    /// The file's modified time when it was last read.
    modified: Option<SystemTime>,
    /// When the modified time was last checked.
    checked: Option<Instant>,
    /// How often the modified time is checked (`window.shader_poll_ms`).
    poll: Duration,
    /// Why the shader is not running, for the banner.
    pub error: Option<String>,
}

/// The shader path of a configuration: the file `window.shader` names, relative paths read from the
/// folder of the configuration file. None when the setting is empty.
fn resolve_path(config: &Config, config_path: &Path) -> Option<PathBuf> {
    let name = config.window.shader.trim();
    if name.is_empty() {
        return None;
    }
    let path = PathBuf::from(name);
    if path.is_absolute() {
        return Some(path);
    }
    Some(config_path.parent().map_or(path.clone(), |dir| dir.join(&path)))
}

/// The file's modified time, None when it cannot be read.
fn modified_time(path: &Path) -> Option<SystemTime> {
    std::fs::metadata(path).and_then(|m| m.modified()).ok()
}

impl ShaderState {
    /// A state for `config`, with nothing compiled yet (see `apply`).
    pub fn new(config: &Config, config_path: &Path) -> ShaderState {
        let mut s = ShaderState::default();
        s.configure(config, config_path);
        s
    }

    /// Reads `window.shader` and its poll interval from a configuration. Call `apply` afterwards.
    pub fn configure(&mut self, config: &Config, config_path: &Path) {
        let path = resolve_path(config, config_path);
        if path != self.path {
            self.path = path;
            self.source = None;
            self.modified = None;
        }
        self.poll = Duration::from_millis(config.window.shader_poll_ms.max(50));
    }

    /// Reads the file and gives it to the painter when its text differs from what the painter has,
    /// or when `force` is set (a new renderer). A read or compile error is kept in `error` and
    /// printed, and the painter then draws without a shader. True when `error` changed.
    pub fn apply(&mut self, painter: &mut Painter, force: bool) -> bool {
        let before = self.error.clone();
        let Some(path) = self.path.clone() else {
            if self.source.take().is_some() || force {
                let _ = painter.set_shader(None);
            }
            self.error = None;
            return before != self.error;
        };
        self.modified = modified_time(&path);
        match std::fs::read_to_string(&path) {
            Err(e) => self.fail(painter, format!("{}: {e}", path.display())),
            Ok(text) if force || self.source.as_deref() != Some(text.as_str()) => {
                self.error = painter.set_shader(Some(&text)).err().map(|e| format!("{}: {e}", path.display()));
                if let Some(e) = &self.error {
                    eprintln!("termoil: shader error: {e}");
                }
                self.source = Some(text);
            }
            Ok(_) => {}
        }
        before != self.error
    }

    /// Records a read failure: the painter drops its shader and the error is shown and printed.
    fn fail(&mut self, painter: &mut Painter, message: String) {
        let _ = painter.set_shader(None);
        eprintln!("termoil: shader error: {message}");
        self.source = None;
        self.error = Some(message);
    }

    /// Checks the file's modified time when the poll interval has passed and reloads the file when
    /// it changed. Returns whether anything changed (the window needs a frame) and when to check
    /// next (None when no shader is set).
    pub fn poll(&mut self, painter: &mut Painter, now: Instant) -> (bool, Option<Instant>) {
        let Some(path) = &self.path else { return (false, None) };
        let due = self.checked.map_or(now, |c| c + self.poll);
        if now < due {
            return (false, Some(due));
        }
        self.checked = Some(now);
        let changed = modified_time(path) != self.modified;
        let redraw = changed && {
            self.apply(painter, false);
            true
        };
        (redraw, Some(now + self.poll))
    }

    /// The text for the window's banner: the shader error, when there is one.
    pub fn banner(&self) -> Option<&str> {
        self.error.as_deref()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A configuration with `window.shader` set to `name`.
    fn config_with(name: &str) -> Config {
        let mut c = Config::default();
        c.window.shader = name.into();
        c
    }

    #[test]
    fn an_empty_shader_setting_means_no_shader() {
        assert_eq!(resolve_path(&config_with("  "), Path::new("/etc/termoil/config.toml")), None);
    }

    #[test]
    fn a_relative_shader_path_is_read_from_the_config_folder() {
        let p = resolve_path(&config_with("fx/crt.wgsl"), Path::new("/etc/termoil/config.toml"));
        assert_eq!(p, Some(PathBuf::from("/etc/termoil/fx/crt.wgsl")));
    }

    #[test]
    fn changing_the_path_forgets_the_compiled_source() {
        let mut s = ShaderState::new(&config_with("a.wgsl"), Path::new("/c/config.toml"));
        s.source = Some("x".into());
        s.configure(&config_with("a.wgsl"), Path::new("/c/config.toml"));
        assert!(s.source.is_some(), "the same path keeps the source");
        s.configure(&config_with("b.wgsl"), Path::new("/c/config.toml"));
        assert!(s.source.is_none());
    }
}
