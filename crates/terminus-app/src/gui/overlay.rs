//! The window's overlays that take the keyboard while open: the command palette and the search
//! bar. Each one turns keys into actions through `App::run_action`, so what they do is what the
//! same action does from a binding, the CLI or IPC.

use serde_json::{Map, Value};
use terminus_input::{Key, KeyEvent, Modifiers, NamedKey};
use terminus_protocol::{ActionCall, ParamKind};
use terminus_render::Colors;

use crate::app::App;
use crate::gui::chrome::sgr_colors;
use crate::gui::look::mix;
use crate::model::PaneId;
use crate::palette::{Entry, filter};

/// Rows of matches the palette shows.
pub const PALETTE_ROWS: usize = 12;
/// The palette's width in columns, at most.
pub const PALETTE_COLS: u16 = 96;

/// An open overlay.
pub enum Overlay {
    /// The command palette.
    Palette(PaletteUi),
    /// The search bar for a pane.
    Search(SearchUi),
    /// Keyboard labels on a pane's links.
    Hints(HintsUi),
    /// The theme switcher.
    Themes(ThemesUi),
}

/// The theme switcher's state.
pub struct ThemesUi {
    /// The filter text.
    pub query: String,
    /// The highlighted theme among the matches.
    pub selected: usize,
    /// The theme in effect when the switcher opened, restored by Escape.
    pub original: String,
}

impl ThemesUi {
    /// The bundled theme names matching the query.
    fn matches(&self) -> Vec<&'static str> {
        let q = self.query.to_lowercase();
        crate::config::themes()
            .iter()
            .map(|t| t.name.as_str())
            .filter(|n| q.is_empty() || n.to_lowercase().contains(&q))
            .collect()
    }
}

/// The hint labels' state.
pub struct HintsUi {
    /// The pane the links are in.
    pub pane: PaneId,
    /// Each label with its link and where it starts (absolute row, column).
    pub items: Vec<(String, crate::links::Link, usize, u16)>,
    /// The letters typed so far.
    pub typed: String,
}

/// The palette's state.
pub struct PaletteUi {
    /// The filter text, or the argument being typed.
    pub query: String,
    /// The highlighted match.
    pub selected: usize,
    entries: Vec<Entry>,
    /// The action and parameter an argument is being typed for.
    asking: Option<(&'static str, &'static str, ParamKind)>,
}

/// The search bar's state.
pub struct SearchUi {
    /// The pattern.
    pub query: String,
    /// Match case.
    pub case_sensitive: bool,
    /// The pane searched.
    pub pane: PaneId,
    /// Whether the last search found a match.
    pub found: Option<bool>,
    /// The last error (a pattern that does not compile).
    pub error: Option<String>,
}

/// What a key did to an overlay.
#[derive(PartialEq, Eq)]
pub enum After {
    /// The overlay stays open.
    Open,
    /// The overlay closes.
    Close,
}

impl PaletteUi {
    /// A palette over these entries.
    pub fn new(entries: Vec<Entry>) -> PaletteUi {
        PaletteUi { query: String::new(), selected: 0, entries, asking: None }
    }

    /// The entries matching the query.
    fn matches(&self) -> Vec<&Entry> {
        filter(&self.entries, &self.query)
    }
}

/// The text a key types, when it types any and no control or alt is held.
fn typed(ev: &KeyEvent) -> Option<&str> {
    if ev.mods.contains(Modifiers::CTRL) || ev.mods.contains(Modifiers::ALT) {
        return None;
    }
    ev.text.as_deref().filter(|t| !t.is_empty() && !t.chars().any(char::is_control))
}

/// Handles a key in an overlay.
pub fn key(ov: &mut Overlay, app: &mut App, ev: &KeyEvent) -> After {
    match ov {
        Overlay::Palette(p) => palette_key(p, app, ev),
        Overlay::Search(s) => search_key(s, app, ev),
        Overlay::Hints(h) => hints_key(h, app, ev),
        Overlay::Themes(t) => themes_key(t, app, ev),
    }
}

/// Theme keys: type to filter, arrows move and preview, Enter keeps and saves, Escape restores.
fn themes_key(t: &mut ThemesUi, app: &mut App, ev: &KeyEvent) -> After {
    match ev.key {
        Key::Named(NamedKey::Escape) => {
            let _ = crate::themes::preview(app, &t.original);
            return After::Close;
        }
        Key::Named(NamedKey::Enter) => {
            if let Some(name) = t.matches().get(t.selected) {
                let _ = crate::themes::set(app, name);
            }
            return After::Close;
        }
        Key::Named(NamedKey::Up) => t.selected = t.selected.saturating_sub(1),
        Key::Named(NamedKey::Down) => t.selected = (t.selected + 1).min(t.matches().len().saturating_sub(1)),
        Key::Named(NamedKey::Backspace) => {
            t.query.pop();
            t.selected = 0;
        }
        _ => match typed(ev) {
            Some(s) => {
                t.query.push_str(s);
                t.selected = 0;
            }
            None => return After::Open,
        },
    }
    if let Some(name) = t.matches().get(t.selected) {
        let _ = crate::themes::preview(app, name);
    }
    After::Open
}

