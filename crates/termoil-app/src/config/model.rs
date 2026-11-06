//! The typed configuration: one struct per `[section]` of the config file, each with a `Default`
//! that is the value used when the file does not set the key.
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use std::collections::BTreeMap;

use super::keys::Binding;

/// An sRGB colour written in the file as `"#rrggbb"` (`"rrggbb"` and `"#rgb"` are also accepted).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Rgb(pub [u8; 3]);

/// Parses `#rrggbb`, `rrggbb`, `#rgb` or `rgb` into a colour.
pub fn parse_hex_color(text: &str) -> Result<Rgb, String> {
    let digits = text.trim().trim_start_matches('#');
    let bad = || format!("`{text}` is not a colour, use \"#rrggbb\"");
    if !digits.chars().all(|c| c.is_ascii_hexdigit()) {
        return Err(bad());
    }
    let value = u32::from_str_radix(digits, 16).map_err(|_| bad())?;
    match digits.len() {
        6 => Ok(Rgb([(value >> 16) as u8, (value >> 8) as u8, value as u8])),
        3 => {
            let expand = |n: u32| ((n & 0xf) * 17) as u8;
            Ok(Rgb([expand(value >> 8), expand(value >> 4), expand(value)]))
        }
        _ => Err(bad()),
    }
}

impl Serialize for Rgb {
    /// Writes the colour as a `"#rrggbb"` string.
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let [r, g, b] = self.0;
        serializer.serialize_str(&format!("#{r:02x}{g:02x}{b:02x}"))
    }
}

impl<'de> Deserialize<'de> for Rgb {
    /// Reads a colour from a hex string, reporting a readable error for anything else.
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let text = String::deserialize(deserializer)?;
        parse_hex_color(&text).map_err(serde::de::Error::custom)
    }
}

/// The whole configuration. Every section is optional in the file.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, Default)]
#[serde(default)]
pub struct Config {
    pub font: Font,
    pub theme: ThemeSel,
    pub colors: ColorOverrides,
    pub cursor: Cursor,
    pub window: Window,
    pub tab_bar: TabBar,
    pub scrollback: Scrollback,
    pub session: SessionRestore,
    pub shell: Shell,
    pub clipboard: Clipboard,
    pub notify: Notify,
    pub accessibility: Accessibility,
    pub mouse: Mouse,
    pub openers: Openers,
    pub keybindings: Keybindings,
    pub updates: Updates,
    pub log: Log,
}

/// `[font]`: the face chain, size and line spacing.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Font {
    pub family: Vec<String>,
    pub size: f64,
    pub ligatures: bool,
    pub ligatures_off: Vec<String>,
    pub hinting: bool,
    pub line_height: f64,
    pub fallback: Vec<String>,
}

impl Default for Font {
    /// The first installed of Cascadia Code, JetBrains Mono, Fira Code, Menlo, DejaVu Sans Mono, at 13 points.
    fn default() -> Self {
        let family = ["Cascadia Code", "JetBrains Mono", "Fira Code", "Menlo", "DejaVu Sans Mono"];
        Font {
            family: family.map(String::from).to_vec(),
            size: 13.0,
            ligatures: true,
            ligatures_off: vec![],
            hinting: true,
            line_height: 1.0,
            fallback: vec![],
        }
    }
}

/// `[theme]`: which colour scheme to use. The file may also write `theme = "Name"` for `theme.name`.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct ThemeSel {
    pub name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub dark: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub light: Option<String>,
}

impl Default for ThemeSel {
    /// Unluminous Dark, with no separate dark and light choice.
    fn default() -> Self {
        ThemeSel { name: super::DEFAULT_THEME.into(), dark: None, light: None }
    }
}

/// The table form of `[theme]`, with every key optional.
#[derive(Deserialize, Default)]
#[serde(default)]
struct ThemeSelTable {
    name: Option<String>,
    dark: Option<String>,
    light: Option<String>,
}

/// Either `theme = "Name"` or the `[theme]` table.
#[derive(Deserialize)]
#[serde(untagged)]
enum ThemeSelRaw {
    Name(String),
    Table(ThemeSelTable),
}

impl<'de> Deserialize<'de> for ThemeSel {
    /// Accepts a bare string as the theme name, or a table with `name`, `dark` and `light`.
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let default = ThemeSel::default();
        Ok(match ThemeSelRaw::deserialize(deserializer)? {
            ThemeSelRaw::Name(name) => ThemeSel { name, ..default },
            ThemeSelRaw::Table(t) => ThemeSel { name: t.name.unwrap_or(default.name), dark: t.dark, light: t.light },
        })
    }
}

/// `[colors]`: overrides applied on top of the chosen theme.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, Default)]
#[serde(default)]
pub struct ColorOverrides {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub foreground: Option<Rgb>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub background: Option<Rgb>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cursor: Option<Rgb>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub selection_background: Option<Rgb>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub selection_foreground: Option<Rgb>,
    pub palette: Vec<Rgb>,
}

