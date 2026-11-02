//! The settings window's model: every editable setting with its type, how a typed value becomes a TOML
//! value, how one key is written into the configuration file while the rest of the file stays as it was,
//! and the checks a value passes before it is written. The view that shows them is `gui::settings_view`.

use toml_edit::{Array, DocumentMut, Item, Table, Value};

use crate::app::App;
use crate::config::{Config, SECTIONS};

/// What kind of value a setting holds, which decides how it is typed, checked and cycled.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    /// true or false.
    Bool,
    /// A whole number of zero or more.
    Uint,
    /// A number with an optional fraction.
    Float,
    /// Free text.
    Text,
    /// One of the listed names.
    Enum(&'static [&'static str]),
    /// The name of a bundled theme.
    Theme,
    /// A comma separated list of text items.
    TextList,
    /// Two whole numbers, such as a horizontal and a vertical padding.
    UintPair,
    /// A value the view cannot edit; the text says why.
    NotEditable(&'static str),
}

/// One setting of the configuration file.
#[derive(Clone, Debug)]
pub struct Setting {
    /// The `[section]` the key is in, such as `window`.
    pub section: &'static str,
    /// The key's name.
    pub key: &'static str,
    /// The one line description from the schema.
    pub doc: &'static str,
    /// The kind of value.
    pub kind: Kind,
    /// The key has no default, so an empty entry removes it from the file.
    pub optional: bool,
}

impl Setting {
    /// The dotted path, such as `font.size`.
    pub fn path(&self) -> String {
        format!("{}.{}", self.section, self.key)
    }

    /// A short description of what can be typed, shown while the value is edited.
    pub fn hint(&self) -> String {
        let base = match self.kind {
            Kind::Bool => "true or false".to_string(),
            Kind::Uint => "a whole number".to_string(),
            Kind::Float => "a number".to_string(),
            Kind::Text => "text".to_string(),
            Kind::Enum(names) => format!("one of {}", names.join(", ")),
            Kind::Theme => "a bundled theme name".to_string(),
            Kind::TextList => "a comma separated list".to_string(),
            Kind::UintPair => "two whole numbers, such as 4, 4".to_string(),
            Kind::NotEditable(why) => why.to_string(),
        };
        if self.optional { format!("{base}, or empty to remove the key") } else { base }
    }
}

/// The kind of the key at a dotted path.
fn kind_of(path: &str) -> Kind {
    const STYLES: &[&str] = &["block", "bar", "underline"];
    const RENDERERS: &[&str] = &["auto", "gpu", "cpu"];
    const EDGES: &[&str] = &["top", "bottom", "left", "right"];
    const BELLS: &[&str] = &["visual", "audible", "none"];
    const LEVELS: &[&str] = &["error", "warn", "info", "debug", "trace"];
    match path {
        "font.family" | "font.ligatures_off" | "font.fallback" | "colors.palette" | "shell.args" => Kind::TextList,
        "font.size"
        | "font.line_height"
        | "window.opacity"
        | "window.minimum_contrast"
        | "mouse.scroll_multiplier"
        | "scrollback.limit_bytes"
        | "scrollback.limit_lines"
        | "cursor.blink_interval_ms"
        | "window.columns"
        | "window.rows"
        | "notify.after_seconds" => numeric_kind(path),
        "theme.name" | "theme.dark" | "theme.light" => Kind::Theme,
        "cursor.style" => Kind::Enum(STYLES),
        "window.renderer" => Kind::Enum(RENDERERS),
        "window.quick_terminal_edge" => Kind::Enum(EDGES),
        "notify.on_bell" => Kind::Enum(BELLS),
        "log.level" => Kind::Enum(LEVELS),
        "window.padding" => Kind::UintPair,
        "shell.env" => Kind::NotEditable("a table; edit it in the file"),
        p if p.starts_with("keybindings") => Kind::NotEditable("a list of tables; edit it in the file"),
        p if is_bool_key(p) => Kind::Bool,
        _ => Kind::Text,
    }
}

