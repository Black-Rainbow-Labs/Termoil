//! Inspection of a parsed config file: unknown keys, renamed keys, bad bindings and out of range values.
//! Everything here reports a line number taken from the source text.
use toml::Spanned;
use toml::de::{DeString, DeTable, DeValue};

use super::keys::parse_keys;
use super::schema::section;
use super::{Config, Warning};

/// Old key paths and the paths that replaced them. The new path must start with the old key's parent path so the
/// key can be rewritten in place, which keeps every line number in later errors correct.
pub static RENAMES: &[(&str, &str)] = &[
    ("font-size", "font.size"),
    ("font-family", "font.family"),
    ("font-ligatures", "font.ligatures"),
    ("line-height", "font.line_height"),
    ("font.ligature", "font.ligatures"),
    ("scrollback.lines", "scrollback.limit_lines"),
    ("scrollback.limit", "scrollback.limit_bytes"),
    ("scrollback-limit", "scrollback.limit_bytes"),
    ("background-opacity", "window.opacity"),
    ("window.background_opacity", "window.opacity"),
    ("window-padding", "window.padding"),
    ("window.contrast", "window.minimum_contrast"),
    ("cursor-style", "cursor.style"),
    ("cursor-blink", "cursor.blink"),
    ("copy-on-select", "clipboard.copy_on_select"),
    ("shell.command", "shell.program"),
];

/// A text edit that rewrites an old key into its new name.
pub struct Rename {
    pub start: usize,
    pub end: usize,
    pub replacement: String,
}

/// What inspecting a file found.
#[derive(Default)]
pub struct Report {
    pub warnings: Vec<Warning>,
    pub renames: Vec<Rename>,
}

/// Converts a byte offset into a 1 based line and column.
pub fn line_col(text: &str, offset: usize) -> (usize, usize) {
    let offset = offset.min(text.len());
    let before = &text.as_bytes()[..offset];
    let line = before.iter().filter(|&&b| b == b'\n').count() + 1;
    let line_start = before.iter().rposition(|&b| b == b'\n').map_or(0, |i| i + 1);
    let column = String::from_utf8_lossy(&before[line_start..]).chars().count() + 1;
    (line, column)
}

/// Looks up a value by dotted path in a parsed table.
fn find_value<'a, 'i>(table: &'a DeTable<'i>, path: &str) -> Option<&'a Spanned<DeValue<'i>>> {
    let mut parts = path.split('.').peekable();
    let mut current = table;
    while let Some(part) = parts.next() {
        let (_, value) = current.iter().find(|(k, _)| k.get_ref().as_ref() == part)?;
        if parts.peek().is_none() {
            return Some(value);
        }
        current = value.get_ref().as_table()?;
    }
    None
}

/// The line of a key given by dotted path, or 0 when the file does not set it.
pub fn line_of(text: &str, table: &DeTable<'_>, path: &str) -> usize {
    find_value(table, path).map_or(0, |v| line_col(text, v.span().start).0)
}

/// Walks a table, recording unknown keys, renames and binding problems into `report`.
pub fn inspect(text: &str, table: &DeTable<'_>) -> Report {
    let mut report = Report::default();
    walk(text, table, "", table, &mut report);
    report
}

/// Joins a parent path and a key into a dotted path.
fn join_path(parent: &str, key: &str) -> String {
    if parent.is_empty() { key.to_string() } else { format!("{parent}.{key}") }
}

/// Visits every key of `current`, which sits at `parent`, descending into known sections.
fn walk(text: &str, root: &DeTable<'_>, parent: &str, current: &DeTable<'_>, report: &mut Report) {
    for (key, value) in current {
        let path = join_path(parent, key.get_ref());
        let line = line_col(text, key.span().start).0;
        if let Some((_, new)) = RENAMES.iter().find(|(old, _)| *old == path) {
            note_rename(root, parent, key, new, line, report);
        } else if section(&path).is_some() {
            if let Some(table) = value.get_ref().as_table() {
                walk(text, root, &path, table, report);
            }
        } else if !is_known_key(parent, key.get_ref()) {
            report.warnings.push(Warning { line, message: format!("unknown key `{path}` is ignored") });
        } else if path.ends_with(".bindings") {
            check_bindings(text, value, report);
        }
    }
}

/// Whether `key` is a documented key of the section at `parent`.
fn is_known_key(parent: &str, key: &str) -> bool {
    section(parent).is_some_and(|s| s.keys.iter().any(|e| e.key == key))
}

