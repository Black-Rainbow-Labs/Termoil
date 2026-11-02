//! R1 to R3 (docs/TEST-SUITE.md section 10).
//!
//! R1 checks the frame builder's records as data. R2 renders scenes with `VelloCpuRenderer` on the
//! vendored JetBrains Mono and compares them with the accepted images in
//! `corpus/render/<platform>/`. R3 renders the same scenes with `VelloGpuRenderer` and compares
//! them with the CPU image under a wider tolerance; it is skipped, with a printed reason, when no
//! GPU adapter is available.

use std::path::PathBuf;

use terminus_font::{FontConfig, FontGrid};
use terminus_render::golden::{self, Tolerance};
use terminus_render::{
    BuildOptions, Colors, CursorStyle, DecorationKind, Frame, RenderState, Renderer, Rgba, VelloCpuRenderer,
    VelloGpuRenderer, build_frame,
};
use terminus_vt::{Terminal, TerminalConfig};

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

/// A terminal fed `bytes`, and the frame built from it.
fn scene(cols: u16, rows: u16, bytes: &[u8], g: &mut FontGrid) -> Frame {
    let mut t = Terminal::new(cols, rows, TerminalConfig::default());
    t.advance(bytes);
    let mut state = RenderState::new(cols, rows, Colors::dark_default());
    state.update(&mut t, 0);
    build_frame(&state, g, &BuildOptions::default())
}

#[test]
fn r1_backgrounds_merge_and_glyphs_sit_on_cells() {
    let mut g = grid(16.0);
    let cw = g.metrics().width as f32;
    let f = scene(20, 3, b"\x1b[41mab\x1b[42mcd\x1b[0m ef", &mut g);
    let row = &f.rows[0];
    assert_eq!(row.backgrounds.len(), 2, "{:?}", row.backgrounds);
    assert_eq!(row.backgrounds[0].x1 - row.backgrounds[0].x0, 2.0 * cw);
    assert_eq!(row.runs.len(), 2, "one run per contiguous face and colour before the space, one after");
    let xs: Vec<f32> = row.runs.iter().flat_map(|r| r.glyphs.iter().map(|g| g.x)).collect();
    for (k, x) in xs.iter().enumerate() {
        let col = (x - 2.0) / cw;
        assert!((col - col.round()).abs() < 0.01, "glyph {k} at x {x} is not on a cell boundary");
    }
}

#[test]
fn r1_runs_split_at_colour_changes() {
    let mut g = grid(16.0);
    let f = scene(20, 2, b"ab\x1b[31mcd\x1b[32mef", &mut g);
    let colors: Vec<Rgba> = f.rows[0].runs.iter().map(|r| r.color).collect();
    assert_eq!(colors.len(), 3);
    assert_ne!(colors[0], colors[1]);
}

/// SGR 8 text stays invisible with `window.minimum_contrast` on (task-2468): the text colour is the
/// background, and the contrast lift used to bring it back to 4.5:1, so concealed text was readable.
#[test]
fn r1_hidden_text_is_not_lifted_by_minimum_contrast() {
    let mut g = grid(16.0);
    let mut t = Terminal::new(20, 2, TerminalConfig::default());
    t.advance(b"\x1b[8msecret\x1b[0m shown");
    let mut state = RenderState::new(20, 2, Colors::dark_default());
    state.update(&mut t, 0);
    let opts = BuildOptions { minimum_contrast: 4.5, ..BuildOptions::default() };
    let f = build_frame(&state, &mut g, &opts);
    let bg = Colors::dark_default().background;
    let hidden =
        f.rows[0].runs.iter().find(|r| r.glyphs.first().is_some_and(|gl| gl.x < g.metrics().width as f32 * 6.0));
    if let Some(run) = hidden {
        assert_eq!(
            (run.color.r, run.color.g, run.color.b),
            (bg.r, bg.g, bg.b),
            "hidden text was drawn in {:?}",
            run.color
        );
    }
}