/// Float for the settings that take a fraction, Uint for the counts.
fn numeric_kind(path: &str) -> Kind {
    match path {
        "font.size" | "font.line_height" | "window.opacity" | "window.minimum_contrast" => Kind::Float,
        _ => Kind::Uint,
    }
}

/// Whether the key at `path` is a true or false setting.
fn is_bool_key(path: &str) -> bool {
    const BOOLS: &[&str] = &[
        "font.ligatures",
        "font.hinting",
        "cursor.blink",
        "window.blur",
        "window.decorations",
        "window.title_follows_cwd",
        "window.show_tab_bar",
        "window.palette_hint",
        "shell.integration",
        "clipboard.copy_on_select",
        "clipboard.allow_osc52_read",
        "clipboard.paste_confirm_newline",
        "notify.command_finished",
        "mouse.focus_follows_mouse",
        "mouse.hide_while_typing",
        "updates.check",
    ];
    BOOLS.contains(&path)
}

/// Every setting of the schema, in schema order, with its kind.
pub fn all() -> Vec<Setting> {
    let mut list = Vec::new();
    for section in SECTIONS {
        for entry in section.keys {
            let path = format!("{}.{}", section.path, entry.key);
            let kind = kind_of(&path);
            list.push(Setting {
                section: section.path,
                key: entry.key,
                doc: entry.doc,
                kind,
                optional: entry.example.is_some(),
            });
        }
    }
    list
}

/// The settings the view can edit.
pub fn editable() -> Vec<Setting> {
    all().into_iter().filter(|s| !matches!(s.kind, Kind::NotEditable(_))).collect()
}

/// The names of the settings the view leaves to the file, with every keybindings table as one name.
pub fn not_editable_names() -> Vec<String> {
    let mut names: Vec<String> = Vec::new();
    for s in all().iter().filter(|s| matches!(s.kind, Kind::NotEditable(_))) {
        let name = if s.section.starts_with("keybindings") { "keybindings".to_string() } else { s.path() };
        if !names.contains(&name) {
            names.push(name);
        }
    }
    names
}

/// The value at a setting's path in a serialized configuration.
fn lookup<'a>(table: &'a toml::Table, setting: &Setting) -> Option<&'a toml::Value> {
    table.get(setting.section)?.as_table()?.get(setting.key)
}

/// A TOML value as the text a person types for it: strings without quotes, lists comma separated.
fn plain(value: &toml::Value) -> String {
    match value {
        toml::Value::String(s) => s.clone(),
        toml::Value::Array(items) => items.iter().map(plain).collect::<Vec<_>>().join(", "),
        other => other.to_string(),
    }
}

/// The text to start an edit from: the current value as typed, or empty when the key is not set.
pub fn edit_text(table: &toml::Table, setting: &Setting) -> String {
    lookup(table, setting).map(plain).unwrap_or_default()
}

/// The value as the settings list shows it.
pub fn display_value(table: &toml::Table, setting: &Setting) -> String {
    match lookup(table, setting) {
        None if setting.optional => "(not set)".to_string(),
        None => "(default)".to_string(),
        Some(v) if plain(v).is_empty() => match v {
            toml::Value::Array(_) => "(none)".to_string(),
            _ => "(empty)".to_string(),
        },
        Some(v) => plain(v),
    }
}

/// The configuration as a table, for reading current values.
pub fn snapshot(config: &Config) -> toml::Table {
    toml::Table::try_from(config).unwrap_or_default()
}

/// The names Left and Right step through for a setting, when it has a fixed set.
pub fn choices(setting: &Setting) -> Option<Vec<String>> {
    match setting.kind {
        Kind::Bool => Some(vec!["true".into(), "false".into()]),
        Kind::Enum(names) => Some(names.iter().map(|n| n.to_string()).collect()),
        Kind::Theme => Some(crate::config::themes().iter().map(|t| t.name.clone()).collect()),
        _ => None,
    }
}