/// The theme switcher's content and its number of rows.
pub fn themes_content(t: &ThemesUi, cols: u16, colors: &Colors) -> (Vec<u8>, u16) {
    let panel = mix(colors.background, colors.foreground, 0.08);
    let matches = t.matches();
    let mut out = format!(
        "{}\x1b[1m{}\x1b[22m\x1b[K",
        sgr_colors(colors.foreground, panel),
        clip(&format!(" Theme: {}\u{2588}   {} of {}", t.query, matches.len(), crate::config::themes().len()), cols)
    );
    let first = t.selected.saturating_sub(PALETTE_ROWS - 1);
    let mut rows = 1u16;
    for (i, name) in matches.iter().enumerate().skip(first).take(PALETTE_ROWS) {
        let (fg, bg) =
            if i == t.selected { (colors.background, colors.foreground) } else { (colors.foreground, panel) };
        out.push_str(&format!("\r\n{}  {}\x1b[K", sgr_colors(fg, bg), clip(name, cols.saturating_sub(2))));
        rows += 1;
    }
    (out.into_bytes(), rows)
}

/// Hint keys: letters narrow the labels and a full label opens its link; anything else closes.
fn hints_key(h: &mut HintsUi, app: &mut App, ev: &KeyEvent) -> After {
    let Some(t) = typed(ev) else { return After::Close };
    h.typed.push_str(&t.to_lowercase());
    if let Some((_, link, _, _)) = h.items.iter().find(|(l, ..)| *l == h.typed) {
        let p = app.panes.get(&h.pane);
        let cwd = p
            .and_then(|p| p.cwd.as_deref())
            .and_then(crate::app::cwd_path)
            .or_else(|| p.and_then(|p| p.session.start_cwd().cloned()));
        let opener = if matches!(link, crate::links::Link::Url(_)) {
            String::new()
        } else {
            app.config.openers.path_command(crate::platform::on_path("unluminous"))
        };
        crate::links::open(link, &opener, cwd.as_deref());
        return After::Close;
    }
    if h.items.iter().any(|(l, ..)| l.starts_with(&h.typed)) { After::Open } else { After::Close }
}

/// A hint label drawn over its link: dark on yellow, with the letters typed so far dimmed.
pub fn hint_content(label: &str, typed: &str) -> Vec<u8> {
    let done = typed.len().min(label.len());
    format!("\x1b[30;48;2;250;210;80;2m{}\x1b[22;1m{}", &label[..done], &label[done..]).into_bytes()
}

/// Palette keys: type to filter, arrows to move, Enter to run (or to ask for a required
/// argument first), Escape to close.
fn palette_key(p: &mut PaletteUi, app: &mut App, ev: &KeyEvent) -> After {
    match ev.key {
        Key::Named(NamedKey::Escape) => return After::Close,
        Key::Named(NamedKey::Up) => p.selected = p.selected.saturating_sub(1),
        Key::Named(NamedKey::Down) => p.selected = (p.selected + 1).min(p.matches().len().saturating_sub(1)),
        Key::Named(NamedKey::Backspace) => {
            p.query.pop();
            p.selected = 0;
        }
        Key::Named(NamedKey::Enter) => return palette_enter(p, app),
        _ => match typed(ev) {
            Some(t) => {
                p.query.push_str(t);
                p.selected = 0;
            }
            None => return After::Open,
        },
    }
    After::Open
}

/// Enter in the palette: run the highlighted action, ask for its required argument, or run the
/// action with the argument just typed.
fn palette_enter(p: &mut PaletteUi, app: &mut App) -> After {
    if let Some((action, param, kind)) = p.asking {
        let mut args = Map::new();
        let value = match kind {
            ParamKind::Int | ParamKind::Pane => p.query.trim().parse::<i64>().map(Value::from).unwrap_or(Value::Null),
            ParamKind::Bool => Value::Bool(matches!(p.query.trim(), "true" | "yes" | "1")),
            _ => Value::String(p.query.clone()),
        };
        args.insert(param.to_string(), value);
        let _ = app.run_action(&ActionCall { action: action.to_string(), args });
        return After::Close;
    }
    let Some(e) = p.matches().get(p.selected).map(|e| (*e).clone()) else { return After::Open };
    if e.needs_args {
        let def = terminus_protocol::action(e.action).expect("palette entries come from the catalogue");
        if let Some(param) = def.params.iter().find(|x| x.required) {
            p.asking = Some((e.action, param.name, param.kind));
            p.query.clear();
            return After::Open;
        }
    }
    let args = e.preset.and_then(|p| serde_json::from_str(p).ok()).unwrap_or_default();
    let _ = app.run_action(&ActionCall { action: e.action.to_string(), args });
    After::Close
}

