//! Mouse report encoding for the xterm tracking modes 9, 1000, 1002 and 1003 and the encodings 1005, 1006, 1015 and 1016.

use crate::modifiers::Modifiers;
use crate::seq::{push_char, push_csi, push_num};

/// A mouse button, wheel direction or the absence of a button.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum MouseButton {
    Left,
    Middle,
    Right,
    WheelUp,
    WheelDown,
    WheelLeft,
    WheelRight,
    /// Button 8, usually Back.
    Back,
    /// Button 9, usually Forward.
    Forward,
    Button10,
    Button11,
    /// No button held, used for motion.
    None,
}

/// What the mouse did.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum MouseEventKind {
    Press,
    Release,
    Motion,
}

/// One mouse event with its cell and pixel position.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct MouseEvent {
    pub kind: MouseEventKind,
    pub button: MouseButton,
    /// Zero based column.
    pub col: u32,
    /// Zero based row.
    pub row: u32,
    pub pixel_x: u32,
    pub pixel_y: u32,
    pub mods: Modifiers,
}

/// Which events the application asked to receive.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MouseTracking {
    Off,
    /// Mode 9: button presses only, no modifiers.
    X10,
    /// Mode 1000: presses and releases.
    Normal,
    /// Mode 1002: plus motion while a button is held.
    ButtonEvent,
    /// Mode 1003: plus all motion.
    AnyEvent,
}

/// How a report is written.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MouseEncoding {
    /// `CSI M` followed by three bytes.
    Default,
    /// Mode 1005: like the default with UTF-8 coded values.
    Utf8,
    /// Mode 1006: `CSI < b ; x ; y M` or `m`.
    Sgr,
    /// Mode 1015: `CSI b ; x ; y M`.
    Urxvt,
    /// Mode 1016: SGR form with pixel coordinates.
    SgrPixels,
}

/// The tracking mode and the encoding together.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct MouseModes {
    pub tracking: MouseTracking,
    pub encoding: MouseEncoding,
}

/// Returns the button number used in a report, or None for a button that cannot be reported.
fn button_code(button: MouseButton) -> Option<u32> {
    Some(match button {
        MouseButton::Left => 0,
        MouseButton::Middle => 1,
        MouseButton::Right => 2,
        MouseButton::WheelUp => 64,
        MouseButton::WheelDown => 65,
        MouseButton::WheelLeft => 66,
        MouseButton::WheelRight => 67,
        MouseButton::Back => 128,
        MouseButton::Forward => 129,
        MouseButton::Button10 => 130,
        MouseButton::Button11 => 131,
        MouseButton::None => return None,
    })
}

/// True for the four wheel directions.
fn is_wheel(button: MouseButton) -> bool {
    matches!(button, MouseButton::WheelUp | MouseButton::WheelDown | MouseButton::WheelLeft | MouseButton::WheelRight)
}

/// Decides whether the tracking mode wants this event at all.
fn tracking_wants(event: &MouseEvent, tracking: MouseTracking) -> bool {
    match (tracking, event.kind) {
        (MouseTracking::Off, _) => false,
        (_, MouseEventKind::Press) => event.button != MouseButton::None,
        (MouseTracking::X10, _) => false,
        (_, MouseEventKind::Release) => event.button != MouseButton::None && !is_wheel(event.button),
        (MouseTracking::Normal, MouseEventKind::Motion) => false,
        (MouseTracking::ButtonEvent, MouseEventKind::Motion) => {
            event.button != MouseButton::None && !is_wheel(event.button)
        }
        (MouseTracking::AnyEvent, MouseEventKind::Motion) => !is_wheel(event.button),
    }
}

/// Returns the modifier bits added to the button number: shift 4, alt 8, ctrl 16.
fn modifier_bits(mods: Modifiers, tracking: MouseTracking) -> u32 {
    if tracking == MouseTracking::X10 {
        return 0;
    }
    let mut bits = 0;
    if mods.contains(Modifiers::SHIFT) {
        bits |= 4;
    }
    if mods.contains(Modifiers::ALT) {
        bits |= 8;
    }
    if mods.contains(Modifiers::CTRL) {
        bits |= 16;
    }
    bits
}

