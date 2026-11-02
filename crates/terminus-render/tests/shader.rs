//! The custom shader pass (`docs/shaders.md`): a shader that inverts colours changes every pixel as
//! expected, the uniforms reach the shader, and a shader that does not compile returns an error and
//! leaves rendering working. Skipped, with a printed reason, when no GPU adapter is available.

use std::path::PathBuf;

use terminus_font::{FontConfig, FontGrid};
use terminus_render::{
    BuildOptions, Colors, Frame, Image, RenderState, Renderer, VelloGpuRenderer, build_frame, shader_reads_time,
};
use terminus_vt::{Terminal, TerminalConfig};

/// Shader that inverts the colour and keeps the alpha.
const INVERT: &str = "fn terminus_shade(uv: vec2<f32>, color: vec4<f32>) -> vec4<f32> {\n    return vec4<f32>(1.0 - color.rgb, color.a);\n}\n";

/// A grid on the vendored JetBrains Mono.
fn grid() -> FontGrid {
    let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../corpus/fonts");
    let files = ["Regular", "Bold", "Italic", "BoldItalic"]
        .iter()
        .map(|s| dir.join(format!("JetBrainsMono-{s}.ttf")))
        .collect();
    FontGrid::new(FontConfig {
        families: vec!["JetBrains Mono".into()],
        font_files: files,
        system_fonts: false,
        size_px: 16.0,
        ..FontConfig::default()
    })
    .expect("font")
}

/// A frame of coloured text.
fn frame(g: &mut FontGrid) -> Frame {
    let mut t = Terminal::new(12, 3, TerminalConfig::default());
    t.advance(b"\x1b[41m red \x1b[42m green \x1b[0m text\r\nsecond line");
    let mut state = RenderState::new(12, 3, Colors::dark_default());
    state.update(&mut t, 0);
    build_frame(&state, g, &BuildOptions::default())
}

/// A GPU renderer sized for `f`, or None (printing why) without an adapter.
fn renderer(f: &Frame) -> Option<VelloGpuRenderer> {
    let r = VelloGpuRenderer::new_offscreen(f.width, f.height, false);
    if r.is_none() {
        eprintln!("shader test skipped: no GPU adapter");
    }
    r
}

/// Draws `f` and reads the pixels back.
fn draw(r: &mut VelloGpuRenderer, f: &Frame, g: &FontGrid) -> Image {
    r.render(f, g).expect("render");
    r.read_pixels().expect("pixels")
}

#[test]
fn an_inverting_shader_inverts_every_pixel() {
    let mut g = grid();
    let f = frame(&mut g);
    let Some(mut r) = renderer(&f) else { return };
    let plain = draw(&mut r, &f, &g);
    r.set_shader(Some(INVERT)).expect("the invert shader compiles");
    assert!(r.has_shader());
    let inverted = draw(&mut r, &f, &g);
    assert_eq!(plain.rgba.len(), inverted.rgba.len());
    let mut changed = 0;
    for (a, b) in plain.rgba.chunks_exact(4).zip(inverted.rgba.chunks_exact(4)) {
        for k in 0..3 {
            let want = 255 - i32::from(a[k]);
            assert!((want - i32::from(b[k])).abs() <= 2, "channel {k}: plain {} inverted {}", a[k], b[k]);
        }
        assert_eq!(a[3], b[3]);
        changed += usize::from(a[..3] != b[..3]);
    }
    assert!(changed > plain.rgba.len() / 8, "the shader changed {changed} pixels");
    r.set_shader(None).expect("removing the shader");
    assert_eq!(draw(&mut r, &f, &g), plain, "without the shader the frame is the plain frame again");
}

#[test]
fn the_uniforms_reach_the_shader() {
    let mut g = grid();
    let f = frame(&mut g);
    let Some(mut r) = renderer(&f) else { return };
    let source = "fn terminus_shade(uv: vec2<f32>, color: vec4<f32>) -> vec4<f32> {\n    \
        let d = distance(uv * terminus.size, terminus.cursor);\n    \
        if (d < 4.0) { return vec4<f32>(1.0, 0.0, 1.0, 1.0); }\n    return color;\n}\n";
    r.set_shader(Some(source)).expect("compiles");
    r.set_shader_cursor(Some((30.0, 20.0)));
    let img = draw(&mut r, &f, &g);
    let at = |x: usize, y: usize| {
        let i = (y * img.width as usize + x) * 4;
        [img.rgba[i], img.rgba[i + 1], img.rgba[i + 2]]
    };
    assert_eq!(at(30, 20), [255, 0, 255], "the pixel under the cursor");
    assert_ne!(at(60, 40), [255, 0, 255], "a pixel away from the cursor");
    assert!(!r.shader_animates());
}

#[test]
fn a_shader_that_does_not_compile_returns_an_error_and_rendering_continues() {
    let mut g = grid();
    let f = frame(&mut g);
    let Some(mut r) = renderer(&f) else { return };
    let plain = draw(&mut r, &f, &g);
    let broken =
        "fn terminus_shade(uv: vec2<f32>, color: vec4<f32>) -> vec4<f32> {\n    return vec4<f32>(nope, 1.0);\n}\n";
    let err = r.set_shader(Some(broken)).expect_err("an undefined name is an error");
    let text = err.to_string();
    assert!(text.contains("nope"), "the message names the problem: {text}");
    assert!(!r.has_shader());
    assert_eq!(draw(&mut r, &f, &g), plain, "the frame is drawn without the pass");
    let missing = r.set_shader(Some("fn other() {}\n")).expect_err("the function is required").to_string();
    assert!(missing.contains("terminus_shade"), "{missing}");
    assert_eq!(draw(&mut r, &f, &g), plain);
    r.set_shader(Some(INVERT)).expect("a good shader works after a bad one");
}

#[test]
fn the_example_shaders_compile_and_draw() {
    let mut g = grid();
    let f = frame(&mut g);
    let Some(mut r) = renderer(&f) else { return };
    let plain = draw(&mut r, &f, &g);
    let examples = [
        ("crt.wgsl", include_str!("../../../docs/shaders/crt.wgsl"), false),
        ("cursor-glow.wgsl", include_str!("../../../docs/shaders/cursor-glow.wgsl"), true),
    ];
    for (name, source, animates) in examples {
        r.set_shader(Some(source)).unwrap_or_else(|e| panic!("{name}: {e}"));
        assert_eq!(r.shader_animates(), animates, "{name}");
        r.set_shader_cursor(Some((20.0, 10.0)));
        assert_ne!(draw(&mut r, &f, &g), plain, "{name} changes the frame");
    }
}

#[test]
fn only_shaders_that_read_the_time_animate() {
    assert!(shader_reads_time("let t = terminus.time * 2.0;"));
    assert!(!shader_reads_time("// terminus.time is not used\nlet a = 1.0;"));
    assert!(!shader_reads_time(INVERT));
}
