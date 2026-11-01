//! Key chords: turning a config chord (`ctrl+c`, `shift+tab`, `f5`) into a key event for the encoder,
//! a key event into a chord for keybinding lookup, and matching chained bindings.

use serde_json::{Map, Value};
use terminus_input::{Key, KeyEvent, Modifiers, NamedKey};

use crate::config::{Binding, KeyChord, Mods, parse_keys};

/// The key event a chord presses, with the text the key would type when it types any.
pub fn chord_to_event(chord: &KeyChord) -> Option<KeyEvent> {
    let mods = to_modifiers(chord.mods);
    let key = match chord.key.as_str() {
        "enter" => Key::Named(NamedKey::Enter),
        "escape" => Key::Named(NamedKey::Escape),
        "tab" => Key::Named(NamedKey::Tab),
        "space" => Key::Char(' '),
        "backspace" => Key::Named(NamedKey::Backspace),
        "delete" => Key::Named(NamedKey::Delete),
        "insert" => Key::Named(NamedKey::Insert),
        "left" => Key::Named(NamedKey::Left),
        "right" => Key::Named(NamedKey::Right),
        "up" => Key::Named(NamedKey::Up),
        "down" => Key::Named(NamedKey::Down),
        "home" => Key::Named(NamedKey::Home),
        "end" => Key::Named(NamedKey::End),
        "pageup" => Key::Named(NamedKey::PageUp),
        "pagedown" => Key::Named(NamedKey::PageDown),
        "plus" => Key::Char('+'),
        "greater" => Key::Char('>'),
        k if k.starts_with('f') && k.len() > 1 => Key::Named(NamedKey::F(k[1..].parse().ok()?)),
        k => {
            let mut chars = k.chars();
            let c = chars.next()?;
            if chars.next().is_some() {
                return None;
            }
            Key::Char(c)
        }
    };
    let mut ev = KeyEvent::press(key, mods);
    if let Key::Char(c) = key
        && !chord.mods.ctrl
        && !chord.mods.alt
        && !chord.mods.super_key
    {
        let typed = if chord.mods.shift { c.to_uppercase().collect::<String>() } else { c.to_string() };
        ev = ev.with_text(&typed);
    }
    Some(ev)
}

/// Converts config modifiers to the encoder's.
fn to_modifiers(m: Mods) -> Modifiers {
    let mut out = Modifiers::NONE;
    for (on, bit) in [
        (m.shift, Modifiers::SHIFT),
        (m.alt, Modifiers::ALT),
        (m.ctrl, Modifiers::CTRL),
        (m.super_key, Modifiers::SUPER),
    ] {
        if on {
            out |= bit;
        }
    }
    out
}

/// Parses a single chord such as `ctrl+c`.
pub fn parse_chord(text: &str) -> Result<KeyChord, String> {
    let mut chords = parse_keys(text)?;
    if chords.len() != 1 {
        return Err(format!("'{text}' is a sequence; send one key at a time"));
    }
    Ok(chords.remove(0))
}

/// The chord a key event matches in the binding table, when it has a name the table can hold.
pub fn event_to_chord(ev: &KeyEvent) -> Option<KeyChord> {
    let key = match ev.key {
        Key::Char(' ') => "space".to_string(),
        Key::Char('+') => "plus".to_string(),
        Key::Char('>') => "greater".to_string(),
        Key::Char('\0') => return None,
        Key::Char(c) => c.to_lowercase().collect(),
        Key::Named(n) => named_key_name(n)?.to_string(),
    };
    let m = ev.mods;
    Some(KeyChord {
        mods: Mods {
            ctrl: m.contains(Modifiers::CTRL),
            shift: m.contains(Modifiers::SHIFT),
            alt: m.contains(Modifiers::ALT),
            super_key: m.contains(Modifiers::SUPER),
        },
        key,
    })
}

/// The binding table name of a named key.
fn named_key_name(n: NamedKey) -> Option<String> {
    Some(match n {
        NamedKey::Enter => "enter".into(),
        NamedKey::Escape => "escape".into(),
        NamedKey::Tab => "tab".into(),
        NamedKey::Backspace => "backspace".into(),
        NamedKey::Delete => "delete".into(),
        NamedKey::Insert => "insert".into(),
        NamedKey::Left => "left".into(),
        NamedKey::Right => "right".into(),
        NamedKey::Up => "up".into(),
        NamedKey::Down => "down".into(),
        NamedKey::Home => "home".into(),
        NamedKey::End => "end".into(),
        NamedKey::PageUp => "pageup".into(),
        NamedKey::PageDown => "pagedown".into(),
        NamedKey::F(n) => format!("f{n}"),
        _ => return None,
    })
}

/// A binding resolved for lookup.
#[derive(Clone, Debug)]
pub struct Bound {
    /// The chords in order.
    pub chords: Vec<KeyChord>,
    /// The action.
    pub action: String,
    /// Its arguments.
    pub args: Map<String, Value>,
}

/// What a key press did against the bindings.
#[derive(Clone, Debug, PartialEq)]
pub enum Lookup {
    /// No binding starts with the keys pressed so far.
    None,
    /// The keys so far start a chained binding; wait for the next.
    Pending,
    /// A binding matched: run this action with these arguments.
    Action(String, Map<String, Value>),
}

/// Resolves bindings once, dropping those whose keys do not parse and those set to `none`.
pub fn resolve(bindings: &[Binding]) -> Vec<Bound> {
    bindings
        .iter()
        .filter(|b| b.action != "none" && b.table.is_none())
        .filter_map(|b| {
            let chords = b.chords().ok()?;
            let args = serde_json::to_value(&b.args).ok().and_then(|v| v.as_object().cloned()).unwrap_or_default();
            Some(Bound { chords, action: b.action.clone(), args })
        })
        .collect()
}

/// Looks up the keys pressed so far (`pressed`, the last one included).
pub fn lookup(bound: &[Bound], pressed: &[KeyChord]) -> Lookup {
    let mut pending = false;
    for b in bound {
        if b.chords.len() < pressed.len() || b.chords[..pressed.len()] != *pressed {
            continue;
        }
        if b.chords.len() == pressed.len() {
            return Lookup::Action(b.action.clone(), b.args.clone());
        }
        pending = true;
    }
    if pending { Lookup::Pending } else { Lookup::None }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn chords_round_trip() {
        for text in ["ctrl+c", "shift+tab", "f5", "alt+x", "ctrl+shift+t", "enter", "space", "pageup"] {
            let chord = parse_chord(text).unwrap();
            let ev = chord_to_event(&chord).unwrap_or_else(|| panic!("{text}"));
            assert_eq!(event_to_chord(&ev).as_ref(), Some(&chord), "{text}");
        }
    }

    #[test]
    fn chained_lookup() {
        let bound = resolve(&[Binding::new("ctrl+a>c", "new-tab"), Binding::new("ctrl+shift+t", "new-tab")]);
        let a = parse_chord("ctrl+a").unwrap();
        let c = parse_chord("c").unwrap();
        assert_eq!(lookup(&bound, std::slice::from_ref(&a)), Lookup::Pending);
        assert!(matches!(lookup(&bound, &[a, c]), Lookup::Action(ref n, _) if n == "new-tab"));
        assert_eq!(lookup(&bound, &[parse_chord("x").unwrap()]), Lookup::None);
    }
}
