//! F7: the PNG decode path with a size cap, through a kitty transmit of format 100.
#![no_main]
use base64::Engine;
use libfuzzer_sys::fuzz_target;
use termoil_vt::graphics::{GraphicsContext, GraphicsStore};

fuzz_target!(|data: &[u8]| {
    let mut store = GraphicsStore::new(8 << 20);
    store.set_max_pixels(1 << 20);
    let ctx = GraphicsContext { cols: 80, rows: 24, cell_px: (10, 20), ..GraphicsContext::default() };
    let payload = base64::engine::general_purpose::STANDARD.encode(data);
    let cmd = format!("a=T,f=100,i=1;{payload}");
    let _ = store.kitty_command(cmd.as_bytes(), &ctx);
});