/// Returns the button field of a report: the button, motion flag and modifier bits. Releases use button 3 except in SGR forms.
fn button_field(event: &MouseEvent, modes: &MouseModes) -> Option<u32> {
    let sgr = matches!(modes.encoding, MouseEncoding::Sgr | MouseEncoding::SgrPixels);
    let base = match (event.kind, button_code(event.button)) {
        (MouseEventKind::Release, Some(_)) if !sgr => 3,
        (_, Some(code)) => code,
        (MouseEventKind::Motion, None) => 3,
        (_, None) => return None,
    };
    let motion = if event.kind == MouseEventKind::Motion { 32 } else { 0 };
    Some(base | motion | modifier_bits(event.mods, modes.tracking))
}

/// Writes a value plus 32 as one byte, or as UTF-8 in mode 1005. Returns false when it does not fit.
fn push_legacy_value(out: &mut Vec<u8>, value: u32, utf8: bool) -> bool {
    let coded = value + 32;
    if utf8 {
        match char::from_u32(coded).filter(|_| coded <= 2047) {
            Some(c) => push_char(out, c),
            None => return false,
        }
    } else {
        match u8::try_from(coded) {
            Ok(b) => out.push(b),
            Err(_) => return false,
        }
    }
    true
}

/// Writes the default or UTF-8 form `CSI M b x y`. Returns false when a coordinate is out of range.
fn push_legacy_report(out: &mut Vec<u8>, button: u32, col: u32, row: u32, utf8: bool) -> bool {
    let mut report = Vec::with_capacity(9);
    push_csi(&mut report);
    report.push(b'M');
    let ok = push_legacy_value(&mut report, button, utf8)
        && push_legacy_value(&mut report, col + 1, utf8)
        && push_legacy_value(&mut report, row + 1, utf8);
    if ok {
        out.extend_from_slice(&report);
    }
    ok
}

/// Writes `CSI < b ; x ; y M` or `m` for the SGR forms.
fn push_sgr_report(out: &mut Vec<u8>, button: u32, x: u32, y: u32, release: bool) {
    push_csi(out);
    out.push(b'<');
    push_num(out, button);
    out.push(b';');
    push_num(out, x);
    out.push(b';');
    push_num(out, y);
    out.push(if release { b'm' } else { b'M' });
}

/// Writes `CSI b+32 ; x ; y M` for the urxvt form.
fn push_urxvt_report(out: &mut Vec<u8>, button: u32, col: u32, row: u32) {
    push_csi(out);
    push_num(out, button + 32);
    out.push(b';');
    push_num(out, col + 1);
    out.push(b';');
    push_num(out, row + 1);
    out.push(b'M');
}

/// Encodes a mouse event for the active tracking mode and encoding. Returns false when nothing is reported:
/// the mode does not want the event, motion stayed in the last reported cell, or the legacy form cannot express the position.
pub fn encode_mouse(
    event: &MouseEvent, modes: &MouseModes, last_reported_cell: Option<(u32, u32)>, out: &mut Vec<u8>,
) -> bool {
    if !tracking_wants(event, modes.tracking) {
        return false;
    }
    let pixel_mode = modes.encoding == MouseEncoding::SgrPixels;
    let same_cell = last_reported_cell == Some((event.col, event.row));
    if event.kind == MouseEventKind::Motion && same_cell && !pixel_mode {
        return false;
    }
    let Some(button) = button_field(event, modes) else { return false };
    let release = event.kind == MouseEventKind::Release;
    match modes.encoding {
        MouseEncoding::Default => push_legacy_report(out, button, event.col, event.row, false),
        MouseEncoding::Utf8 => push_legacy_report(out, button, event.col, event.row, true),
        MouseEncoding::Sgr => {
            push_sgr_report(out, button, event.col + 1, event.row + 1, release);
            true
        }
        MouseEncoding::Urxvt => {
            push_urxvt_report(out, button, event.col, event.row);
            true
        }
        MouseEncoding::SgrPixels => {
            push_sgr_report(out, button, event.pixel_x + 1, event.pixel_y + 1, release);
            true
        }
    }
}
