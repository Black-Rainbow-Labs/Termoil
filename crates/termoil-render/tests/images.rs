//! R1 to R3 for images (docs/TEST-SUITE.md section 10): graphics store placements become image
//! records at the right pixel rectangles in the right z group, and the CPU and GPU renderers draw them.

use std::path::PathBuf;

use base64::Engine;
use base64::engine::general_purpose::STANDARD;
use termoil_font::{FontConfig, FontGrid};
use termoil_render::golden::{self, Tolerance};
use termoil_render::{
    BuildOptions, Colors, Frame, FrameBuilder, ImageRecord, RenderState, Renderer, VelloCpuRenderer, VelloGpuRenderer,
    build_frame, diacritic_for,
};
use termoil_vt::{Terminal, TerminalConfig};

/// The corpus folder.
fn corpus() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../corpus")
}

/// A grid on the vendored JetBrains Mono at `size` pixels.
fn grid(size: f32) -> FontGrid {
    let dir = corpus().join("fonts");
    let files = ["Regular", "Bold", "Italic", "BoldItalic"]
        .iter()
        .map(|s| dir.join(format!("JetBrainsMono-{s}.ttf")))
        .collect();
    FontGrid::new(FontConfig {
        families: vec!["JetBrains Mono".into()],
        font_files: files,
        system_fonts: false,
        size_px: size,
        ..FontConfig::default()
    })
    .expect("font")
}

/// A terminal whose cell size matches the font grid.
fn terminal(cols: u16, rows: u16, g: &FontGrid) -> Terminal {
    let mut t = Terminal::new(cols, rows, TerminalConfig::default());
    t.set_cell_pixels(g.metrics().width as u16, g.metrics().height as u16);
    t
}

/// A test picture: red, green, blue and white quadrants, the white one half transparent.
fn picture(w: usize, h: usize) -> Vec<u8> {
    let mut px = Vec::with_capacity(w * h * 4);
    for y in 0..h {
        for x in 0..w {
            px.extend_from_slice(match (x < w / 2, y < h / 2) {
                (true, true) => &[255, 0, 0, 255],
                (false, true) => &[0, 200, 0, 255],
                (true, false) => &[0, 0, 255, 255],
                (false, false) => &[255, 255, 255, 128],
            });
        }
    }
    px
}

/// The escape sequence that transmits a picture with `keys` (for example `a=T,i=1`).
fn kitty(keys: &str, w: usize, h: usize) -> Vec<u8> {
    let data = STANDARD.encode(picture(w, h));
    format!("\x1b_G{keys},f=32,s={w},v={h},q=2;{data}\x1b\\").into_bytes()
}

/// A frame built from a terminal's current state.
fn frame_of(t: &mut Terminal, scroll: usize, g: &mut FontGrid) -> (RenderState, Frame) {
    let mut state = RenderState::new(t.cols(), t.rows(), Colors::dark_default());
    state.update(t, scroll);
    let f = build_frame(&state, g, &BuildOptions::default());
    (state, f)
}

/// Every record of a frame, in the order drawn.
fn all_records(f: &Frame) -> Vec<&ImageRecord> {
    f.images_below.iter().chain(&f.images_mid).chain(&f.images_above).collect()
}

#[test]
fn r1_a_placement_becomes_a_record_at_its_pixel_rectangle() {
    let mut g = grid(16.0);
    let (cw, ch) = (g.metrics().width as f32, g.metrics().height as f32);
    let mut t = terminal(20, 8, &g);
    t.advance(b"\x1b[3;4H");
    t.advance(&kitty("a=T,i=1,C=1", 48, 32));
    let (_, f) = frame_of(&mut t, 0, &mut g);
    assert_eq!((f.images_below.len(), f.images_mid.len(), f.images_above.len()), (0, 0, 1));
    let r = &f.images_above[0];
    let (px, py) = (f.padding.0 as f32, f.padding.1 as f32);
    assert_eq!((r.dest.x0, r.dest.y0), (px + 3.0 * cw, py + 2.0 * ch));
    assert_eq!((r.dest.x1 - r.dest.x0, r.dest.y1 - r.dest.y0), (48.0, 32.0), "drawn at its own size");
    assert_eq!((r.src.x0, r.src.y0, r.src.x1, r.src.y1), (0.0, 0.0, 48.0, 32.0));
    assert_eq!(f.live_images, [(r.image.id, r.image.generation)]);
}

