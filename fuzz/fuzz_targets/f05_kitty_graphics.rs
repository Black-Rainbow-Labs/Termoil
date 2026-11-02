//! F5: the kitty graphics command parser and store.
#![no_main]
use libfuzzer_sys::fuzz_target;
use termoil_vt::graphics::{GraphicsContext, GraphicsStore, parse_kitty_command};

fuzz_target!(|data: &[u8]| {
    let _ = parse_kitty_command(data);
    let mut store = GraphicsStore::new(8 << 20);
    store.set_max_pixels(1 << 20);
    let ctx = GraphicsContext { cols: 80, rows: 24, cell_px: (10, 20), allow_file_transfer: false, ..GraphicsContext::default() };
    for chunk in data.split(|&b| b == b'\n') {
        let _ = store.kitty_command(chunk, &ctx);
    }
    assert!(store.used_bytes() <= 8 << 20);
});
