//! The settings overlay: every editable setting of the configuration file grouped by section, with its
//! current value and help text. Typing filters, Up and Down move, Enter edits the selected value, and a
//! saved value is written to the file by `settings::save`. It is drawn like the palette, as a `Strip`.

use serde_json::Map;
use terminus_input::{Key, KeyEvent, NamedKey};
use terminus_protocol::ActionCall;
use terminus_render::Colors;

use crate::app::App;
use crate::gui::chrome::sgr_colors;
use crate::gui::look::mix;
use crate::gui::overlay::{After, clip, typed};
use crate::settings::{self, Kind, Setting};

/// The settings view's width in columns, at most.
pub const SETTINGS_COLS: u16 = 120;
/// Rows the view keeps for its title, status, detail and note lines.
const CHROME_ROWS: usize = 4;
/// Columns for a setting's path and for its value in the list.
const PATH_COLS: usize = 30;
const VALUE_COLS: usize = 28;

/// One selectable row of the list.
#[derive(Clone)]
enum Row {
    /// Runs the `open-config` action.
    OpenFile,
    /// A setting, by index into the view's settings.
    Setting(usize),
}

/// A value being typed.
struct Edit {
    /// The text typed so far.
    text: String,
}

/// The settings view's state.
pub struct SettingsUi {
    /// The filter text.
    pub query: String,
    /// The highlighted row among the matches.
    pub selected: usize,
    settings: Vec<Setting>,
    table: toml::Table,
    edit: Option<Edit>,
    message: Option<Message>,
}

/// What the status line says.
enum Message {
    /// A value was refused or could not be written.
    Error(String),
    /// A value was saved.
    Saved(String),
}

impl SettingsUi {
    /// A view of the settings with the values in `config`.
    pub fn new(config: &crate::config::Config) -> SettingsUi {
        SettingsUi {
            query: String::new(),
            selected: 0,
            settings: settings::editable(),
            table: settings::snapshot(config),
            edit: None,
            message: None,
        }
    }

    /// The rows matching the filter: every word of the query must appear in the setting's path or help.
    fn matches(&self) -> Vec<Row> {
        let words: Vec<String> = self.query.to_lowercase().split_whitespace().map(String::from).collect();
        let open = ["open the file", "config", "edit file"];
        let mut rows = Vec::new();
        if words.iter().all(|w| open.iter().any(|o| o.contains(w.as_str()))) {
            rows.push(Row::OpenFile);
        }
        for (i, s) in self.settings.iter().enumerate() {
            let hay = format!("{} {}", s.path(), s.doc).to_lowercase();
            if words.iter().all(|w| hay.contains(w.as_str())) {
                rows.push(Row::Setting(i));
            }
        }
        rows
    }

    /// The setting under the highlight, when the highlight is on a setting.
    fn current(&self) -> Option<&Setting> {
        match self.matches().get(self.selected)? {
            Row::Setting(i) => self.settings.get(*i),
            Row::OpenFile => None,
        }
    }
}

/// Handles a key in the settings view.
pub fn key(s: &mut SettingsUi, app: &mut App, ev: &KeyEvent) -> After {
    if s.edit.is_some() { edit_key(s, app, ev) } else { browse_key(s, app, ev) }
}

/// Keys while browsing: type to filter, arrows move, Enter edits or runs, Space toggles a boolean.
fn browse_key(s: &mut SettingsUi, app: &mut App, ev: &KeyEvent) -> After {
    match ev.key {
        Key::Named(NamedKey::Escape) => return After::Close,
        Key::Named(NamedKey::Up) => s.selected = s.selected.saturating_sub(1),
        Key::Named(NamedKey::Down) => s.selected = (s.selected + 1).min(s.matches().len().saturating_sub(1)),
        Key::Named(NamedKey::Enter) => return activate(s, app),
        Key::Named(NamedKey::Backspace) => {
            s.query.pop();
            s.selected = 0;
        }
        Key::Char(' ') if matches!(s.current().map(|c| c.kind), Some(Kind::Bool)) => toggle(s, app),
        _ => {
            if let Some(t) = typed(ev) {
                s.query.push_str(t);
                s.selected = 0;
            }
        }
    }
    After::Open
}

