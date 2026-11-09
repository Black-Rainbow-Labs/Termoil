//! Key chords, chained sequences, bindings and the default keybinding tables.
use serde::{Deserialize, Serialize};
use std::fmt;

/// The modifier keys held with a chord. `super_key` is Cmd on macOS and the Windows key elsewhere.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct Mods {
    pub ctrl: bool,
    pub shift: bool,
    pub alt: bool,
    pub super_key: bool,
}

/// One key press with its modifiers. `key` is normalised: a lowercase character, or a name such as `enter`,
/// `pageup`, `left`, `f11`, `plus` or `space`.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct KeyChord {
    pub mods: Mods,
    pub key: String,
}

impl fmt::Display for KeyChord {
    /// Writes the chord in its canonical form, for example `ctrl+shift+t`.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let m = self.mods;
        for (on, name) in [(m.ctrl, "ctrl"), (m.shift, "shift"), (m.alt, "alt"), (m.super_key, "super")] {
            if on {
                write!(f, "{name}+")?;
            }
        }
        write!(f, "{}", self.key)
    }
}

/// The operating systems with their own keybinding section.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Platform {
    Windows,
    MacOs,
    Linux,
}

impl Platform {
    /// The platform this binary was built for.
    pub fn current() -> Platform {
        if cfg!(target_os = "windows") {
            Platform::Windows
        } else if cfg!(target_os = "macos") {
            Platform::MacOs
        } else {
            Platform::Linux
        }
    }
}

/// One keybinding: a key or sequence, the action it runs, optional arguments and an optional key table name.
/// `action = "none"` removes the binding for `keys` from the defaults.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Binding {
    pub keys: String,
    pub action: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub table: Option<String>,
    #[serde(default, skip_serializing_if = "toml::Table::is_empty")]
    pub args: toml::Table,
}

impl Binding {
    /// Creates a binding with no arguments in the root key table.
    pub fn new(keys: &str, action: &str) -> Binding {
        Binding { keys: keys.into(), action: action.into(), table: None, args: toml::Table::new() }
    }

    /// The chords of this binding in order (one for a plain key, several for `ctrl+a>c`).
    pub fn chords(&self) -> Result<Vec<KeyChord>, String> {
        parse_keys(&self.keys)
    }
}

/// Maps a modifier name to a setter, or `None` when the name is not a modifier.
fn apply_modifier(mods: &mut Mods, name: &str) -> bool {
    match name {
        "ctrl" | "control" => mods.ctrl = true,
        "shift" => mods.shift = true,
        "alt" | "option" | "opt" => mods.alt = true,
        "super" | "cmd" | "command" | "win" | "windows" | "meta" => mods.super_key = true,
        _ => return false,
    }
    true
}

/// Normalises a key name: single characters are lowercased, aliases map to one canonical name, and anything
/// that is not a character, a known name or `f1` to `f24` is an error.
fn normalize_key(raw: &str) -> Result<String, String> {
    let lower = raw.to_lowercase();
    let mut chars = lower.chars();
    if let (Some(c), None) = (chars.next(), chars.next()) {
        return Ok(match c {
            '+' => "plus".into(),
            '>' => "greater".into(),
            ' ' => "space".into(),
            _ => c.to_string(),
        });
    }
    let named = match lower.as_str() {
        "plus" => "plus",
        "greater" => "greater",
        "minus" => "-",
        "comma" => ",",
        "period" => ".",
        "slash" => "/",
        "backslash" => "\\",
        "semicolon" => ";",
        "quote" => "'",
        "equal" | "equals" => "=",
        "backtick" | "grave" => "`",
        "enter" | "return" => "enter",
        "esc" | "escape" => "escape",
        "tab" => "tab",
        "space" | "spc" => "space",
        "backspace" | "bs" => "backspace",
        "delete" | "del" => "delete",
        "insert" | "ins" => "insert",
        "home" => "home",
        "end" => "end",
        "pageup" | "pgup" | "page_up" => "pageup",
        "pagedown" | "pgdn" | "page_down" => "pagedown",
        "up" | "arrowup" => "up",
        "down" | "arrowdown" => "down",
        "left" | "arrowleft" => "left",
        "right" | "arrowright" => "right",
        other => return normalize_function_key(other).ok_or_else(|| format!("unknown key `{raw}`")),
    };
    Ok(named.to_string())
}

