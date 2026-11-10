//! The window's overlays that take the keyboard while open: the command palette and the search
//! bar. Each one turns keys into actions through `App::run_action`, so what they do is what the
//! same action does from a binding, the CLI or IPC.

use serde_json::{Map, Value};
use termoil_input::{Key, KeyEvent, Modifiers, NamedKey};
use termoil_protocol::{ActionCall, ParamDef, ParamKind};
use termoil_render::Colors;
use termoil_vt::style::Rgb;

use crate::answer::{Answer, Asked};
use crate::app::App;
use crate::gui::chrome::sgr_colors;
use crate::gui::look::mix;
use crate::model::PaneId;
use crate::palette::{Entry, filter};

/// Rows of matches the palette shows.
pub const PALETTE_ROWS: usize = 12;
/// The palette's width in columns, at most.
pub const PALETTE_COLS: u16 = 96;
/// Rows of an answer the overlay shows at once, below its title.
pub const ANSWER_ROWS: usize = 20;

/// An open overlay.
pub enum Overlay {
    /// The command palette.
    Palette(PaletteUi),
    /// The search bar for a pane.
    Search(SearchUi),
    /// Keyboard labels on a pane's links.
    Hints(HintsUi),
    /// The theme switcher, with the shader list beside it.
    Themes(ThemesUi),
    /// The settings view.
    Settings(crate::gui::settings_view::SettingsUi),
    /// What an action run from the window answered.
    Answer(AnswerUi),
}

/// The answer overlay's state.
pub struct AnswerUi {
    /// The answer shown.
    pub answer: Answer,
    /// The first line shown, for an answer longer than [`ANSWER_ROWS`].
    pub top: usize,
}

impl AnswerUi {
    /// An overlay showing `answer` from its first line.
    pub fn new(answer: Answer) -> AnswerUi {
        AnswerUi { answer, top: 0 }
    }

    /// The largest first line: the one that shows the last line at the bottom.
    fn last_top(&self) -> usize {
        self.answer.lines.len().saturating_sub(ANSWER_ROWS)
    }
}

/// Which list of the theme switcher takes the arrow keys and the typed filter.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ThemeList {
    /// The color themes, on the left.
    Themes,
    /// The built in shaders, on the right (task-2590).
    Shaders,
}

/// The theme switcher's state: the themes on the left and the shaders on the right, each with its
/// own filter and highlight, so a color theme and a shader are chosen together.
pub struct ThemesUi {
    /// The themes' filter text.
    pub query: String,
    /// The highlighted theme among the matches.
    pub selected: usize,
    /// The theme in effect when the switcher opened, restored by Escape.
    pub original: String,
    /// The list that takes the keys.
    pub list: ThemeList,
    /// The shaders' filter text.
    pub shader_query: String,
    /// The highlighted shader among the matches.
    pub shader_selected: usize,
    /// `window.shader` when the switcher opened, file path or name, restored by Escape.
    pub original_shader: String,
    /// A shader was highlighted with the keys, so Enter writes it. Until then a shader file set by
    /// path is left as it is.
    pub shader_touched: bool,
}

impl ThemesUi {
    /// A switcher opened while theme `original` and `window.shader` value `shader` are in effect, with
    /// both highlighted.
    pub fn new(original: String, shader: String) -> ThemesUi {
        let mut ui = ThemesUi {
            query: String::new(),
            selected: 0,
            original,
            list: ThemeList::Themes,
            shader_query: String::new(),
            shader_selected: 0,
            original_shader: shader,
            shader_touched: false,
        };
        ui.selected = ui.matches().iter().position(|n| n.eq_ignore_ascii_case(&ui.original)).unwrap_or(0);
        let current = crate::shaders::list_name(&ui.original_shader);
        ui.shader_selected = ui.shader_matches().iter().position(|n| Some(*n) == current).unwrap_or(0);
        ui
    }

    /// The shader names matching the shader filter: `none` first, then the built in shaders in their
    /// own order.
    pub fn shader_matches(&self) -> Vec<&'static str> {
        let q = self.shader_query.to_lowercase();
        let names = std::iter::once(crate::shaders::NONE).chain(crate::shaders::BUILTINS.iter().map(|b| b.name));
        names.filter(|n| q.is_empty() || n.contains(&q)).collect()
    }

    /// The bundled theme names matching the query: the themes Unluminous ships first, in their own
    /// order, then the rest by name.
    pub fn matches(&self) -> Vec<&'static str> {
        let q = self.query.to_lowercase();
        let hit = |n: &str| q.is_empty() || n.to_lowercase().contains(&q);
        let featured = crate::config::featured_themes();
        let rest = crate::config::themes().iter().map(|t| t.name.as_str()).filter(|n| !featured.contains(n));
        featured.iter().copied().chain(rest).filter(|n| hit(n)).collect()
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
    /// The action whose required arguments are being typed.
    asking: Option<Asking>,
}

