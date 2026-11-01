//! F3: OSC payloads with each of the three terminators.
#![no_main]
use libfuzzer_sys::fuzz_target;
use terminus_vt::{Terminal, TerminalConfig};
#[path = "common.rs"]
mod common;

fuzz_target!(|data: &[u8]| {
    let Some((&first, rest)) = data.split_first() else { return };
    let mut bytes = b"\x1b]".to_vec();
    bytes.extend_from_slice(rest);
    let term: &[u8] = match first % 3 {
        0 => b"\x07",
        1 => b"\x1b\\",
        _ => b"\x9c",
    };
    bytes.extend_from_slice(term);
    let mut t = Terminal::new(20, 5, TerminalConfig::default());
    t.advance(&bytes);
    let _ = t.take_events();
    let _ = t.take_replies();
    common::check(&t);
});