/// Returns `fN` for `f1` to `f24`, otherwise `None`.
fn normalize_function_key(name: &str) -> Option<String> {
    let number: u32 = name.strip_prefix('f')?.parse().ok()?;
    (1..=24).contains(&number).then(|| format!("f{number}"))
}

/// Splits `ctrl+shift+t` into its modifier part and key part. A trailing `+` is the plus key (`ctrl++`).
fn split_chord(text: &str) -> (&str, &str) {
    if text == "+" {
        return ("", "+");
    }
    if let Some(mods) = text.strip_suffix("++") {
        return (mods, "+");
    }
    match text.rfind('+') {
        Some(i) => (&text[..i], &text[i + 1..]),
        None => ("", text),
    }
}

/// Parses one chord such as `Ctrl+Shift+T`, `cmd+t`, `alt+left`, `f11` or `ctrl+plus`.
fn parse_chord(text: &str) -> Result<KeyChord, String> {
    let text = text.trim();
    if text.is_empty() {
        return Err("empty key".into());
    }
    let (mod_part, key_part) = split_chord(text);
    let mut mods = Mods::default();
    for name in mod_part.split('+').map(str::trim).filter(|n| !n.is_empty()) {
        if !apply_modifier(&mut mods, &name.to_lowercase()) {
            return Err(format!("unknown modifier `{name}` in `{text}`"));
        }
    }
    if key_part.is_empty() {
        return Err(format!("`{text}` has no key after the last `+`"));
    }
    Ok(KeyChord { mods, key: normalize_key(key_part.trim())? })
}

/// Parses a key string into its chords. `ctrl+shift+t` is one chord and `ctrl+a>c` is the sequence "press
/// ctrl+a, then c". Names are case insensitive, `cmd`, `command`, `win` and `super` are the same modifier.
pub fn parse_keys(text: &str) -> Result<Vec<KeyChord>, String> {
    let parts: Vec<&str> = text.split('>').collect();
    // A lone `>` or a chord ending in `>` is the greater-than key written as a character.
    if text.trim() == ">" {
        return Ok(vec![parse_chord("greater")?]);
    }
    parts.iter().map(|p| parse_chord(p)).collect()
}

/// The canonical text of a key string, for example `Ctrl+Shift+T` becomes `ctrl+shift+t`.
pub fn normalize_keys(text: &str) -> Result<String, String> {
    let chords = parse_keys(text)?;
    Ok(chords.iter().map(KeyChord::to_string).collect::<Vec<_>>().join(">"))
}

/// The default bindings for Windows and Linux, exactly as the PRD lists them, plus `ctrl+=`: the key
/// people press for Ctrl and plus on a US layout, where the plus sign needs Shift (task-2589).
pub fn default_keybindings() -> Vec<Binding> {
    let table = [
        ("ctrl+shift+t", "new-tab"),
        ("ctrl+shift+d", "split-right"),
        ("ctrl+shift+e", "split-down"),
        ("ctrl+shift+w", "close"),
        ("ctrl+tab", "next-tab"),
        ("ctrl+shift+tab", "previous-tab"),
        ("alt+left", "focus-left"),
        ("alt+right", "focus-right"),
        ("alt+up", "focus-up"),
        ("alt+down", "focus-down"),
        ("ctrl+shift+f", "search"),
        ("ctrl+shift+p", "command-palette"),
        ("ctrl+shift+up", "previous-prompt"),
        ("ctrl+shift+down", "next-prompt"),
        ("ctrl+shift+space", "select-last-output"),
        ("ctrl+shift+c", "copy"),
        ("ctrl+shift+v", "paste"),
        ("shift+pageup", "scroll-page-up"),
        ("shift+pagedown", "scroll-page-down"),
        ("ctrl+shift+home", "scroll-to-top"),
        ("ctrl+shift+end", "scroll-to-bottom"),
        ("ctrl+plus", "increase-font-size"),
        ("ctrl+=", "increase-font-size"),
        ("ctrl+minus", "decrease-font-size"),
        ("ctrl+0", "reset-font-size"),
        ("f11", "toggle-fullscreen"),
        ("ctrl+shift+n", "new-window"),
        ("ctrl+shift+k", "clear-scrollback"),
        ("ctrl+shift+h", "show-hints"),
        ("ctrl+,", "open-settings"),
    ];
    let mut out: Vec<Binding> = table.iter().map(|(k, a)| Binding::new(k, a)).collect();
    out.extend(resize_bindings("ctrl+shift+alt"));
    out
}

