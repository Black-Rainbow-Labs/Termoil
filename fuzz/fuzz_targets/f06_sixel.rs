//! F6: the sixel decoder.
#![no_main]
use libfuzzer_sys::fuzz_target;
use termoil_vt::graphics::{GraphicsContext, GraphicsStore};

fuzz_target!(|data: &[u8]| {
    let Some((&first, rest)) = data.split_first() else { return };
    let mut store = GraphicsStore::new(8 << 20);
    store.set_max_pixels(1 << 20);
    let ctx = GraphicsContext { cols: 80, rows: 24, cell_px: (10, 20), ..GraphicsContext::default() };
    let params = [first as u16 % 10, (first / 10) as u16 % 3];
    let _ = store.sixel(&params, rest, &ctx);
});