/// The palette asking for an action's required arguments, one after another.
struct Asking {
    /// The action.
    action: &'static str,
    /// The required parameters not yet typed; the first is the one being asked for.
    left: Vec<ParamDef>,
    /// The arguments typed so far.
    args: Map<String, Value>,
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

    /// The entries matching the query, best first, as the palette lists them; `selected` indexes it.
    pub fn visible(&self) -> Vec<&Entry> {
        self.matches()
    }
}

/// The text a key types, when it types any and no control or alt is held.
pub(crate) fn typed(ev: &KeyEvent) -> Option<&str> {
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
        Overlay::Settings(s) => crate::gui::settings_view::key(s, app, ev),
        Overlay::Answer(a) => answer_key(a, ev),
    }
}

/// Answer keys: Up and Down scroll an answer longer than the overlay; any other key closes it.
fn answer_key(a: &mut AnswerUi, ev: &KeyEvent) -> After {
    match ev.key {
        Key::Named(NamedKey::Up) => a.top = a.top.saturating_sub(1),
        Key::Named(NamedKey::Down) => a.top = (a.top + 1).min(a.last_top()),
        Key::Named(NamedKey::PageUp) => a.top = a.top.saturating_sub(ANSWER_ROWS),
        Key::Named(NamedKey::PageDown) => a.top = (a.top + ANSWER_ROWS).min(a.last_top()),
        _ => return After::Close,
    }
    After::Open
}

/// Theme keys: Left, Right and Tab switch between the themes and the shaders; typing filters the
/// list in use; the arrows move and preview; Enter keeps and saves the theme and the shader;
/// Escape restores both.
fn themes_key(t: &mut ThemesUi, app: &mut App, ev: &KeyEvent) -> After {
    match ev.key {
        Key::Named(NamedKey::Escape) => {
            let _ = crate::themes::preview(app, &t.original);
            crate::shaders::restore(app, &t.original_shader);
            return After::Close;
        }
        Key::Named(NamedKey::Enter) => {
            themes_enter(t, app);
            return After::Close;
        }
        Key::Named(NamedKey::Left) => t.list = ThemeList::Themes,
        Key::Named(NamedKey::Right) => t.list = ThemeList::Shaders,
        Key::Named(NamedKey::Tab) => {
            t.list = if t.list == ThemeList::Themes { ThemeList::Shaders } else { ThemeList::Themes };
        }
        _ => {
            if !list_key(t, ev) {
                return After::Open;
            }
        }
    }
    themes_preview(t, app);
    After::Open
}

/// Up, Down, Backspace and typed text for the list in use. False when the key does nothing.
fn list_key(t: &mut ThemesUi, ev: &KeyEvent) -> bool {
    let len = match t.list {
        ThemeList::Themes => t.matches().len(),
        ThemeList::Shaders => t.shader_matches().len(),
    };
    let (query, selected) = match t.list {
        ThemeList::Themes => (&mut t.query, &mut t.selected),
        ThemeList::Shaders => (&mut t.shader_query, &mut t.shader_selected),
    };
    match ev.key {
        Key::Named(NamedKey::Up) => *selected = selected.saturating_sub(1),
        Key::Named(NamedKey::Down) => *selected = (*selected + 1).min(len.saturating_sub(1)),
        Key::Named(NamedKey::Backspace) => {
            query.pop();
            *selected = 0;
        }
        _ => match typed(ev) {
            Some(s) => {
                query.push_str(s);
                *selected = 0;
            }
            None => return false,
        },
    }
    if t.list == ThemeList::Shaders {
        t.shader_touched = true;
    }
    true
}

/// Shows the highlighted theme, and the highlighted shader once one was chosen with the keys.
fn themes_preview(t: &ThemesUi, app: &mut App) {
    if let Some(name) = t.matches().get(t.selected) {
        let _ = crate::themes::preview(app, name);
    }
    if t.shader_touched
        && let Some(name) = t.shader_matches().get(t.shader_selected)
    {
        let _ = crate::shaders::preview(app, name);
    }
}

/// Enter: saves the highlighted theme, and the highlighted shader when one was chosen with the keys.
fn themes_enter(t: &ThemesUi, app: &mut App) {
    if let Some(name) = t.matches().get(t.selected) {
        let _ = crate::themes::set(app, name);
    }
    if t.shader_touched
        && let Some(name) = t.shader_matches().get(t.shader_selected)
    {
        let _ = crate::shaders::set(app, name);
    }
}