/// Bindings that move a split's divider with the arrow keys and the given modifiers.
fn resize_bindings(mods: &str) -> Vec<Binding> {
    ["left", "right", "up", "down"]
        .iter()
        .map(|dir| {
            let mut b = Binding::new(&format!("{mods}+{dir}"), "resize-split");
            b.args.insert("direction".into(), toml::Value::String((*dir).into()));
            b
        })
        .collect()
}

/// The default bindings on macOS: the same actions with Cmd in place of Ctrl+Shift where the two would
/// otherwise clash with the system, and Cmd+Alt+Arrow for pane focus because Alt+Arrow moves by word.
pub fn default_keybindings_macos() -> Vec<Binding> {
    let table = [
        ("cmd+t", "new-tab"),
        ("cmd+d", "split-right"),
        ("cmd+shift+d", "split-down"),
        ("cmd+w", "close"),
        ("ctrl+tab", "next-tab"),
        ("ctrl+shift+tab", "previous-tab"),
        ("cmd+alt+left", "focus-left"),
        ("cmd+alt+right", "focus-right"),
        ("cmd+alt+up", "focus-up"),
        ("cmd+alt+down", "focus-down"),
        ("cmd+f", "search"),
        ("cmd+shift+p", "command-palette"),
        ("cmd+up", "previous-prompt"),
        ("cmd+down", "next-prompt"),
        ("cmd+shift+space", "select-last-output"),
        ("cmd+c", "copy"),
        ("cmd+v", "paste"),
        ("shift+pageup", "scroll-page-up"),
        ("shift+pagedown", "scroll-page-down"),
        ("cmd+home", "scroll-to-top"),
        ("cmd+end", "scroll-to-bottom"),
        ("cmd+plus", "increase-font-size"),
        ("cmd+=", "increase-font-size"),
        ("cmd+minus", "decrease-font-size"),
        ("cmd+0", "reset-font-size"),
        ("ctrl+cmd+f", "toggle-fullscreen"),
        ("cmd+n", "new-window"),
        ("cmd+k", "clear-scrollback"),
        ("cmd+,", "open-settings"),
    ];
    let mut out: Vec<Binding> = table.iter().map(|(k, a)| Binding::new(k, a)).collect();
    out.extend(resize_bindings("cmd+ctrl"));
    out.push(Binding::new("cmd+shift+h", "show-hints"));
    out
}

/// The default bindings for a platform.
pub fn default_keybindings_for(platform: Platform) -> Vec<Binding> {
    match platform {
        Platform::MacOs => default_keybindings_macos(),
        _ => default_keybindings(),
    }
}

/// The identity of a binding for override purposes: its key table and canonical keys (the raw lowercase text
/// when the keys do not parse).
fn binding_identity(binding: &Binding) -> (Option<String>, String) {
    let keys = normalize_keys(&binding.keys).unwrap_or_else(|_| binding.keys.to_lowercase());
    (binding.table.clone(), keys)
}

/// Applies `extra` over `list` in order: a binding replaces any earlier binding for the same keys, and
/// `action = "none"` removes it. Bindings whose keys do not parse are ignored.
pub fn overlay_bindings(list: &mut Vec<Binding>, extra: &[Binding]) {
    for binding in extra {
        if parse_keys(&binding.keys).is_err() {
            continue;
        }
        let id = binding_identity(binding);
        list.retain(|existing| binding_identity(existing) != id);
        if binding.action != "none" {
            list.push(binding.clone());
        }
    }
}