/// The cursor shape.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum CursorStyle {
    Block,
    Bar,
    Underline,
}

/// `[tab_bar]`: the sizes of the tab bar, in pixels at a display scale of 1 and in columns.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct TabBar {
    pub padding: u32,
    pub corner_radius: f32,
    pub inset: f32,
    pub underline: f32,
    pub min_columns: u16,
    pub max_columns: u16,
    pub show_with_one_tab: bool,
    pub bar_shade: f32,
}

impl Default for TabBar {
    /// Tabs 12 to 26 columns wide with rounded top corners and a two pixel accent line, on a bar 30% darker
    /// than the terminal, and no bar at all while the window has one tab (task-2487).
    fn default() -> Self {
        TabBar {
            padding: 8,
            corner_radius: 8.0,
            inset: 5.0,
            underline: 2.0,
            min_columns: 12,
            max_columns: 26,
            show_with_one_tab: false,
            bar_shade: 0.3,
        }
    }
}

/// `[cursor]`: shape and blinking.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Cursor {
    pub style: CursorStyle,
    pub blink: bool,
    pub blink_interval_ms: u64,
}

impl Default for Cursor {
    /// A steady block.
    fn default() -> Self {
        Cursor { style: CursorStyle::Block, blink: false, blink_interval_ms: 600 }
    }
}

/// Which renderer draws the window.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Renderer {
    Auto,
    Gpu,
    Cpu,
}

/// The screen edge the quick terminal slides from.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Edge {
    Top,
    Bottom,
    Left,
    Right,
}

/// `[window]`: size, chrome, rendering and the quick terminal.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Window {
    pub padding: [u32; 2],
    pub opacity: f64,
    pub blur: bool,
    pub decorations: bool,
    pub columns: u32,
    pub rows: u32,
    pub minimum_contrast: f64,
    pub renderer: Renderer,
    pub title_follows_cwd: bool,
    pub show_tab_bar: bool,
    pub palette_hint: bool,
    pub quick_terminal_edge: Edge,
    pub quick_terminal_hotkey: String,
    pub shader: String,
    pub shader_poll_ms: u64,
    pub repaint_retry_ms: u64,
    pub repaint_retry_max_ms: u64,
    pub output_fps: u32,
    pub input_grace_ms: u64,
}

impl Default for Window {
    /// 120 by 34 cells, opaque, with the 4.5:1 minimum contrast the PRD asks for.
    fn default() -> Self {
        Window {
            padding: [4, 4],
            opacity: 1.0,
            blur: false,
            decorations: true,
            columns: 120,
            rows: 34,
            minimum_contrast: 4.5,
            renderer: Renderer::Auto,
            title_follows_cwd: true,
            show_tab_bar: true,
            palette_hint: true,
            quick_terminal_edge: Edge::Top,
            quick_terminal_hotkey: "ctrl+`".into(),
            shader: String::new(),
            shader_poll_ms: 1000,
            repaint_retry_ms: 16,
            repaint_retry_max_ms: 1000,
            output_fps: 60,
            input_grace_ms: 250,
        }
    }
}

/// `[scrollback]`: how much history each pane keeps.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Scrollback {
    pub limit_bytes: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub limit_lines: Option<u64>,
}

impl Default for Scrollback {
    /// Fifty million bytes of history and no line limit.
    fn default() -> Self {
        Scrollback { limit_bytes: 50_000_000, limit_lines: None }
    }
}

/// `[session]`: saving windows, tabs, splits and history on exit and reopening them on start.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct SessionRestore {
    pub restore: bool,
    pub scrollback_lines: u64,
}

impl Default for SessionRestore {
    /// Restore off, and 2000 lines of history saved per pane when it is on.
    fn default() -> Self {
        SessionRestore { restore: false, scrollback_lines: 2000 }
    }
}

/// `[shell]`: the program a new pane runs.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Shell {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub program: Option<String>,
    pub args: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cwd: Option<String>,
    pub integration: bool,
    pub env: BTreeMap<String, String>,
}

impl Default for Shell {
    /// The registered shell, no arguments, shell integration on.
    fn default() -> Self {
        Shell { program: None, args: vec![], cwd: None, integration: true, env: BTreeMap::new() }
    }
}

/// `[clipboard]`: copy, paste and OSC 52 behaviour.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Clipboard {
    pub copy_on_select: bool,
    pub allow_osc52_read: bool,
    pub paste_confirm_newline: bool,
    pub word_separators: String,
}

impl Default for Clipboard {
    /// Nothing copied on select, OSC 52 read refused, no paste confirmation.
    fn default() -> Self {
        Clipboard {
            copy_on_select: false,
            allow_osc52_read: false,
            paste_confirm_newline: false,
            word_separators: " \t'\"()[]{}<>|,;`".into(),
        }
    }
}

