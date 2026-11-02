//! Key chords: turning a config chord (`ctrl+c`, `shift+tab`, `f5`) into a key event for the encoder,
//! a key event into a chord for keybinding lookup, and matching chained bindings and key tables.
//!
//! A binding with `table = "name"` belongs to that key table and is used only while the table is
//! active (`activate-key-table`). Keys are looked up in the newest active table first, then the
//! older ones, then the bindings outside any table. `keys = "catch_all"` in a table takes every key
//! the table binds no other way.

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
    /// The chords in order; empty for a table's `catch_all`.
    pub chords: Vec<KeyChord>,
    /// The action.
    pub action: String,
    /// Its arguments.
    pub args: Map<String, Value>,
    /// The key table it belongs to, or None for the bindings outside any table.
    pub table: Option<String>,
}

/// A key table made active by `activate-key-table`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ActiveTable {
    /// The table's name.
    pub name: String,
    /// Deactivate it after the next key.
    pub once: bool,
}

/// The `keys` value that makes a table binding take every key the table binds no other way.
pub const CATCH_ALL: &str = "catch_all";

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

/// Resolves bindings once, dropping those whose keys do not parse and those set to `none`. A
/// `catch_all` binding is kept only inside a key table.
pub fn resolve(bindings: &[Binding]) -> Vec<Bound> {
    bindings
        .iter()
        .filter(|b| b.action != "none")
        .filter_map(|b| {
            let chords = if b.keys == CATCH_ALL && b.table.is_some() { Vec::new() } else { b.chords().ok()? };
            let args = serde_json::to_value(&b.args).ok().and_then(|v| v.as_object().cloned()).unwrap_or_default();
            Some(Bound { chords, action: b.action.clone(), args, table: b.table.clone() })
        })
        .collect()
}

/// Looks up the keys pressed so far (`pressed`, the last one included) in the bindings outside
/// any key table.
pub fn lookup(bound: &[Bound], pressed: &[KeyChord]) -> Lookup {
    lookup_in(bound, None, pressed)
}

/// Looks up the keys pressed so far in the active key tables, newest first, and then in the
/// bindings outside any table.
pub fn lookup_active(bound: &[Bound], active: &[ActiveTable], pressed: &[KeyChord]) -> Lookup {
    for table in active.iter().rev() {
        match lookup_in(bound, Some(&table.name), pressed) {
            Lookup::None => {}
            found => return found,
        }
    }
    lookup_in(bound, None, pressed)
}

/// Looks up the keys pressed so far in one key table (None: the bindings outside any table). A
/// single key that nothing else in a table matches goes to the table's `catch_all`.
fn lookup_in(bound: &[Bound], table: Option<&str>, pressed: &[KeyChord]) -> Lookup {
    let mut pending = false;
    let mut catch_all = None;
    for b in bound.iter().filter(|b| b.table.as_deref() == table) {
        if b.chords.is_empty() {
            catch_all = Some(b);
            continue;
        }
        if b.chords.len() < pressed.len() || b.chords[..pressed.len()] != *pressed {
            continue;
        }
        if b.chords.len() == pressed.len() {
            return Lookup::Action(b.action.clone(), b.args.clone());
        }
        pending = true;
    }
    match catch_all {
        _ if pending => Lookup::Pending,
        Some(b) if pressed.len() == 1 => Lookup::Action(b.action.clone(), b.args.clone()),
        _ => Lookup::None,
    }
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

    /// A binding in `table`.
    fn in_table(keys: &str, action: &str, table: &str) -> Binding {
        Binding { table: Some(table.into()), ..Binding::new(keys, action) }
    }

    #[test]
    fn key_tables_are_looked_up_newest_first_then_the_root() {
        let bound = resolve(&[
            Binding::new("ctrl+shift+t", "new-tab"),
            Binding::new("h", "should-not-run"),
            in_table("h", "focus-left", "panes"),
            in_table("escape", "deactivate-key-table", "panes"),
            in_table("h", "resize-split", "resize"),
            in_table("catch_all", "deactivate-all-key-tables", "resize"),
        ]);
        let key = |k: &str| vec![parse_chord(k).unwrap()];
        let action = |l: Lookup| match l {
            Lookup::Action(n, _) => n,
            other => format!("{other:?}"),
        };
        let panes = ActiveTable { name: "panes".into(), once: false };
        let resize = ActiveTable { name: "resize".into(), once: false };
        assert_eq!(action(lookup(&bound, &key("h"))), "should-not-run", "no table is active");
        assert_eq!(action(lookup_active(&bound, std::slice::from_ref(&panes), &key("h"))), "focus-left");
        assert_eq!(action(lookup_active(&bound, std::slice::from_ref(&panes), &key("ctrl+shift+t"))), "new-tab");
        assert_eq!(action(lookup_active(&bound, std::slice::from_ref(&panes), &key("x"))), "None");
        let both = [panes, resize];
        assert_eq!(action(lookup_active(&bound, &both, &key("h"))), "resize-split", "the newest table wins");
        assert_eq!(action(lookup_active(&bound, &both, &key("x"))), "deactivate-all-key-tables", "catch_all");
        assert!(resolve(&[Binding::new("catch_all", "new-tab")]).is_empty(), "catch_all only inside a table");
    }
}
