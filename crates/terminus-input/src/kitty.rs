//! The kitty keyboard protocol encoder, written from the published specification.

use crate::key::{Key, KeyEvent, KeyEventKind, KeyboardModes, KeypadKey, NamedKey};
use crate::keycodes::{Form, is_cursor_or_function_key, kitty_form};
use crate::legacy;
use crate::modifiers::Modifiers;
use crate::seq::{has_control, push_csi, push_num, unshifted};

const DISAMBIGUATE: u8 = 1;
const REPORT_EVENTS: u8 = 2;
const REPORT_ALTERNATES: u8 = 4;
const REPORT_ALL_KEYS: u8 = 8;
const REPORT_TEXT: u8 = 16;

/// How a key event is sent under the active flags.
enum Plan {
    /// Nothing is sent.
    Skip,
    /// The associated text is sent as plain UTF-8.
    Text,
    /// The legacy encoding is used.
    Legacy,
    /// A CSI escape code is used.
    Escape,
}

/// Encodes a key event under the kitty keyboard protocol flags in `modes`. Returns false when nothing is sent.
pub(crate) fn encode(event: &KeyEvent, modes: &KeyboardModes, out: &mut Vec<u8>) -> bool {
    let flags = modes.kitty_flags;
    if event.kind == KeyEventKind::Release && flags & REPORT_EVENTS == 0 {
        return false;
    }
    match plan(event, flags) {
        Plan::Skip => false,
        Plan::Text => push_event_text(event, out),
        Plan::Legacy => legacy::encode(event, modes, out),
        Plan::Escape => encode_escape(event, flags, out),
    }
}

/// Writes the associated text of a press or repeat event.
fn push_event_text(event: &KeyEvent, out: &mut Vec<u8>) -> bool {
    if event.kind == KeyEventKind::Release {
        return false;
    }
    match event.text.as_ref().filter(|t| !t.is_empty()) {
        Some(t) => {
            out.extend_from_slice(t.as_bytes());
            true
        }
        None => false,
    }
}

/// True when a modifier other than shift and the locks is held.
fn has_command_modifier(mods: Modifiers) -> bool {
    mods.intersects(Modifiers::ALT | Modifiers::CTRL | Modifiers::EXTENDED)
}

/// Decides whether the event is sent as text, legacy bytes, an escape code or not at all.
fn plan(event: &KeyEvent, flags: u8) -> Plan {
    let all = flags & REPORT_ALL_KEYS != 0;
    let disambiguate = flags & DISAMBIGUATE != 0;
    let mods = event.mods;
    match event.key {
        Key::Named(k) if k.is_modifier_key() => {
            if all {
                Plan::Escape
            } else {
                Plan::Skip
            }
        }
        _ if all => Plan::Escape,
        Key::Char(_) if disambiguate && has_command_modifier(mods) => Plan::Escape,
        Key::Char(_) => Plan::Legacy,
        Key::Named(k) => plan_named(event, k, flags),
    }
}

/// Plans a named key when not every key is reported as an escape code.
fn plan_named(event: &KeyEvent, key: NamedKey, flags: u8) -> Plan {
    let disambiguate = flags & DISAMBIGUATE != 0;
    let mods = event.mods;
    match key {
        NamedKey::Enter | NamedKey::Tab | NamedKey::Backspace => {
            let modified = mods.intersects(Modifiers::SHIFT | Modifiers::ALT | Modifiers::CTRL | Modifiers::EXTENDED);
            if disambiguate && modified { Plan::Escape } else { Plan::Legacy }
        }
        NamedKey::Kp(kp) if disambiguate && kp != KeypadKey::Begin => {
            let text = event.text.as_ref().is_some_and(|t| !t.is_empty() && !has_control(t));
            if text && !has_command_modifier(mods) { Plan::Text } else { Plan::Escape }
        }
        _ if disambiguate => Plan::Escape,
        k if flags & REPORT_EVENTS != 0 && is_cursor_or_function_key(k) => Plan::Escape,
        _ => Plan::Legacy,
    }
}

/// Returns the key code of a character key: the lowercase code point, or 0 for a pure text event.
fn char_code(c: char) -> u32 {
    if c == '\0' { 0 } else { unshifted(c) as u32 }
}

/// Returns the code points of the associated text when the event should carry it.
fn associated_text(event: &KeyEvent, flags: u8) -> Option<Vec<u32>> {
    let wanted = flags & REPORT_ALL_KEYS != 0 && flags & REPORT_TEXT != 0;
    let has_text_key = matches!(event.key, Key::Char(_) | Key::Named(NamedKey::Kp(_)));
    if !wanted || !has_text_key || event.kind == KeyEventKind::Release {
        return None;
    }
    let text = event.text.as_ref().filter(|t| !t.is_empty() && !has_control(t))?;
    Some(text.chars().map(|c| c as u32).collect())
}