/// The next (or previous) name after `current` in the setting's choices, wrapping around.
pub fn cycle(setting: &Setting, current: &str, forward: bool) -> Option<String> {
    let list = choices(setting)?;
    let at = list.iter().position(|c| c.eq_ignore_ascii_case(current.trim()));
    let n = list.len();
    let next = match (at, forward) {
        (Some(i), true) => (i + 1) % n,
        (Some(i), false) => (i + n - 1) % n,
        (None, true) => 0,
        (None, false) => n - 1,
    };
    list.get(next).cloned()
}

/// Parses a typed value for the setting's kind. `Ok(None)` means the key is to be removed.
pub fn to_value(setting: &Setting, input: &str) -> Result<Option<Value>, String> {
    let text = input.trim();
    if setting.optional && text.is_empty() {
        return Ok(None);
    }
    let value = match setting.kind {
        Kind::Bool => Value::from(parse_bool(text)?),
        Kind::Uint => Value::from(parse_uint(text)?),
        Kind::Float => Value::from(parse_float(text)?),
        Kind::Text => Value::from(input),
        Kind::Enum(names) => Value::from(parse_enum(text, names)?),
        Kind::Theme => Value::from(parse_theme(text)?),
        Kind::TextList => Value::Array(text_list(input)),
        Kind::UintPair => Value::Array(parse_pair(text)?),
        Kind::NotEditable(why) => return Err(format!("{} is {why}", setting.path())),
    };
    Ok(Some(value))
}

/// true or false, with yes, no, on and off accepted.
fn parse_bool(text: &str) -> Result<bool, String> {
    match text.to_lowercase().as_str() {
        "true" | "yes" | "on" => Ok(true),
        "false" | "no" | "off" => Ok(false),
        _ => Err(format!("`{text}` is not true or false")),
    }
}

/// A whole number of zero or more that fits a TOML integer.
fn parse_uint(text: &str) -> Result<i64, String> {
    let n: u64 = text.parse().map_err(|_| format!("`{text}` is not a whole number of zero or more"))?;
    i64::try_from(n).map_err(|_| format!("{n} is too large"))
}

/// A finite number.
fn parse_float(text: &str) -> Result<f64, String> {
    match text.parse::<f64>() {
        Ok(n) if n.is_finite() => Ok(n),
        _ => Err(format!("`{text}` is not a number")),
    }
}

/// One of the names, ignoring case; the error lists the names.
fn parse_enum(text: &str, names: &[&str]) -> Result<String, String> {
    let lower = text.to_lowercase();
    if names.contains(&lower.as_str()) {
        return Ok(lower);
    }
    Err(format!("unknown value `{text}`, use one of {}", names.join(", ")))
}

/// The bundled theme's own spelling of its name.
fn parse_theme(text: &str) -> Result<String, String> {
    crate::config::theme(text)
        .map(|t| t.name.clone())
        .ok_or_else(|| format!("no theme named `{text}`, see termoil +list-themes"))
}

/// A comma separated list as an array of strings, skipping empty items.
fn text_list(input: &str) -> Array {
    input.split(',').map(str::trim).filter(|s| !s.is_empty()).collect()
}

/// Two whole numbers separated by a comma, a space or an x.
fn parse_pair(text: &str) -> Result<Array, String> {
    let parts: Vec<&str> = text.split([',', ' ', 'x']).filter(|p| !p.is_empty()).collect();
    let bad = || format!("`{text}` is not two whole numbers, such as 4, 4");
    let [a, b] = parts[..] else { return Err(bad()) };
    let n = |s: &str| s.parse::<u32>().map(i64::from).map_err(|_| bad());
    Ok([n(a)?, n(b)?].into_iter().collect())
}