#[test]
fn r1_decorations() {
    let mut g = grid(16.0);
    let f = scene(20, 2, b"\x1b[4mul\x1b[0m \x1b[4:3mcu\x1b[0m \x1b[9mst\x1b[0m \x1b[21mdb\x1b[0m", &mut g);
    let kinds: Vec<DecorationKind> = f.rows[0].decorations.iter().map(|d| d.kind).collect();
    assert_eq!(
        kinds,
        [
            DecorationKind::Underline,
            DecorationKind::CurlyUnderline,
            DecorationKind::Strike,
            DecorationKind::DoubleUnderline
        ]
    );
    let ul = f.rows[0].decorations[0];
    assert_eq!(ul.x1 - ul.x0, 2.0 * g.metrics().width as f32, "one underline across both cells");
}

#[test]
fn r1_cursor_shapes() {
    let mut g = grid(16.0);
    for (seq, style) in
        [(&b"\x1b[2 q"[..], CursorStyle::Block), (b"\x1b[6 q", CursorStyle::Bar), (b"\x1b[4 q", CursorStyle::Underline)]
    {
        let f = scene(10, 2, seq, &mut g);
        assert_eq!(f.cursor.map(|c| c.style), Some(style));
    }
    let f = scene(10, 2, b"\x1b[?25l", &mut g);
    assert!(f.cursor.is_none());
    let f = scene(10, 2, "中\x1b[D\x1b[D".as_bytes(), &mut g);
    assert_eq!(
        f.cursor.map(|c| c.width),
        Some(2.0 * g.metrics().width as f32),
        "a cursor on a wide character covers two cells"
    );
}

#[test]
fn r1_sprites_and_wide_cells() {
    let mut g = grid(16.0);
    let f = scene(10, 2, "─│┌█⠿\u{e0b0}中x".as_bytes(), &mut g);
    let sprites: String = f.rows[0].sprites.iter().map(|s| s.ch).collect();
    assert_eq!(sprites, "─│┌█⠿\u{e0b0}");
    let glyphs: usize = f.rows[0].runs.iter().map(|r| r.glyphs.len()).sum();
    assert_eq!(glyphs, 2, "the wide character and x");
}

#[test]
fn r1_only_dirty_rows_are_rebuilt() {
    let mut g = grid(16.0);
    let mut t = Terminal::new(10, 5, TerminalConfig::default());
    t.advance(b"one\r\ntwo\r\nthree");
    let mut state = RenderState::new(10, 5, Colors::dark_default());
    state.update(&mut t, 0);
    let opts = BuildOptions { all_rows: false, ..BuildOptions::default() };
    assert_eq!(build_frame(&state, &mut g, &opts).rows.len(), 5);
    t.advance(b"\x1b[1;1HX");
    state.update(&mut t, 0);
    let rows: Vec<u16> = build_frame(&state, &mut g, &opts).rows.iter().map(|r| r.row).collect();
    assert_eq!(rows, [0, 2], "the row written and the row the cursor left");
}

/// The scenes the pixel tests draw: name, columns, rows, bytes, font size.
fn pixel_scenes() -> Vec<(&'static str, u16, u16, Vec<u8>, f32)> {
    let attrs = b"\x1b[1mbold\x1b[0m \x1b[3mitalic\x1b[0m \x1b[2mfaint\x1b[0m \x1b[7minverse\x1b[0m \x1b[9mstrike\x1b[0m \x1b[53mover\x1b[0m\r\n\x1b[31mred\x1b[32mgreen\x1b[34mblue\x1b[38;2;255;128;0mrgb\x1b[48;5;24mbg\x1b[0m";
    let underlines = b"\x1b[4:1msingle\x1b[0m \x1b[4:2mdouble\x1b[0m \x1b[4:3mcurly\x1b[0m\r\n\x1b[4:4mdotted\x1b[0m \x1b[4:5mdashed\x1b[0m \x1b[4:3;58;5;196mred curl\x1b[0m";
    let boxes =
        "┌──┬──┐ ╭─╮ ═══ ▀▄█ ░▒▓\r\n│ab│cd│ ╰─╯ ║║║ ⠿⡇⢸ \u{e0b0}\u{e0b2}\r\n└──┴──┘ ╱╲╳ ━━━ ▖▗▘ \u{e0b4}\u{e0b6}"
            .as_bytes();
    let ligature = b"=> != === -> \x1b[31m=\x1b[32m>\x1b[0m |> <=";
    vec![
        ("attributes", 48, 2, attrs.to_vec(), 16.0),
        ("underlines", 40, 2, underlines.to_vec(), 16.0),
        ("box_drawing_16", 30, 3, boxes.to_vec(), 16.0),
        ("box_drawing_11", 30, 3, boxes.to_vec(), 11.0),
        ("box_drawing_27", 30, 3, boxes.to_vec(), 27.0),
        ("ligature_split", 30, 1, ligature.to_vec(), 16.0),
        ("cursor_block", 6, 1, b"ab\x1b[D".to_vec(), 16.0),
        ("cursor_bar", 6, 1, b"\x1b[6 qab\x1b[D".to_vec(), 16.0),
        ("wide", 10, 1, "a中b".as_bytes().to_vec(), 16.0),
    ]
}

