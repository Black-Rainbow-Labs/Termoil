//! F9: the mouse encoder over every tracking mode and encoding.
#![no_main]
use libfuzzer_sys::fuzz_target;
use termoil_input::{Modifiers, MouseButton, MouseEncoding, MouseEvent, MouseEventKind, MouseModes, MouseTracking, encode_mouse};

fuzz_target!(|data: &[u8]| {
    if data.len() < 12 {
        return;
    }
    let buttons = [MouseButton::Left, MouseButton::Middle, MouseButton::Right, MouseButton::WheelUp, MouseButton::WheelDown, MouseButton::WheelLeft, MouseButton::WheelRight, MouseButton::Back, MouseButton::Forward, MouseButton::Button10, MouseButton::Button11, MouseButton::None];
    let kind = match data[0] % 3 {
        0 => MouseEventKind::Press,
        1 => MouseEventKind::Release,
        _ => MouseEventKind::Motion,
    };
    let u = |i: usize| u32::from_le_bytes([data[i], data[i + 1], 0, 0]);
    let event = MouseEvent { kind, button: buttons[data[1] as usize % buttons.len()], col: u(2), row: u(4), pixel_x: u(6), pixel_y: u(8), mods: Modifiers(data[10]) };
    let tracking = [MouseTracking::Off, MouseTracking::X10, MouseTracking::Normal, MouseTracking::ButtonEvent, MouseTracking::AnyEvent][data[11] as usize % 5];
    let encoding = [MouseEncoding::Default, MouseEncoding::Utf8, MouseEncoding::Sgr, MouseEncoding::Urxvt, MouseEncoding::SgrPixels][(data[11] / 5) as usize % 5];
    let mut out = Vec::new();
    let _ = encode_mouse(&event, &MouseModes { tracking, encoding }, Some((u(2) ^ 1, u(4))), &mut out);
});