#[test]
fn r1_z_index_picks_the_group() {
    let mut g = grid(16.0);
    let mut t = terminal(30, 8, &g);
    for (i, z) in [(1, -1_500_000_000i64), (2, -5), (3, 0), (4, 7)] {
        t.advance(format!("\x1b[1;{}H", i * 4).as_bytes());
        t.advance(&kitty(&format!("a=T,i={i},C=1,z={z}"), 8, 8));
    }
    let (_, f) = frame_of(&mut t, 0, &mut g);
    let ids = |v: &[ImageRecord]| v.iter().map(|r| r.image.id).collect::<Vec<_>>();
    assert_eq!(ids(&f.images_below), [1]);
    assert_eq!(ids(&f.images_mid), [2]);
    assert_eq!(ids(&f.images_above), [3, 4], "within a group the lower z draws first");
}

#[test]
fn r1_an_image_is_clipped_at_the_bottom_of_the_grid() {
    let mut g = grid(16.0);
    let ch = g.metrics().height as f32;
    let mut t = terminal(20, 4, &g);
    t.advance(b"\x1b[4;1H");
    t.advance(&kitty("a=T,i=1,C=1", 48, 40));
    let (_, f) = frame_of(&mut t, 0, &mut g);
    let r = &f.images_above[0];
    let grid_bottom = f.padding.1 as f32 + 4.0 * ch;
    assert_eq!(r.dest.y1, grid_bottom, "cut at the last row");
    let shown = r.dest.y1 - r.dest.y0;
    assert_eq!(shown, ch);
    assert_eq!(r.src.y1, shown, "the source is cut by the same amount at the same scale");
    assert_eq!(r.src.y0, 0.0);
}

#[test]
fn r1_a_scrolled_view_moves_the_image_and_keeps_its_generation() {
    let mut g = grid(16.0);
    let ch = g.metrics().height as f32;
    let mut t = terminal(20, 4, &g);
    t.advance(b"\x1b[2;1H");
    t.advance(&kitty("a=T,i=1,C=1", 8, 8));
    t.advance(b"\x1b[4;1H1\r\n2\r\n3\r\n4\r\n5\r\n6");
    let mut state = RenderState::new(20, 4, Colors::dark_default());
    state.update(&mut t, 0);
    let live = build_frame(&state, &mut g, &BuildOptions::default());
    assert!(all_records(&live).is_empty(), "scrolled out of the active area");
    state.update(&mut t, 5);
    let f = build_frame(&state, &mut g, &BuildOptions::default());
    let r = &f.images_above[0];
    assert_eq!(r.dest.y0, f.padding.1 as f32 + ch, "second row of the scrolled view");
    state.update(&mut t, 4);
    let g2 = build_frame(&state, &mut g, &BuildOptions::default());
    assert_eq!(g2.images_above[0].dest.y0, f.padding.1 as f32, "one row higher after one row less scroll");
    assert_eq!(g2.images_above[0].image.generation, r.image.generation, "a scroll does not make a new image");
    assert_eq!(live.live_images, f.live_images, "an image off screen is still alive");
}

#[test]
fn r1_unicode_placeholders_resolve_to_the_cells_they_name() {
    let mut g = grid(16.0);
    let (cw, ch) = (g.metrics().width as f32, g.metrics().height as f32);
    let mut t = terminal(20, 6, &g);
    t.advance(&kitty("a=t,i=7", 4 * cw as usize, 2 * ch as usize));
    t.advance(b"\x1b_Ga=p,i=7,U=1,c=4,r=2,q=2\x1b\\");
    let mark = |n| diacritic_for(n).unwrap();
    let mut text = String::from("\x1b[3;5H\x1b[38;5;7m");
    for row in [0u32, 1] {
        for c in 0..4 {
            text.push('\u{10EEEE}');
            text.push(mark(row));
            text.push(mark(c));
        }
        text.push_str("\x1b[4;5H");
    }
    t.advance(text.as_bytes());
    let (_, f) = frame_of(&mut t, 0, &mut g);
    let rs = all_records(&f);
    assert_eq!(rs.len(), 2, "one record for each row of placeholders");
    let (px, py) = (f.padding.0 as f32, f.padding.1 as f32);
    for (k, r) in rs.iter().enumerate() {
        assert_eq!(r.dest.x0, px + 4.0 * cw);
        assert_eq!(r.dest.x1, px + 8.0 * cw, "four cells wide");
        assert_eq!(r.dest.y0, py + (2 + k) as f32 * ch);
        assert_eq!(r.dest.y1, r.dest.y0 + ch, "one cell tall");
    }
    assert!(rs[0].src.y1 <= rs[1].src.y0 + 0.01, "the two rows show different halves of the image");
    assert_eq!(f.glyph_count(), 0, "a placeholder cell draws no glyph");
}