/// Enter on a row: open the file, flip a boolean, or start editing the value.
fn activate(s: &mut SettingsUi, app: &mut App) -> After {
    let Some(row) = s.matches().get(s.selected).cloned() else { return After::Open };
    let Row::Setting(i) = row else {
        let _ = app.run_action(&ActionCall { action: "open-config".into(), args: Map::new() });
        return After::Close;
    };
    let setting = s.settings[i].clone();
    if setting.kind == Kind::Bool {
        toggle(s, app);
    } else {
        s.message = None;
        s.edit = Some(Edit { text: settings::edit_text(&s.table, &setting) });
    }
    After::Open
}

/// Flips the highlighted boolean and saves it.
fn toggle(s: &mut SettingsUi, app: &mut App) {
    let Some(setting) = s.current().cloned() else { return };
    let now = settings::edit_text(&s.table, &setting) == "true";
    let value = if now { "false" } else { "true" };
    save(s, app, &setting, value);
}

/// Keys while a value is typed: text, Backspace, Left and Right to cycle choices, Enter to save, Escape
/// to cancel the edit.
fn edit_key(s: &mut SettingsUi, app: &mut App, ev: &KeyEvent) -> After {
    let Some(setting) = s.current().cloned() else {
        s.edit = None;
        return After::Open;
    };
    match ev.key {
        Key::Named(NamedKey::Escape) => {
            s.edit = None;
            s.message = None;
        }
        Key::Named(NamedKey::Enter) => {
            let text = s.edit.as_ref().map(|e| e.text.clone()).unwrap_or_default();
            if save(s, app, &setting, &text) {
                s.edit = None;
            }
        }
        _ => {
            if let Some(edit) = s.edit.as_mut() {
                type_into(edit, &setting, ev);
            }
        }
    }
    After::Open
}

/// Applies a typing key to the text being edited: Backspace deletes, Left and Right cycle the choices of
/// a setting that has a fixed set, and anything else that types text adds it.
fn type_into(edit: &mut Edit, setting: &Setting, ev: &KeyEvent) {
    match ev.key {
        Key::Named(NamedKey::Backspace) => {
            edit.text.pop();
        }
        Key::Named(NamedKey::Left) | Key::Named(NamedKey::Right) => {
            let forward = matches!(ev.key, Key::Named(NamedKey::Right));
            if let Some(next) = settings::cycle(setting, &edit.text, forward) {
                edit.text = next;
            }
        }
        _ => {
            if let Some(t) = typed(ev) {
                edit.text.push_str(t);
            }
        }
    }
}

/// Saves a value through `settings::save` and records the outcome in the status line. Returns whether
/// the value was written.
fn save(s: &mut SettingsUi, app: &mut App, setting: &Setting, input: &str) -> bool {
    match settings::save(app, setting, input) {
        Ok(()) => {
            s.table = settings::snapshot(&app.config);
            let shown = settings::display_value(&s.table, setting);
            s.message = Some(Message::Saved(format!("Saved {} = {shown}", setting.path())));
            true
        }
        Err(e) => {
            s.message = Some(Message::Error(e));
            false
        }
    }
}

