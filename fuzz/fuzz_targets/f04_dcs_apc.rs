//! F4: DCS and APC payloads (DECRQSS, XTGETTCAP, tmux passthrough, kitty graphics, sixel).
#![no_main]
use libfuzzer_sys::fuzz_target;
use terminus_vt::{Terminal, TerminalConfig};
#[path = "common.rs"]
mod common;

fuzz_target!(|data: &[u8]| {
    let Some((&first, rest)) = data.split_first() else { return };
    let intro: &[u8] = match first % 6 {
        0 => b"\x1bP$q",
        1 => b"\x1bP+q",
        2 => b"\x1bPtmux;",
        3 => b"\x1bPq",
        4 => b"\x1b_G",
        _ => b"\x1bP",
    };
    let mut bytes = intro.to_vec();
    bytes.extend_from_slice(rest);
    bytes.extend_from_slice(b"\x1b\\");
    let mut t = Terminal::new(20, 5, TerminalConfig::default());
    t.advance(&bytes);
    common::check(&t);
});