/// Search keys: type to search as you go, Enter for the next older match, Shift+Enter for the next
/// newer one, Alt+C to toggle case, Escape to close.
fn search_key(s: &mut SearchUi, app: &mut App, ev: &KeyEvent) -> After {
    let shift = ev.mods.contains(Modifiers::SHIFT);
    match ev.key {
        Key::Named(NamedKey::Escape) => return After::Close,
        Key::Named(NamedKey::Enter) => {
            run_search(s, app, if shift { "search-next" } else { "search-previous" }, false);
            return After::Open;
        }
        Key::Named(NamedKey::Backspace) => {
            s.query.pop();
        }
        Key::Char('c') if ev.mods.contains(Modifiers::ALT) => s.case_sensitive = !s.case_sensitive,
        _ => match typed(ev) {
            Some(t) => s.query.push_str(t),
            None => return After::Open,
        },
    }
    run_search(s, app, "search", true);
    After::Open
}

/// Runs a search action for the bar's pane and records the outcome.
fn run_search(s: &mut SearchUi, app: &mut App, action: &str, with_pattern: bool) {
    if s.query.is_empty() {
        s.found = None;
        s.error = None;
        return;
    }
    let mut args = Map::new();
    args.insert("pane".into(), s.pane.into());
    if with_pattern {
        args.insert("pattern".into(), Value::String(s.query.clone()));
        args.insert("case-sensitive".into(), Value::Bool(s.case_sensitive));
    }
    match app.run_action(&ActionCall { action: action.into(), args }) {
        crate::app::Outcome::Done(Ok(v)) => {
            s.found = v.get("found").and_then(Value::as_bool);
            s.error = None;
        }
        crate::app::Outcome::Done(Err(e)) => s.error = Some(e.to_string()),
        crate::app::Outcome::Wait(..) => {}
    }
}

/// The palette's content as escape sequences, and its number of rows.
pub fn palette_content(p: &PaletteUi, cols: u16, colors: &Colors) -> (Vec<u8>, u16) {
    let panel = mix(colors.background, colors.foreground, 0.08);
    let dim = mix(colors.foreground, colors.background, 0.4);
    let mut out = String::new();
    let prompt = match p.asking {
        Some((action, param, _)) => format!(" {action} --{param}: {}\u{2588}", p.query),
        None => format!(" > {}\u{2588}", p.query),
    };
    out.push_str(&format!("{}\x1b[1m{}\x1b[22m\x1b[K", sgr_colors(colors.foreground, panel), clip(&prompt, cols)));
    let mut rows = 1u16;
    if p.asking.is_none() {
        let matches = p.matches();
        let first = p.selected.saturating_sub(PALETTE_ROWS - 1);
        for (i, e) in matches.iter().enumerate().skip(first).take(PALETTE_ROWS) {
            let (fg, bg) =
                if i == p.selected { (colors.background, colors.foreground) } else { (colors.foreground, panel) };
            let keys = if e.keys.is_empty() { String::new() } else { format!("  {}", e.keys) };
            let line = format!("  {:<28}{}", e.action, e.summary);
            let room = (cols as usize).saturating_sub(keys.chars().count());
            out.push_str(&format!("\r\n{}{}\x1b[K", sgr_colors(fg, bg), clip(&line, room as u16)));
            if !keys.is_empty() {
                let at = cols as usize - keys.chars().count() + 1;
                let key_fg = if i == p.selected { fg } else { dim };
                out.push_str(&format!("\x1b[{at}G{}{keys}", sgr_colors(key_fg, bg)));
            }
            rows += 1;
        }
    }
    (out.into_bytes(), rows)
}

/// The search bar's content.
pub fn search_content(s: &SearchUi, cols: u16, colors: &Colors) -> Vec<u8> {
    let panel = mix(colors.background, colors.foreground, 0.08);
    let status = match (&s.error, s.found) {
        (Some(e), _) => e.clone(),
        (None, Some(true)) => "Enter older, Shift+Enter newer".into(),
        (None, Some(false)) => "no match".into(),
        (None, None) => String::new(),
    };
    let case = if s.case_sensitive { "[Aa]" } else { "[aa]" };
    let line = format!(" Find: {}\u{2588}   {case} Alt+C   {status}", s.query);
    format!("{}{}\x1b[K", sgr_colors(colors.foreground, panel), clip(&line, cols)).into_bytes()
}

/// Cuts text to `cols` characters.
fn clip(s: &str, cols: u16) -> String {
    s.chars().take(cols as usize).collect()
}