/// Renders a frame with the CPU renderer.
fn render_cpu(f: &Frame, g: &FontGrid) -> terminus_render::Image {
    let mut r = VelloCpuRenderer::new(f.width, f.height, 0, false);
    r.render(f, g).expect("render");
    r.read_pixels().expect("pixels")
}

#[test]
fn r2_cpu_pixel_goldens() {
    let dir = golden::platform_dir(&corpus());
    let mut failures = Vec::new();
    for (name, cols, rows, bytes, size) in pixel_scenes() {
        let mut g = grid(size);
        let f = scene(cols, rows, &bytes, &mut g);
        let img = render_cpu(&f, &g);
        if let Err(e) = golden::check(&dir, name, &img, Tolerance::CPU, (f.cell_width, f.cell_height), f.padding) {
            failures.push(e);
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

#[test]
fn r3_gpu_matches_cpu() {
    let mut gpu: Option<VelloGpuRenderer> = None;
    let mut failures = Vec::new();
    for (name, cols, rows, bytes, size) in pixel_scenes() {
        let mut g = grid(size);
        let f = scene(cols, rows, &bytes, &mut g);
        let r = match gpu.as_mut() {
            Some(r) => {
                r.resize(f.width, f.height);
                r
            }
            None => match VelloGpuRenderer::new_offscreen(f.width, f.height, false) {
                Some(r) => gpu.insert(r),
                None => {
                    eprintln!("R3 skipped: no GPU adapter");
                    return;
                }
            },
        };
        r.render(&f, &g).expect("gpu render");
        let gpu_img = r.read_pixels().expect("gpu pixels");
        let cpu_img = render_cpu(&f, &g);
        if let Some(why) = golden::compare(&cpu_img, &gpu_img, Tolerance::GPU, (f.cell_width, f.cell_height), f.padding)
        {
            let dir = corpus().join("..").join("_agent_output").join("r3");
            let _ = std::fs::create_dir_all(&dir);
            let _ = golden::write_png(&dir.join(format!("{name}.gpu.png")), &gpu_img);
            let _ = golden::write_png(&dir.join(format!("{name}.cpu.png")), &cpu_img);
            failures.push(format!("{name}: {why}"));
        }
    }
    eprintln!("R3 ran on {}", gpu.as_ref().map(|g| g.name()).unwrap_or_default());
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

#[test]
fn r1_a_scroll_rebuilds_every_row() {
    let mut g = grid(16.0);
    let mut t = Terminal::new(10, 4, TerminalConfig::default());
    t.advance(b"1\r\n2\r\n3\r\n4");
    let mut state = RenderState::new(10, 4, Colors::dark_default());
    state.update(&mut t, 0);
    let opts = BuildOptions { all_rows: false, ..BuildOptions::default() };
    t.advance(b"\r\n5");
    state.update(&mut t, 0);
    let f = build_frame(&state, &mut g, &opts);
    assert_eq!(f.rows.len(), 4, "every visible row moved");
    let firsts: Vec<char> = state.lines.iter().map(|l| l.cells[0].text.first().unwrap_or(' ')).collect();
    assert_eq!(firsts, ['2', '3', '4', '5']);
}

#[test]
fn r4_idle_terminal_builds_no_rows() {
    let mut g = grid(16.0);
    let mut t = Terminal::new(10, 4, TerminalConfig::default());
    t.advance(b"text");
    let mut state = RenderState::new(10, 4, Colors::dark_default());
    state.update(&mut t, 0);
    let opts = BuildOptions { all_rows: false, ..BuildOptions::default() };
    for _ in 0..100 {
        state.update(&mut t, 0);
        assert!(build_frame(&state, &mut g, &opts).rows.is_empty(), "an unchanged terminal has nothing to draw");
    }
}

#[test]
fn r1_frame_builder_reuses_rows_that_only_moved() {
    let mut g = grid(16.0);
    let mut t = Terminal::new(10, 4, TerminalConfig::default());
    t.advance(b"1\r\n2\r\n3\r\n4");
    let mut state = RenderState::new(10, 4, Colors::dark_default());
    let mut b = terminus_render::FrameBuilder::new();
    state.update(&mut t, 0);
    let first = b.build(&state, &mut g, &BuildOptions::default());
    t.advance(b"\r\n5");
    state.update(&mut t, 0);
    let second = b.build(&state, &mut g, &BuildOptions::default());
    assert_eq!(second.rows.len(), 4);
    assert_eq!(b.stats(), (5, 3), "four rows built first, then one built and three moved");
    assert_eq!(second.rows[0].runs, {
        let mut r = first.rows[1].runs.clone();
        let dy = second.rows[0].y - first.rows[1].y;
        r.iter_mut().flat_map(|x| x.glyphs.iter_mut()).for_each(|gl| gl.y += dy);
        r
    });
    let full = build_frame(&state, &mut g, &BuildOptions::default());
    assert_eq!(second.rows, full.rows, "reused rows equal freshly built ones");
}

/// An OSC 111 reset returns the background to the configured default, not to the colour an
/// earlier OSC 11 set.
#[test]
fn r1_osc_reset_returns_to_configured_colours() {
    let mut t = Terminal::new(4, 1, TerminalConfig::default());
    let mut colors = Colors::dark_default();
    colors.background = terminus_vt::style::Rgb::new(1, 2, 3);
    let mut state = RenderState::new(4, 1, colors);
    t.advance(b"\x1b]11;rgb:ff/00/00\x1b\\");
    state.update(&mut t, 0);
    assert_eq!(state.colors.background, terminus_vt::style::Rgb::new(255, 0, 0));
    t.advance(b"\x1b]111\x1b\\");
    state.update(&mut t, 0);
    assert_eq!(state.colors.background, terminus_vt::style::Rgb::new(1, 2, 3));
}

/// Two frames drawn as layers side by side give the same pixels as each frame drawn alone.
#[test]
fn r2_layers_match_frames_drawn_alone() {
    let mut g = grid(16.0);
    let a = scene(6, 2, b"\x1b[31mleft\r\nAB", &mut g);
    let b = scene(6, 2, b"\x1b[44mright\x1b[m\r\nCD", &mut g);
    let (alone_a, alone_b) = (render_cpu(&a, &g), render_cpu(&b, &g));
    let mut r = VelloCpuRenderer::new(a.width + b.width, a.height, 0, false);
    let layers = [
        terminus_render::Layer { x: 0.0, y: 0.0, frame: &a },
        terminus_render::Layer { x: a.width as f32, y: 0.0, frame: &b },
    ];
    r.render_layers(Rgba::rgb(0, 0, 0), &layers, &g).expect("render");
    let both = r.read_pixels().expect("pixels");
    for y in 0..a.height as usize {
        let row = &both.rgba[y * both.width as usize * 4..(y + 1) * both.width as usize * 4];
        let wa = a.width as usize * 4;
        assert_eq!(&row[..wa], &alone_a.rgba[y * wa..(y + 1) * wa], "left pane row {y}");
        assert_eq!(
            &row[wa..],
            &alone_b.rgba[y * b.width as usize * 4..(y + 1) * b.width as usize * 4],
            "right pane row {y}"
        );
    }
}

/// The link under the pointer is underlined over exactly its cells, in the text colour.
#[test]
fn r1_hovered_link_is_underlined() {
    let mut g = grid(16.0);
    let mut t = Terminal::new(10, 2, TerminalConfig::default());
    t.advance(b"go here ok");
    let mut state = RenderState::new(10, 2, Colors::dark_default());
    state.update(&mut t, 0);
    state.link_hover = vec![(0, 3, 6)];
    let f = build_frame(&state, &mut g, &BuildOptions::default());
    let m = f.cell_width as f32;
    let under: Vec<_> = f.rows[0].decorations.iter().filter(|d| d.kind == DecorationKind::Underline).collect();
    assert_eq!(under.len(), 1);
    assert_eq!((under[0].x0, under[0].x1), (f.padding.0 as f32 + 3.0 * m, f.padding.0 as f32 + 7.0 * m));
    assert!(f.rows[1].decorations.is_empty());
}

/// Search matches get the palette's yellow behind the text; the selection still wins over them.
#[test]
fn r1_search_hits_are_highlighted() {
    let mut g = grid(16.0);
    let mut t = Terminal::new(10, 1, TerminalConfig::default());
    t.advance(b"ab ab ab");
    let colors = Colors::dark_default();
    let yellow = colors.palette[3];
    let mut state = RenderState::new(10, 1, colors);
    state.update(&mut t, 0);
    state.search_hits = vec![(0, 0, 1), (0, 3, 4)];
    state.selection = vec![(0, 3, 4)];
    let f = build_frame(&state, &mut g, &BuildOptions::default());
    let spans = &f.rows[0].backgrounds;
    let y = Rgba::rgb(yellow.r, yellow.g, yellow.b);
    assert!(spans.iter().any(|s| s.color == y && s.x0 == f.padding.0 as f32), "{spans:?}");
    assert_eq!(spans.iter().filter(|s| s.color == y).count(), 1, "the selected match is drawn as the selection");
}

/// The OpenGL backend starts a device where the machine has OpenGL (PRD 5.2: wgpu on Vulkan,
/// Metal, DX12 and OpenGL as the platform allows). A machine without it skips.
#[test]
fn opengl_backend_starts_a_device() {
    match terminus_render::GpuContext::with_backends(wgpu::Backends::GL) {
        Ok(ctx) => assert_eq!(ctx.info().backend, wgpu::Backend::Gl, "{:?}", ctx.info()),
        Err(e) => println!("no OpenGL adapter here; skipping: {e}"),
    }
}

/// `window.opacity`: with a translucent window colour the empty part of the window keeps that
/// alpha (drawn once, not once per row), and a cell with a background of its own stays opaque.
#[test]
fn translucent_window_colour_leaves_coloured_cells_opaque() {
    let mut g = grid(16.0);
    let mut t = Terminal::new(10, 2, TerminalConfig::default());
    t.advance(b"\x1b[41m  \x1b[0m");
    let mut state = RenderState::new(10, 2, Colors::dark_default());
    state.update(&mut t, 0);
    let opts = BuildOptions { background_alpha: 128, ..BuildOptions::default() };
    let f = build_frame(&state, &mut g, &opts);
    assert_eq!(f.background.a, 128);
    let mut r = VelloCpuRenderer::new(f.width, f.height, 1, false);
    let layer = terminus_render::Layer { x: 0.0, y: 0.0, frame: &f };
    r.render_layers(f.background, &[layer], &g).expect("render");
    let img = r.read_pixels().expect("pixels");
    let alpha = |x: u32, y: u32| img.rgba[((y * img.width + x) * 4 + 3) as usize];
    let (cw, ch) = (f.cell_width, f.cell_height);
    let (px, py) = f.padding;
    // Column 5, row 1: no colour of its own, so the window colour's alpha.
    let empty = alpha(px + 5 * cw + cw / 2, py + ch + ch / 2);
    assert!((126..=130).contains(&empty), "empty cell alpha {empty}");
    // Column 0, row 0: red background, opaque.
    assert_eq!(alpha(px + cw / 2, py + ch / 2), 255);
}
