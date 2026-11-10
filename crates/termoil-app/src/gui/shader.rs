//! A window's custom shader (`window.shader`): a built in shader by name (`crate::shaders`) or a
//! WGSL file, with the cursor effect (`window.cursor_effect`, `crate::cursor_effects`) composed after
//! it, compiled on the window's GPU renderer, reloaded when the configuration changes or the
//! file's modified time changes, and the error text the window's banner shows.

use std::path::{Path, PathBuf};
use std::time::{Duration, Instant, SystemTime};

use crate::config::Config;
use crate::gui::paint::Painter;

/// Where a window's shader comes from.
#[derive(Clone, Debug, PartialEq, Eq)]
enum Origin {
    /// A shader built into Termoil, by name.
    Builtin(&'static str),
    /// A file, made absolute against the configuration file's folder.
    File(PathBuf),
}

/// The state of one window's shader.
#[derive(Default)]
pub struct ShaderState {
    /// What `window.shader` names.
    path: Option<Origin>,
    /// The cursor effect `window.cursor_effect` names.
    cursor: Option<&'static str>,
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

/// What `window.shader` names: a built in shader when the value is one of their names (no folder and
/// no extension), otherwise a file, with relative paths read from the folder of the configuration
/// file. None when the setting is empty.
fn resolve_path(config: &Config, config_path: &Path) -> Option<Origin> {
    let name = config.window.shader.trim();
    if name.is_empty() {
        return None;
    }
    let bare = !name.contains(['/', '\\', '.']);
    if let Some(b) = crate::shaders::builtin(name).filter(|_| bare) {
        return Some(Origin::Builtin(b.name));
    }
    let path = PathBuf::from(name);
    if path.is_absolute() {
        return Some(Origin::File(path));
    }
    Some(Origin::File(config_path.parent().map_or(path.clone(), |dir| dir.join(&path))))
}

/// The source of a shader and the name its errors carry: the built in text, or the file read now.
fn read(origin: &Origin) -> (String, Result<String, String>) {
    match origin {
        Origin::Builtin(name) => {
            let text = crate::shaders::builtin(name).map(|b| b.source.to_string());
            (format!("built in shader {name}"), text.ok_or_else(|| format!("no built in shader {name}")))
        }
        Origin::File(path) => {
            (path.display().to_string(), std::fs::read_to_string(path).map_err(|e| format!("{}: {e}", path.display())))
        }
    }
}

/// The file's modified time for a file shader, None for a built in one.
fn origin_modified(origin: &Origin) -> Option<SystemTime> {
    match origin {
        Origin::File(path) => modified_time(path),
        Origin::Builtin(_) => None,
    }
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
        let cursor = crate::cursor_effects::effect(&config.window.cursor_effect).map(|e| e.name);
        if path != self.path || cursor != self.cursor {
            // Clearing both settings keeps the old source, so `apply` knows the painter still has a
            // shader to drop. Anything else forgets it, so the new text is compiled.
            if path.is_some() || cursor.is_some() {
                self.source = None;
            }
            self.path = path;
            self.cursor = cursor;
            self.modified = None;
        }
        self.poll = Duration::from_millis(config.window.shader_poll_ms.max(50));
    }

    /// Reads the file and gives it to the painter when its text differs from what the painter has,
    /// or when `force` is set (a new renderer). A read or compile error is kept in `error` and
    /// printed, and the painter then draws without a shader. True when `error` changed.
    pub fn apply(&mut self, painter: &mut Painter, force: bool) -> bool {
        let before = self.error.clone();
        if self.path.is_none() && self.cursor.is_none() {
            if self.must_drop(force) {
                let _ = painter.set_shader(None);
            }
            self.error = None;
            return before != self.error;
        }
        let (label, text) = self.composed();
        match text {
            Err(e) => self.fail(painter, e),
            Ok(text) if force || self.source.as_deref() != Some(text.as_str()) => {
                self.error = painter.set_shader(Some(&text)).err().map(|e| format!("{label}: {e}"));
                if let Some(e) = &self.error {
                    eprintln!("termoil: shader error: {e}");
                }
                self.source = Some(text);
            }
            Ok(_) => {}
        }
        before != self.error
    }

    /// The text the painter compiles, and the name its errors carry: the shader (read now when it is a
    /// file) with the cursor effect after it. Records the file's modified time.
    fn composed(&mut self) -> (String, Result<String, String>) {
        let cursor = self.cursor.and_then(crate::cursor_effects::effect);
        let Some(origin) = self.path.clone() else {
            let name = self.cursor.unwrap_or_default();
            return (format!("cursor effect {name}"), Ok(crate::cursor_effects::compose(None, cursor)));
        };
        self.modified = origin_modified(&origin);
        let (label, text) = read(&origin);
        let label = match self.cursor {
            Some(name) => format!("{label} with cursor effect {name}"),
            None => label,
        };
        (label, text.map(|t| crate::cursor_effects::compose(Some(&t), cursor)))
    }

    /// With no shader set, whether the painter still has one to drop: once after the setting was
    /// cleared, and always for a new renderer (`force`). Forgets the old source.
    fn must_drop(&mut self, force: bool) -> bool {
        self.source.take().is_some() || force
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
        let Some(Origin::File(path)) = &self.path else { return (false, None) };
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
        assert_eq!(p, Some(Origin::File(PathBuf::from("/etc/termoil/fx/crt.wgsl"))));
    }

    #[test]
    fn a_built_in_name_is_the_built_in_shader_and_a_path_is_a_file() {
        let at = Path::new("/etc/termoil/config.toml");
        assert_eq!(resolve_path(&config_with("Neon"), at), Some(Origin::Builtin("neon")));
        assert_eq!(
            resolve_path(&config_with("crt.wgsl"), at),
            Some(Origin::File(PathBuf::from("/etc/termoil/crt.wgsl")))
        );
        let (label, text) = read(&Origin::Builtin("crt"));
        assert_eq!(label, "built in shader crt");
        assert!(text.unwrap().contains("termoil_shade"));
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

    #[test]
    fn a_cursor_effect_alone_or_with_a_shader_is_composed_and_changing_it_recompiles() {
        let at = Path::new("/c/config.toml");
        let mut c = config_with("");
        c.window.cursor_effect = "Sonar".into();
        let mut s = ShaderState::new(&c, at);
        assert_eq!(s.cursor, Some("sonar"));
        let (label, text) = s.composed();
        assert_eq!(label, "cursor effect sonar");
        assert!(text.unwrap().contains("fn termoil_cursor_effect("));
        c.window.shader = "crt".into();
        s.configure(&c, at);
        let (label, text) = s.composed();
        assert_eq!(label, "built in shader crt with cursor effect sonar");
        assert!(text.unwrap().contains("fn termoil_base_shade("));
        s.source = Some("x".into());
        c.window.cursor_effect = "orbit".into();
        s.configure(&c, at);
        assert!(s.source.is_none(), "a new cursor effect is compiled");
    }

    #[test]
    fn clearing_the_setting_drops_the_running_shader_once() {
        // task-2528: setting window.shader to "" left the CRT shader running, because the path
        // change forgot the source and `apply` then thought nothing was running.
        let mut s = ShaderState::new(&config_with("crt.wgsl"), Path::new("/c/config.toml"));
        s.source = Some("fn termoil_shade() {}".into());
        s.configure(&config_with(""), Path::new("/c/config.toml"));
        assert_eq!(s.path, None);
        assert!(s.must_drop(false), "the painter's shader is dropped after the setting is cleared");
        assert!(!s.must_drop(false), "and only once");
        assert!(s.must_drop(true), "a new renderer is always told");
    }
}