/// The theme switcher's columns: the theme list's width, and the shader list's width after a one
/// column gap. Below 48 columns the shader list is left out.
fn theme_columns(cols: u16) -> (u16, u16) {
    if cols < 48 {
        return (cols, 0);
    }
    let shaders = (cols / 3).clamp(18, 34);
    (cols - shaders - 1, shaders)
}

/// The theme switcher's content and its number of rows: a header with both filters, the themes on
/// the left and the shaders on the right, and a line saying how to use them.
pub fn themes_content(t: &ThemesUi, cols: u16, colors: &Colors) -> (Vec<u8>, u16) {
    let panel = mix(colors.background, colors.foreground, 0.08);
    let dim = mix(colors.foreground, panel, 0.45);
    let (left_w, right_w) = theme_columns(cols);
    let themes = t.matches();
    let shaders = t.shader_matches();
    let cursor = |list| if t.list == list { "\u{2588}" } else { "" };
    let count = crate::config::themes().len();
    let left = format!(" Theme: {}{}   {} of {count}", t.query, cursor(ThemeList::Themes), themes.len());
    let right = format!(" Shader: {}{}", t.shader_query, cursor(ThemeList::Shaders));
    let mut out = format!("{}\x1b[1m{}", sgr_colors(colors.foreground, panel), pad(&left, left_w));
    if right_w > 0 {
        out.push_str(&format!(" {}", pad(&right, right_w)));
    }
    out.push_str("\x1b[22m\x1b[K");
    let theme_top = t.selected.saturating_sub(PALETTE_ROWS - 1);
    let shader_top = t.shader_selected.saturating_sub(PALETTE_ROWS - 1);
    let shown = if right_w > 0 { themes.len().max(shaders.len()) } else { themes.len() };
    let mut rows = 1u16;
    for row in 0..shown.min(PALETTE_ROWS) {
        let theme = theme_cell(t, &themes, theme_top + row, left_w, colors, panel, dim);
        out.push_str(&format!("\r\n{theme}"));
        if right_w > 0 {
            let shader = shader_cell(t, &shaders, shader_top + row, right_w, colors, panel);
            out.push_str(&format!("{} {shader}", sgr_colors(colors.foreground, panel)));
        }
        out.push_str("\x1b[K");
        rows += 1;
    }
    out.push_str(&format!("\r\n{}{}\x1b[K", sgr_colors(dim, panel), clip(&themes_hint(t, right_w > 0), cols)));
    (out.into_bytes(), rows + 1)
}

/// One theme row, `width` columns wide: the name, and `from Unluminous` for the featured themes. The
/// highlighted theme is drawn inverted while the theme list has the keys, and marked otherwise.
fn theme_cell(t: &ThemesUi, themes: &[&str], i: usize, width: u16, colors: &Colors, panel: Rgb, dim: Rgb) -> String {
    let Some(name) = themes.get(i) else {
        return format!("{}{}", sgr_colors(colors.foreground, panel), pad("", width));
    };
    let active = i == t.selected && t.list == ThemeList::Themes;
    let (fg, bg) = if active { (colors.background, colors.foreground) } else { (colors.foreground, panel) };
    let mark = if i == t.selected { "\u{203a} " } else { "  " };
    let label = clip(&format!("{mark}{name}"), width);
    let tag = if crate::config::featured_themes().contains(name) { "  from Unluminous" } else { "" };
    let room = width.saturating_sub(label.chars().count() as u16);
    let tag_fg = if active { mix(colors.background, colors.foreground, 0.35) } else { dim };
    format!("{}{label}{}{}", sgr_colors(fg, bg), sgr_colors(tag_fg, bg), pad(tag, room))
}

/// One shader row, `width` columns wide. The highlighted shader is drawn inverted while the shader
/// list has the keys, and marked otherwise.
fn shader_cell(t: &ThemesUi, shaders: &[&str], i: usize, width: u16, colors: &Colors, panel: Rgb) -> String {
    let Some(name) = shaders.get(i) else {
        return format!("{}{}", sgr_colors(colors.foreground, panel), pad("", width));
    };
    let active = i == t.shader_selected && t.list == ThemeList::Shaders;
    let (fg, bg) = if active { (colors.background, colors.foreground) } else { (colors.foreground, panel) };
    let mark = if i == t.shader_selected { "\u{203a} " } else { "  " };
    format!("{}{}", sgr_colors(fg, bg), pad(&format!("{mark}{name}"), width))
}

/// The switcher's last line: what the highlighted shader looks like while the shader list has the
/// keys, otherwise how to use the switcher.
fn themes_hint(t: &ThemesUi, with_shaders: bool) -> String {
    let highlighted = t.shader_matches().get(t.shader_selected).and_then(|n| crate::shaders::builtin(n));
    match highlighted {
        Some(b) if t.list == ThemeList::Shaders => format!(" {}: {}", b.name, b.summary),
        _ if with_shaders => {
            " Left and Right switch lists, Up and Down preview, Enter keeps both, Escape restores".to_string()
        }
        _ => " Up and Down preview, Enter keeps, Escape restores".to_string(),
    }
}