/// Records the rewrite for a renamed key, or a warning when the new key is already set in the file.
fn note_rename(
    root: &DeTable<'_>, parent: &str, key: &Spanned<DeString<'_>>, new: &str, line: usize, report: &mut Report,
) {
    let old = join_path(parent, key.get_ref());
    if find_value(root, new).is_some() {
        let message = format!("`{old}` is the old name of `{new}`, which is also set; `{old}` is ignored");
        report.warnings.push(Warning { line, message });
        return;
    }
    report.warnings.push(Warning { line, message: format!("`{old}` was renamed to `{new}`; the new name was used") });
    let relative = if parent.is_empty() { new } else { new.strip_prefix(&format!("{parent}.")).unwrap_or(new) };
    let span = key.span();
    report.renames.push(Rename { start: span.start, end: span.end, replacement: relative.to_string() });
}

/// Warns about each binding in a `bindings` array whose keys do not parse or that has an unknown field.
fn check_bindings(text: &str, value: &Spanned<DeValue<'_>>, report: &mut Report) {
    let Some(items) = value.get_ref().as_array() else { return };
    for item in items {
        let Some(table) = item.get_ref().as_table() else { continue };
        for (field, v) in table {
            let line = line_col(text, v.span().start).0;
            match field.get_ref().as_ref() {
                "keys" => check_binding_keys(v, line, report),
                "action" | "args" | "table" => {}
                other => report
                    .warnings
                    .push(Warning { line, message: format!("unknown binding field `{other}` is ignored") }),
            }
        }
    }
}

/// Warns when the `keys` string of a binding cannot be parsed; the binding is then skipped when resolving.
fn check_binding_keys(value: &Spanned<DeValue<'_>>, line: usize, report: &mut Report) {
    if let Some(keys) = value.get_ref().as_str()
        && let Err(why) = parse_keys(keys)
    {
        report.warnings.push(Warning { line, message: format!("binding `{keys}` is ignored: {why}") });
    }
}

/// Applies the recorded renames to the text, last first so earlier offsets stay valid.
pub fn apply_renames(text: &str, renames: &[Rename]) -> String {
    let mut sorted: Vec<&Rename> = renames.iter().collect();
    sorted.sort_by_key(|r| std::cmp::Reverse(r.start));
    let mut out = text.to_string();
    for r in sorted {
        out.replace_range(r.start..r.end, &r.replacement);
    }
    out
}

/// Checks ranges after deserializing, correcting a bad value to something usable and adding a warning that
/// names the line the file set it on.
pub fn validate(config: &mut Config, text: &str, table: &DeTable<'_>, warnings: &mut Vec<Warning>) {
    let mut fix = |path: &str, message: String| {
        warnings.push(Warning { line: line_of(text, table, path), message });
    };
    let defaults = Config::default();
    if config.font.size.is_nan() || config.font.size <= 0.0 {
        fix("font.size", format!("font.size must be above 0, using {}", defaults.font.size));
        config.font.size = defaults.font.size;
    }
    if config.font.line_height.is_nan() || config.font.line_height <= 0.0 {
        fix("font.line_height", format!("font.line_height must be above 0, using {}", defaults.font.line_height));
        config.font.line_height = defaults.font.line_height;
    }
    if !(0.0..=1.0).contains(&config.window.opacity) {
        fix("window.opacity", "window.opacity must be from 0.0 to 1.0, it was clamped".into());
        config.window.opacity =
            if config.window.opacity.is_nan() { 1.0 } else { config.window.opacity.clamp(0.0, 1.0) };
    }
    if config.window.columns == 0 || config.window.rows == 0 {
        fix("window.columns", "window.columns and window.rows must be at least 1, using 1".into());
        config.window.columns = config.window.columns.max(1);
        config.window.rows = config.window.rows.max(1);
    }
    if !(1.0..=21.0).contains(&config.window.minimum_contrast) {
        fix("window.minimum_contrast", "window.minimum_contrast must be from 1 to 21, it was clamped".into());
        config.window.minimum_contrast = config.window.minimum_contrast.clamp(1.0, 21.0);
    }
    if config.colors.palette.len() > 16 {
        fix("colors.palette", "colors.palette has more than 16 colours, the extra ones are ignored".into());
        config.colors.palette.truncate(16);
    }
    if config.cursor.blink_interval_ms == 0 {
        fix(
            "cursor.blink_interval_ms",
            format!("cursor.blink_interval_ms must be above 0, using {}", defaults.cursor.blink_interval_ms),
        );
        config.cursor.blink_interval_ms = defaults.cursor.blink_interval_ms;
    }
}