/// Returns the modifier bits to report: lock bits are dropped for text producing keys unless all keys are reported.
fn reported_mods(event: &KeyEvent, flags: u8) -> Modifiers {
    let text_key =
        matches!(event.key, Key::Char(_) | Key::Named(NamedKey::Enter | NamedKey::Tab | NamedKey::Backspace));
    if flags & REPORT_ALL_KEYS == 0 && text_key { event.mods.without(Modifiers::LOCKS) } else { event.mods }
}

/// Returns the event type sub field value: 2 for repeat and 3 for release, None for press or when types are not requested.
fn event_type(kind: KeyEventKind, flags: u8) -> Option<u32> {
    if flags & REPORT_EVENTS == 0 {
        return None;
    }
    match kind {
        KeyEventKind::Press => None,
        KeyEventKind::Repeat => Some(2),
        KeyEventKind::Release => Some(3),
    }
}

/// Writes `;mods` with an optional `:event` sub field.
fn push_mods_field(out: &mut Vec<u8>, wire_mods: u32, event: Option<u32>) {
    out.push(b';');
    push_num(out, wire_mods);
    if let Some(e) = event {
        out.push(b':');
        push_num(out, e);
    }
}

/// Writes the alternate key sub fields (`:shifted:base`) when alternate key reporting is on.
fn push_alternates(out: &mut Vec<u8>, event: &KeyEvent, flags: u8, mods: Modifiers) {
    let Key::Char(c) = event.key else { return };
    if flags & REPORT_ALTERNATES == 0 || c == '\0' {
        return;
    }
    let shifted = event.shifted_key.filter(|_| mods.contains(Modifiers::SHIFT));
    let base = event.base_layout_key.filter(|b| *b != c);
    if shifted.is_none() && base.is_none() {
        return;
    }
    out.push(b':');
    if let Some(s) = shifted {
        push_num(out, s as u32);
    }
    if let Some(b) = base {
        out.push(b':');
        push_num(out, b as u32);
    }
}

/// Writes `CSI code[:alternates] ; mods[:event] ; text u`, dropping trailing empty fields.
fn push_unicode_form(out: &mut Vec<u8>, code: u32, event: &KeyEvent, flags: u8, mods: Modifiers) {
    let ev = event_type(event.kind, flags);
    let text = associated_text(event, flags);
    push_csi(out);
    push_num(out, code);
    push_alternates(out, event, flags, mods);
    if mods.wire_value() > 1 || ev.is_some() {
        push_mods_field(out, mods.wire_value(), ev);
    } else if text.is_some() {
        out.push(b';');
    }
    if let Some(points) = text {
        out.push(b';');
        let joined: Vec<String> = points.iter().map(|p| p.to_string()).collect();
        out.extend_from_slice(joined.join(":").as_bytes());
    }
    out.push(b'u');
}

/// Writes the tilde form `CSI number ; mods ~` or the letter form `CSI 1 ; mods letter`.
fn push_functional_form(out: &mut Vec<u8>, form: Form, wire_mods: u32, ev: Option<u32>) {
    push_csi(out);
    let with_mods = wire_mods > 1 || ev.is_some();
    match form {
        Form::Tilde(n) => {
            push_num(out, n);
            if with_mods {
                push_mods_field(out, wire_mods, ev);
            }
            out.push(b'~');
        }
        Form::Letter(l) => {
            if with_mods {
                out.extend_from_slice(b"1");
                push_mods_field(out, wire_mods, ev);
            }
            out.push(l);
        }
        Form::Unicode(_) => {}
    }
}

/// Encodes a key event as a kitty escape code.
fn encode_escape(event: &KeyEvent, flags: u8, out: &mut Vec<u8>) -> bool {
    let all = flags & REPORT_ALL_KEYS != 0;
    let legacy_text_key = matches!(event.key, Key::Named(NamedKey::Enter | NamedKey::Tab | NamedKey::Backspace));
    if event.kind == KeyEventKind::Release && legacy_text_key && !all {
        return false;
    }
    let form = match event.key {
        Key::Char(c) => Form::Unicode(char_code(c)),
        Key::Named(k) => match kitty_form(k) {
            Some(f) => f,
            None => return false,
        },
    };
    let mods = reported_mods(event, flags);
    match form {
        Form::Unicode(code) => push_unicode_form(out, code, event, flags, mods),
        other => push_functional_form(out, other, mods.wire_value(), event_type(event.kind, flags)),
    }
    true
}