/// One line of the list: a section header or a selectable row.
enum Line {
    Header(&'static str),
    Item(usize),
}

/// The list's lines for the matching rows: the open file row first, then each section's header before its
/// settings. The second value is the line the highlight is on.
fn layout(s: &SettingsUi, rows: &[Row]) -> (Vec<Line>, usize) {
    let mut lines = Vec::new();
    let mut at = 0;
    let mut last: Option<&'static str> = None;
    for (n, row) in rows.iter().enumerate() {
        if let Row::Setting(i) = row {
            let section = s.settings[*i].section;
            if last != Some(section) {
                lines.push(Line::Header(section));
                last = Some(section);
            }
        }
        if n == s.selected {
            at = lines.len();
        }
        lines.push(Line::Item(n));
    }
    (lines, at)
}

/// The settings view's content as escape sequences, and its number of rows. `max_rows` is the room the
/// window leaves for it; the view fills it so it covers the window.
pub fn settings_content(s: &SettingsUi, cols: u16, max_rows: u16, colors: &Colors) -> (Vec<u8>, u16) {
    let panel = mix(colors.background, colors.foreground, 0.08);
    let dim = mix(colors.foreground, colors.background, 0.4);
    let rows = s.matches();
    let (lines, at) = layout(s, &rows);
    let body = (max_rows as usize).saturating_sub(CHROME_ROWS).max(3);
    let first = (at + 1).saturating_sub(body);
    let mut out = String::new();
    out.push_str(&sgr_colors(colors.foreground, panel));
    out.push_str(&format!("\x1b[1m{}\x1b[22m\x1b[K", clip(&title(s, rows.len()), cols)));
    out.push_str(&format!("\r\n{}", status_line(s, cols, colors, panel)));
    for n in 0..body {
        out.push_str("\r\n");
        match lines.get(first + n) {
            Some(Line::Header(name)) => out.push_str(&header_line(name, cols, dim, panel)),
            Some(Line::Item(i)) => out.push_str(&item_line(s, &rows[*i], *i == s.selected, cols, colors, panel)),
            None => out.push_str(&format!("{}\x1b[K", sgr_colors(colors.foreground, panel))),
        }
    }
    out.push_str(&format!("\r\n{}", detail_line(s, &rows, cols, colors, panel)));
    out.push_str(&format!("\r\n{}", note_line(cols, dim, panel)));
    (out.into_bytes(), (body + CHROME_ROWS) as u16)
}

/// The first line: the filter, or the setting being edited with the text typed so far.
fn title(s: &SettingsUi, found: usize) -> String {
    match (&s.edit, s.current()) {
        (Some(e), Some(setting)) => format!(" Editing {}: {}\u{2588}", setting.path(), e.text),
        _ => format!(" Settings: {}\u{2588}   {} of {}", s.query, found, s.settings.len() + 1),
    }
}

/// The second line: an error or the last save, otherwise what the keys do.
fn status_line(s: &SettingsUi, cols: u16, colors: &Colors, panel: terminus_vt::style::Rgb) -> String {
    let (fg, text) = match (&s.message, &s.edit, s.current()) {
        (Some(Message::Error(e)), ..) => (terminus_vt::style::Rgb::new(255, 130, 130), format!(" {e}")),
        (Some(Message::Saved(m)), None, _) => (terminus_vt::style::Rgb::new(130, 220, 140), format!(" {m}")),
        (_, Some(_), Some(setting)) => {
            let cycle = if settings::choices(setting).is_some() { ", Left and Right change it" } else { "" };
            (colors.foreground, format!(" {}. Enter saves, Escape cancels{cycle}", setting.hint()))
        }
        _ => (colors.foreground, " Up and Down move, type to filter, Enter edits, Space toggles, Escape closes".into()),
    };
    format!("{}{}\x1b[K", sgr_colors(fg, panel), clip(&text, cols))
}

/// A section's header line.
fn header_line(name: &str, cols: u16, dim: terminus_vt::style::Rgb, panel: terminus_vt::style::Rgb) -> String {
    format!("{}\x1b[1m{}\x1b[22m\x1b[K", sgr_colors(dim, panel), clip(&format!(" [{name}]"), cols))
}

/// One row of the list: the path, the current value and the help text, highlighted when selected.
fn item_line(
    s: &SettingsUi, row: &Row, selected: bool, cols: u16, colors: &Colors, panel: terminus_vt::style::Rgb,
) -> String {
    let (fg, bg) = if selected { (colors.background, colors.foreground) } else { (colors.foreground, panel) };
    let text = match row {
        Row::OpenFile => "  Open the file  (opens the configuration file in your editor)".to_string(),
        Row::Setting(i) => {
            let setting = &s.settings[*i];
            let value = settings::display_value(&s.table, setting);
            let path = clip(&format!("  {}", setting.path()), PATH_COLS as u16);
            let value = clip(&value, VALUE_COLS as u16 - 1);
            format!("{path:<PATH_COLS$}{value:<VALUE_COLS$}{}", setting.doc)
        }
    };
    format!("{}{}\x1b[K", sgr_colors(fg, bg), clip(&text, cols))
}

/// The line under the list: the highlighted setting's full help text.
fn detail_line(s: &SettingsUi, rows: &[Row], cols: u16, colors: &Colors, panel: terminus_vt::style::Rgb) -> String {
    let text = match rows.get(s.selected) {
        Some(Row::Setting(i)) => format!(" {}: {}", s.settings[*i].path(), s.settings[*i].doc),
        Some(Row::OpenFile) => " Opens the configuration file, for settings this view does not edit.".to_string(),
        None => " No setting matches.".to_string(),
    };
    format!("{}{}\x1b[K", sgr_colors(colors.foreground, panel), clip(&text, cols))
}

/// The last line: the settings left to the file.
fn note_line(cols: u16, dim: terminus_vt::style::Rgb, panel: terminus_vt::style::Rgb) -> String {
    let names = settings::not_editable_names().join(", ");
    let text = format!(" Not edited here, use Open the file: {names}");
    format!("{}{}\x1b[K", sgr_colors(dim, panel), clip(&text, cols))
}
