//! F2: parser plus screen on a small terminal through both paths, with the grid invariants.
#![no_main]
use libfuzzer_sys::fuzz_target;
use termoil_vt::{Terminal, TerminalConfig};
#[path = "common.rs"]
mod common;

fuzz_target!(|data: &[u8]| {
    let Some((&first, rest)) = data.split_first() else { return };
    let mut t = Terminal::new(1 + (first % 13) as u16, 1 + (first / 32) as u16 % 6, TerminalConfig::default());
    t.set_simd(first & 1 == 0);
    t.advance(rest);
    common::check(&t);
});