/// The configuration text with the setting's key set to `input`, or removed when `input` is empty for an
/// optional key. Only that key changes: comments, other keys and their order stay as they were.
pub fn with_value(text: &str, setting: &Setting, input: &str) -> Result<String, String> {
    let mut doc: DocumentMut = text.parse().map_err(|e| format!("the file is not valid TOML: {e}"))?;
    match to_value(setting, input)? {
        Some(value) => set_value(&mut doc, setting, value)?,
        None => remove_value(&mut doc, setting)?,
    }
    Ok(doc.to_string())
}

/// Puts `value` at the setting's key, creating its section when the file has none.
fn set_value(doc: &mut DocumentMut, setting: &Setting, value: Value) -> Result<(), String> {
    if setting.section == "theme" && set_bare_theme(doc, setting, &value) {
        return Ok(());
    }
    let table = section_mut(doc, setting.section)?;
    put(table, setting.key, value);
    Ok(())
}

/// Handles a file that writes `theme = "Name"` at the top. Setting `name` replaces that line's value;
/// setting `dark` or `light` turns the line into a `[theme]` table that keeps the name. Returns true when
/// the value was fully written here.
fn set_bare_theme(doc: &mut DocumentMut, setting: &Setting, value: &Value) -> bool {
    let Some(Item::Value(old)) = doc.get("theme") else { return false };
    let old_name = old.as_str().unwrap_or_default().to_string();
    if setting.key == "name" {
        put(doc.as_table_mut(), "theme", value.clone());
        return true;
    }
    let mut table = Table::new();
    table.insert("name", Item::Value(Value::from(old_name)));
    table.insert(setting.key, Item::Value(value.clone()));
    doc.insert("theme", Item::Table(table));
    true
}

/// The table for a section, created (with a blank line before it) when missing.
fn section_mut<'a>(doc: &'a mut DocumentMut, section: &str) -> Result<&'a mut dyn toml_edit::TableLike, String> {
    if !doc.contains_key(section) {
        let mut table = Table::new();
        if !doc.to_string().trim().is_empty() {
            table.decor_mut().set_prefix("\n");
        }
        doc.insert(section, Item::Table(table));
    }
    doc.get_mut(section)
        .and_then(Item::as_table_like_mut)
        .ok_or_else(|| format!("`{section}` in the file is not a table"))
}

/// Sets `key` in a table. A key already there keeps the spacing and trailing comment of its line.
fn put(table: &mut dyn toml_edit::TableLike, key: &str, mut value: Value) {
    match table.get_mut(key) {
        Some(item) => {
            if let Some(old) = item.as_value() {
                *value.decor_mut() = old.decor().clone();
            }
            *item = Item::Value(value);
        }
        None => {
            table.insert(key, Item::Value(value));
        }
    }
}

/// Removes the setting's key when the file has it.
fn remove_value(doc: &mut DocumentMut, setting: &Setting) -> Result<(), String> {
    if setting.section == "theme" && matches!(doc.get("theme"), Some(Item::Value(_))) {
        return Err("theme.name cannot be removed".into());
    }
    if let Some(table) = doc.get_mut(setting.section).and_then(Item::as_table_like_mut) {
        table.remove(setting.key);
    }
    Ok(())
}

/// The new file text for an edit, after checking it: the file must parse, and none of the checks in
/// `config/check.rs` (ranges, types, enum names) may complain about the key. Nothing is written here.
pub fn checked_edit(text: &str, setting: &Setting, input: &str) -> Result<String, String> {
    Config::parse(text).map_err(|e| format!("fix the file first, {e}"))?;
    let new_text = with_value(text, setting, input)?;
    let (_, warnings) = Config::parse(&new_text).map_err(|e| e.message)?;
    let path = setting.path();
    let own = |w: &&crate::config::Warning| {
        w.message.contains(&path) && !w.message.contains("old name") && !w.message.contains("renamed")
    };
    match warnings.iter().find(own) {
        Some(w) => Err(w.message.clone()),
        None => Ok(new_text),
    }
}

