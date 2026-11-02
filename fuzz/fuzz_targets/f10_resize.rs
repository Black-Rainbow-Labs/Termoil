//! F10: resize and reflow under a random sequence of output and size changes.
#![no_main]
use libfuzzer_sys::fuzz_target;
use termoil_vt::pagelist::ScrollbackLimit;
use termoil_vt::{Terminal, TerminalConfig};
#[path = "common.rs"]
mod common;

fuzz_target!(|data: &[u8]| {
    let config = TerminalConfig { scrollback: ScrollbackLimit { bytes: 200_000, lines: Some(200) }, ..TerminalConfig::default() };
    let mut t = Terminal::new(20, 6, config);
    for op in data.split(|&b| b == 0xff) {
        match op.split_first() {
            Some((&0xfe, size)) if size.len() >= 2 => t.resize(1 + size[0] as u16 % 60, 1 + size[1] as u16 % 20),
            _ => t.advance(op),
        }
        common::check(&t);
    }
});