/// What the bell does.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum BellMode {
    Visual,
    Audible,
    None,
}

/// `[notify]`: desktop notifications and the bell.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Notify {
    pub after_seconds: u64,
    pub on_bell: BellMode,
    pub command_finished: bool,
}

impl Default for Notify {
    /// Notify for commands longer than ten seconds, with a visual bell.
    fn default() -> Self {
        Notify { after_seconds: 10, on_bell: BellMode::Visual, command_finished: true }
    }
}

/// `[accessibility]`: screen reader access through UI Automation, NSAccessibility and AT-SPI.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Accessibility {
    pub enabled: bool,
    pub update_interval_ms: u64,
}

impl Default for Accessibility {
    /// On, with the accessibility tree updated at most four times a second.
    fn default() -> Self {
        Accessibility { enabled: true, update_interval_ms: 250 }
    }
}

/// `[mouse]`: pointer behaviour.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Mouse {
    pub focus_follows_mouse: bool,
    pub hide_while_typing: bool,
    pub scroll_multiplier: u32,
}

impl Default for Mouse {
    /// Click to focus, hide the pointer while typing, three lines per wheel notch.
    fn default() -> Self {
        Mouse { focus_follows_mouse: false, hide_while_typing: true, scroll_multiplier: 3 }
    }
}

/// `[openers]`: command templates used when a path or URL is clicked.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Openers {
    pub path: String,
    pub url: String,
}

/// The path opener used when Unluminous is installed.
pub const UNLUMINOUS_PATH_OPENER: &str = "unluminous {path}:{line}:{col}";

impl Default for Openers {
    /// Unluminous for paths, the platform opener for URLs (an empty string).
    fn default() -> Self {
        Openers { path: UNLUMINOUS_PATH_OPENER.into(), url: String::new() }
    }
}

impl Openers {
    /// The command template to run for a path: the configured one, except that the default Unluminous template
    /// falls back to the platform opener (an empty string) when Unluminous is not installed.
    pub fn path_command(&self, unluminous_installed: bool) -> String {
        if self.path == UNLUMINOUS_PATH_OPENER && !unluminous_installed { String::new() } else { self.path.clone() }
    }
}

/// Bindings for one platform, written under `[keybindings.windows]`, `[keybindings.macos]` or `[keybindings.linux]`.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, Default)]
#[serde(default)]
pub struct PlatformKeys {
    pub bindings: Vec<Binding>,
}

/// `[keybindings]`: bindings added to or removed from the defaults, then the per platform sections.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, Default)]
#[serde(default)]
pub struct Keybindings {
    pub bindings: Vec<Binding>,
    pub windows: PlatformKeys,
    pub macos: PlatformKeys,
    pub linux: PlatformKeys,
}

/// `[updates]`: the update check (`docs/decisions/updates.md`).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Updates {
    pub check: UpdateCheck,
    pub channel: UpdateChannel,
    /// The version the automatic check does not announce; empty for none.
    pub skip: String,
    /// How long one request to the release server may take.
    pub timeout_seconds: u64,
}

impl Default for Updates {
    /// A daily check of the stable channel with a 15 second limit per request.
    fn default() -> Self {
        Updates { check: UpdateCheck::Daily, channel: UpdateChannel::Stable, skip: String::new(), timeout_seconds: 15 }
    }
}

/// When Termoil asks for a newer version.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum UpdateCheck {
    /// At most once a day, counted across every Termoil process.
    #[default]
    Daily,
    /// Every time the first window opens.
    Start,
    /// Never on its own.
    Off,
}

impl<'de> Deserialize<'de> for UpdateCheck {
    /// Reads `daily`, `start` or `off`, and the boolean the key used to be: true is daily, false is off.
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        #[derive(Deserialize)]
        #[serde(untagged)]
        enum Raw {
            Flag(bool),
            Word(String),
        }
        match Raw::deserialize(d)? {
            Raw::Flag(true) => Ok(UpdateCheck::Daily),
            Raw::Flag(false) => Ok(UpdateCheck::Off),
            Raw::Word(w) => match w.as_str() {
                "daily" => Ok(UpdateCheck::Daily),
                "start" => Ok(UpdateCheck::Start),
                "off" => Ok(UpdateCheck::Off),
                other => {
                    Err(serde::de::Error::custom(format!("unknown variant `{other}`, expected daily, start or off")))
                }
            },
        }
    }
}

/// Which list of releases the update check asks.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum UpdateChannel {
    /// Versions promoted after they were used as a nightly.
    #[default]
    Stable,
    /// Every release.
    Nightly,
}

/// `[log]`: log verbosity.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Log {
    pub level: String,
}

impl Default for Log {
    /// Level `info`.
    fn default() -> Self {
        Log { level: "info".into() }
    }
}