/// Checks an edit, writes it to the configuration file and reloads the file so the change takes effect.
pub fn save(app: &mut App, setting: &Setting, input: &str) -> Result<(), String> {
    let path = app.config_path.clone();
    let text = std::fs::read_to_string(&path).unwrap_or_default();
    let new_text = checked_edit(&text, setting, input)?;
    crate::themes::write_file(&path, &new_text)?;
    crate::view::reload_config(app).map(|_| ()).map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The setting at a dotted path.
    fn setting(path: &str) -> Setting {
        all().into_iter().find(|s| s.path() == path).expect("a schema key")
    }

    /// A key in a section the file already has changes in place and keeps the comments.
    #[test]
    fn writes_a_key_in_an_existing_section() {
        let text = "# mine\n[font]\nfamily = [\"A\"] # the face\nsize = 12 # points\n\n[cursor]\nblink = true\n";
        let out = with_value(text, &setting("font.size"), "14").unwrap();
        assert_eq!(
            out,
            "# mine\n[font]\nfamily = [\"A\"] # the face\nsize = 14.0 # points\n\n[cursor]\nblink = true\n"
        );
    }

    /// A key in a section the file lacks adds the section at the end.
    #[test]
    fn writes_a_key_whose_section_is_missing() {
        let out = with_value("[font]\nsize = 12\n", &setting("window.opacity"), "0.9").unwrap();
        assert_eq!(out, "[font]\nsize = 12\n\n[window]\nopacity = 0.9\n");
        assert_eq!(with_value("", &setting("cursor.style"), "bar").unwrap(), "[cursor]\nstyle = \"bar\"\n");
    }

    /// A key missing from an existing section is added to that section.
    #[test]
    fn adds_a_key_to_a_section_that_exists() {
        let out = with_value("[window]\n# note\nblur = true\n[log]\nlevel = \"info\"\n", &setting("window.rows"), "30");
        assert_eq!(out.unwrap(), "[window]\n# note\nblur = true\nrows = 30\n[log]\nlevel = \"info\"\n");
    }

    /// An empty entry removes an optional key and is refused for a key with a default.
    #[test]
    fn empty_removes_an_optional_key() {
        let text = "[scrollback]\n# keep\nlimit_bytes = 5\nlimit_lines = 100\n";
        let out = with_value(text, &setting("scrollback.limit_lines"), "").unwrap();
        assert_eq!(out, "[scrollback]\n# keep\nlimit_bytes = 5\n");
        assert!(with_value(text, &setting("scrollback.limit_bytes"), "").is_err());
    }

    /// `theme = "Name"` at the top is edited in place, and becomes a table when dark is set.
    #[test]
    fn edits_a_bare_theme_line() {
        let text = "theme = \"Nord\" # colours\n[font]\nsize = 12\n";
        let out = with_value(text, &setting("theme.name"), "dracula").unwrap();
        assert_eq!(out, "theme = \"Dracula\" # colours\n[font]\nsize = 12\n");
        let both = with_value(text, &setting("theme.dark"), "Dracula").unwrap();
        let (config, _) = Config::parse(&both).unwrap();
        assert_eq!((config.theme.name.as_str(), config.theme.dark.as_deref()), ("Nord", Some("Dracula")));
    }

    /// Keybindings and the shell environment are left out of the editor and named in the view.
    #[test]
    fn lists_and_tables_are_not_editable() {
        assert!(editable().iter().all(|s| !s.section.starts_with("keybindings") && s.path() != "shell.env"));
        assert_eq!(not_editable_names(), vec!["shell.env".to_string(), "keybindings".to_string()]);
        assert!(with_value("", &setting("keybindings.bindings"), "x").is_err());
    }

    /// Every key of the schema has a kind, so the view covers the whole file.
    #[test]
    fn every_schema_key_is_listed() {
        let paths: Vec<String> = all().iter().map(Setting::path).collect();
        assert_eq!(paths.len(), SECTIONS.iter().map(|s| s.keys.len()).sum::<usize>());
        assert_eq!(setting("font.size").kind, Kind::Float);
        assert_eq!(setting("window.blur").kind, Kind::Bool);
        assert!(editable().len() > 50);
    }

    /// A number outside the range check.rs allows is refused, with that check's message.
    #[test]
    fn rejects_out_of_range_numbers() {
        let err = checked_edit("", &setting("window.opacity"), "1.5").unwrap_err();
        assert!(err.contains("window.opacity must be from 0.0 to 1.0"), "{err}");
        let err = checked_edit("", &setting("font.size"), "0").unwrap_err();
        assert!(err.contains("font.size must be above 0"), "{err}");
        let err = checked_edit("", &setting("window.minimum_contrast"), "40").unwrap_err();
        assert!(err.contains("window.minimum_contrast"), "{err}");
        assert!(checked_edit("", &setting("window.opacity"), "0.5").is_ok());
    }

    /// An unknown enum name, a non number and a negative count are refused.
    #[test]
    fn rejects_bad_values() {
        let err = checked_edit("", &setting("cursor.style"), "pointy").unwrap_err();
        assert!(err.contains("unknown value `pointy`") && err.contains("block, bar, underline"), "{err}");
        assert!(checked_edit("", &setting("font.size"), "big").unwrap_err().contains("not a number"));
        assert!(checked_edit("", &setting("scrollback.limit_bytes"), "-1").is_err());
        assert!(checked_edit("", &setting("window.blur"), "maybe").is_err());
        assert!(checked_edit("", &setting("theme.name"), "No Such Theme").unwrap_err().contains("no theme named"));
        assert!(checked_edit("", &setting("colors.cursor"), "red").is_err());
        assert!(checked_edit("", &setting("window.padding"), "4").is_err());
    }

    /// A file that already has a syntax error is not written over.
    #[test]
    fn refuses_a_broken_file() {
        let err = checked_edit("[font\nsize = 1", &setting("font.size"), "14").unwrap_err();
        assert!(err.starts_with("fix the file first"), "{err}");
    }

    /// Lists, pairs and colours are written with their TOML types and parse back.
    #[test]
    fn writes_lists_pairs_and_colours() {
        let text = checked_edit("", &setting("font.family"), "Fira Code, Menlo").unwrap();
        let text = checked_edit(&text, &setting("window.padding"), "6, 2").unwrap();
        let text = checked_edit(&text, &setting("colors.cursor"), "#ff0000").unwrap();
        let (config, _) = Config::parse(&text).unwrap();
        assert_eq!(config.font.family, vec!["Fira Code", "Menlo"]);
        assert_eq!(config.window.padding, [6, 2]);
        assert_eq!(config.colors.cursor.map(|c| c.0), Some([255, 0, 0]));
    }

    /// Left and Right step through the choices and wrap.
    #[test]
    fn cycles_choices() {
        let s = setting("cursor.style");
        assert_eq!(cycle(&s, "block", true).as_deref(), Some("bar"));
        assert_eq!(cycle(&s, "block", false).as_deref(), Some("underline"));
        assert_eq!(cycle(&s, "underline", true).as_deref(), Some("block"));
        assert!(cycle(&setting("font.size"), "1", true).is_none());
    }

    /// Current values come from the configuration and show as a person would type them.
    #[test]
    fn shows_current_values() {
        let table = snapshot(&Config::default());
        assert_eq!(display_value(&table, &setting("font.size")), "13.0");
        assert_eq!(display_value(&table, &setting("window.padding")), "4, 4");
        assert_eq!(display_value(&table, &setting("shell.program")), "(not set)");
        assert_eq!(display_value(&table, &setting("font.fallback")), "(none)");
        assert_eq!(edit_text(&table, &setting("cursor.style")), "block");
    }
}