#[test]
fn r1_rows_under_a_changed_image_are_dirty_and_the_cache_follows_the_store() {
    let mut g = grid(16.0);
    let mut t = terminal(20, 8, &g);
    let mut state = RenderState::new(20, 8, Colors::dark_default());
    let opts = BuildOptions { all_rows: false, ..BuildOptions::default() };
    state.update(&mut t, 0);
    state.update(&mut t, 0);
    assert!(build_frame(&state, &mut g, &opts).rows.is_empty());
    t.advance(b"\x1b[4;1H");
    t.advance(&kitty("a=T,i=1,C=1", 8, 40));
    state.update(&mut t, 0);
    let rows: Vec<u16> = build_frame(&state, &mut g, &opts).rows.iter().map(|r| r.row).collect();
    assert!(rows.contains(&3) && rows.contains(&4) && rows.contains(&5), "{rows:?}");
    let mut b = FrameBuilder::new();
    let mut cpu = VelloCpuRenderer::new(100, 100, 0, false);
    let f = b.build(&state, &mut g, &BuildOptions::default());
    cpu.render(&f, &g).unwrap();
    cpu.render(&f, &g).unwrap();
    assert_eq!(cpu.cached_images(), 1, "uploaded once and reused");
    t.advance(b"\x1b_Ga=d,d=A\x1b\\");
    state.update(&mut t, 0);
    let f = b.build(&state, &mut g, &BuildOptions::default());
    assert!(all_records(&f).is_empty());
    cpu.render(&f, &g).unwrap();
    assert_eq!(cpu.cached_images(), 0, "dropped when the store drops it");
}

/// The scenes the pixel tests draw: name, columns, rows, bytes.
fn scenes() -> Vec<(&'static str, u16, u16, Vec<u8>)> {
    let text = b"\x1b[1;1Hunder the picture\r\n\x1b[44mblue background line\x1b[0m\r\nlast line of text";
    let place = |z: &str, at: &str| {
        let mut v = text.to_vec();
        v.extend_from_slice(at.as_bytes());
        v.extend(kitty(&format!("a=T,i=1,C=1,z={z},c=8,r=3"), 48, 32));
        v
    };
    let mut clipped = b"\x1b[1;1Hclipped".to_vec();
    clipped.extend_from_slice(b"\x1b[4;3H");
    clipped.extend(kitty("a=T,i=1,C=1,c=6,r=3", 48, 32));
    vec![
        ("image_under_text", 24, 4, place("-1", "\x1b[1;6H")),
        ("image_above_text", 24, 4, place("0", "\x1b[1;6H")),
        ("image_clipped", 16, 4, clipped),
    ]
}

/// Renders a frame with the CPU renderer.
fn render_cpu(f: &Frame, g: &FontGrid) -> termoil_render::Image {
    let mut r = VelloCpuRenderer::new(f.width, f.height, 0, false);
    r.render(f, g).expect("render");
    r.read_pixels().expect("pixels")
}

/// The frame for a scene.
fn scene_frame(cols: u16, rows: u16, bytes: &[u8], g: &mut FontGrid) -> Frame {
    let mut t = terminal(cols, rows, g);
    t.advance(bytes);
    frame_of(&mut t, 0, g).1
}

#[test]
fn r2_cpu_image_goldens() {
    let dir = golden::platform_dir(&corpus());
    let mut failures = Vec::new();
    for (name, cols, rows, bytes) in scenes() {
        let mut g = grid(16.0);
        let f = scene_frame(cols, rows, &bytes, &mut g);
        assert!(!all_records(&f).is_empty(), "{name} has an image");
        let img = render_cpu(&f, &g);
        if let Err(e) = golden::check(&dir, name, &img, Tolerance::CPU, (f.cell_width, f.cell_height), f.padding) {
            failures.push(e);
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

#[test]
fn r3_gpu_matches_cpu_with_images() {
    let mut failures = Vec::new();
    for (name, cols, rows, bytes) in scenes() {
        let mut g = grid(16.0);
        let f = scene_frame(cols, rows, &bytes, &mut g);
        let Some(mut gpu) = VelloGpuRenderer::new_offscreen(f.width, f.height, false) else {
            eprintln!("R3 images skipped: no GPU adapter");
            return;
        };
        gpu.render(&f, &g).expect("gpu render");
        let gpu_img = gpu.read_pixels().expect("gpu pixels");
        let cpu_img = render_cpu(&f, &g);
        if let Some(why) = golden::compare(&cpu_img, &gpu_img, Tolerance::GPU, (f.cell_width, f.cell_height), f.padding)
        {
            let dir = corpus().join("..").join("_agent_output").join("r3");
            let _ = std::fs::create_dir_all(&dir);
            let _ = golden::write_png(&dir.join(format!("{name}.gpu.png")), &gpu_img);
            let _ = golden::write_png(&dir.join(format!("{name}.cpu.png")), &cpu_img);
            failures.push(format!("{name}: {why}"));
        }
        assert_eq!(gpu.cached_images(), 1);
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}