/// Text cut or filled with spaces to exactly `cols` characters.
fn pad(s: &str, cols: u16) -> String {
    let mut text = clip(s, cols);
    let short = (cols as usize).saturating_sub(text.chars().count());
    text.push_str(&" ".repeat(short));
    text
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

/// Enter in the palette: run the highlighted action, ask for its required arguments one after another,
/// or store the argument just typed and ask for the next one or run the action.
fn palette_enter(p: &mut PaletteUi, app: &mut App) -> After {
    if let Some(mut asking) = p.asking.take() {
        let param = asking.left.remove(0);
        asking.args.insert(param.name.to_string(), typed_value(param.kind, &p.query));
        p.query.clear();
        if asking.left.is_empty() {
            let call = ActionCall { action: asking.action.to_string(), args: asking.args };
            let _ = app.run_for_person(&call, Asked::Palette);
            return After::Close;
        }
        p.asking = Some(asking);
        return After::Open;
    }
    let Some(e) = p.matches().get(p.selected).map(|e| (*e).clone()) else { return After::Open };
    let args: Map<String, Value> = e.preset.and_then(|p| serde_json::from_str(p).ok()).unwrap_or_default();
    let def = termoil_protocol::action(e.action).expect("palette entries come from the catalogue");
    let left: Vec<ParamDef> = def.params.iter().filter(|x| x.required).copied().collect();
    if !left.is_empty() {
        p.asking = Some(Asking { action: e.action, left, args });
        p.query.clear();
        return After::Open;
    }
    let _ = app.run_for_person(&ActionCall { action: e.action.to_string(), args }, Asked::Palette);
    After::Close
}

/// The JSON value of an argument typed into the palette: a number, a boolean or the text.
fn typed_value(kind: ParamKind, text: &str) -> Value {
    match kind {
        ParamKind::Int | ParamKind::Pane => text.trim().parse::<i64>().map(Value::from).unwrap_or(Value::Null),
        ParamKind::Bool => Value::Bool(matches!(text.trim(), "true" | "yes" | "1")),
        _ => Value::String(text.to_string()),
    }
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
        crate::app::Outcome::Wait(..) | crate::app::Outcome::Later(_) => {}
    }
}

/// The palette's content as escape sequences, and its number of rows.
pub fn palette_content(p: &PaletteUi, cols: u16, colors: &Colors) -> (Vec<u8>, u16) {
    let panel = mix(colors.background, colors.foreground, 0.08);
    let dim = mix(colors.foreground, colors.background, 0.4);
    let mut out = String::new();
    let prompt = match &p.asking {
        Some(a) => format!(" {} --{}: {}\u{2588}", a.action, a.left[0].name, p.query),
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

/// The answer overlay's content and its number of rows: the title in bold, red for a failure, then the
/// lines that fit, then a dim line saying how to scroll or close.
pub fn answer_content(a: &AnswerUi, cols: u16, colors: &Colors) -> (Vec<u8>, u16) {
    let panel = mix(colors.background, colors.foreground, 0.08);
    let dim = mix(colors.foreground, panel, 0.4);
    let title_fg = if a.answer.failed { colors.palette[1] } else { colors.foreground };
    let ellipsis = if a.answer.pending { "\u{2026}" } else { "" };
    let title = clip(&format!(" {}{ellipsis}", a.answer.title), cols);
    let mut out = format!("{}\x1b[1m{title}\x1b[22m\x1b[K", sgr_colors(title_fg, panel));
    let mut rows = 1u16;
    for line in a.answer.lines.iter().skip(a.top).take(ANSWER_ROWS) {
        let text = clip(&format!(" {line}"), cols);
        out.push_str(&format!("\r\n{}{text}\x1b[K", sgr_colors(colors.foreground, panel)));
        rows += 1;
    }
    out.push_str(&format!("\r\n{}{}\x1b[K", sgr_colors(dim, panel), clip(&answer_hint(a), cols)));
    (out.into_bytes(), rows + 1)
}

/// The answer overlay's last line: which lines are shown when they do not all fit, and how to close it.
fn answer_hint(a: &AnswerUi) -> String {
    let total = a.answer.lines.len();
    if total > ANSWER_ROWS {
        let last = (a.top + ANSWER_ROWS).min(total);
        format!(" lines {} to {last} of {total}; Up and Down scroll, any other key closes", a.top + 1)
    } else {
        " any key closes".to_string()
    }
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
pub(crate) fn clip(s: &str, cols: u16) -> String {
    s.chars().take(cols as usize).collect()
}
