//! The shaders Termoil ships (task-2590): each one compiles on the GPU renderer, draws a frame that
//! differs from the plain frame, and leaves no pixel black where the frame had text unless it is a
//! shader that may (none do). Skipped, with a printed reason, when no GPU adapter is available.

use std::path::PathBuf;

use termoil_app::cursor_effects::{EFFECTS, compose};
use termoil_app::shaders::BUILTINS;
use termoil_font::{FontConfig, FontGrid};
use termoil_render::{BuildOptions, Colors, Frame, Image, RenderState, Renderer, VelloGpuRenderer, build_frame};
use termoil_vt::{Terminal, TerminalConfig};
use wgpu::Backends as wgpu_backends;

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

/// A frame of colored text over several rows, so every shader has text, color and background to work on.
fn frame(g: &mut FontGrid) -> Frame {
    let mut t = Terminal::new(40, 8, TerminalConfig::default());
    t.advance(b"\x1b[1;32m$\x1b[0m cargo test --workspace\r\n");
    t.advance(b"\x1b[31merror\x1b[0m: \x1b[33mwarning\x1b[0m \x1b[34mblue\x1b[0m \x1b[35mmagenta\x1b[0m\r\n");
    t.advance(b"\x1b[7m reversed \x1b[0m plain text on the background\r\n");
    t.advance(b"test result: ok. 412 passed; 0 failed");
    let mut state = RenderState::new(40, 8, Colors::dark_default());
    state.update(&mut t, 0);
    build_frame(&state, g, &BuildOptions::default())
}

/// Draws `f` and reads the pixels back.
fn draw(r: &mut VelloGpuRenderer, f: &Frame, g: &FontGrid) -> Image {
    r.render(f, g).expect("render");
    r.read_pixels().expect("pixels")
}

/// The share of pixels whose color differs by more than a small amount between two images.
fn changed_share(a: &Image, b: &Image) -> f64 {
    let n = a.rgba.len() / 4;
    let changed = a
        .rgba
        .chunks_exact(4)
        .zip(b.rgba.chunks_exact(4))
        .filter(|(x, y)| (0..3).any(|k| (i32::from(x[k]) - i32::from(y[k])).abs() > 3))
        .count();
    changed as f64 / n as f64
}

/// The number of distinct colors in an image, counted to the nearest 8 levels per channel.
fn distinct_colors(img: &Image) -> usize {
    let mut seen = std::collections::HashSet::new();
    for p in img.rgba.chunks_exact(4) {
        seen.insert((p[0] / 8, p[1] / 8, p[2] / 8));
    }
    seen.len()
}

/// task-2590: the window draws through DX12 on Windows, whose own compiler (FXC) refused crt and
/// tft for writing into a vector at a computed index, though naga and Vulkan accepted them. The
/// installed 1.5.9 then ended with a panic. Every shader is compiled on each backend this machine has.
#[test]
fn every_built_in_shader_compiles_on_every_backend() {
    let backends = [("dx12", wgpu_backends::DX12), ("vulkan", wgpu_backends::VULKAN), ("metal", wgpu_backends::METAL)];
    let mut tried = 0;
    for (name, backend) in backends {
        let Ok(ctx) = termoil_render::GpuContext::with_backends(backend) else { continue };
        tried += 1;
        for b in BUILTINS {
            if let Err(e) = ctx.check_shader(&compose(Some(b.source), None)) {
                panic!("{} does not compile on {name}: {e}", b.name);
            }
        }
        // task-2593: every cursor effect alone, and over every shader, the way the window composes them.
        for e in EFFECTS {
            if let Err(err) = ctx.check_shader(&compose(None, Some(e))) {
                panic!("cursor effect {} does not compile on {name}: {err}", e.name);
            }
            for b in BUILTINS {
                if let Err(err) = ctx.check_shader(&compose(Some(b.source), Some(e))) {
                    panic!("cursor effect {} over {} does not compile on {name}: {err}", e.name, b.name);
                }
            }
        }
    }
    if tried == 0 {
        eprintln!("backend test skipped: no GPU adapter");
    }
}

/// task-2590: a shader DX12's compiler refuses is an error the window shows in its banner, not a
/// panic. Before the internal error scope, this compile ended the program.
#[test]
fn a_shader_dx12_refuses_is_an_error_not_a_crash() {
    let Ok(ctx) = termoil_render::GpuContext::with_backends(wgpu_backends::DX12) else {
        eprintln!("DX12 test skipped: no DX12 adapter");
        return;
    };
    let refused = "fn termoil_shade(uv: vec2<f32>, color: vec4<f32>) -> vec4<f32> {\n    var v = vec3<f32>(0.0);\n    v[u32(uv.x * 3.0)] = 1.0;\n    return vec4<f32>(v, 1.0);\n}\n";
    match ctx.check_shader(refused) {
        Err(e) => assert!(e.to_string().contains("shader"), "{e}"),
        Ok(()) => eprintln!("this DX12 compiler accepts the module, so there is nothing to refuse"),
    }
    assert!(
        ctx.check_shader(&compose(Some(BUILTINS[0].source), None)).is_ok(),
        "the device still works after the refusal"
    );
}

#[test]
fn every_built_in_shader_compiles_and_changes_the_frame() {
    let mut g = grid();
    let f = frame(&mut g);
    let Some(mut r) = VelloGpuRenderer::new_offscreen(f.width, f.height, false) else {
        eprintln!("built in shader test skipped: no GPU adapter");
        return;
    };
    let plain = draw(&mut r, &f, &g);
    // A cursor near the top left, for the shaders that read termoil.cursor.
    r.set_shader_cursor(Some((40.0, 12.0)));
    let mut report = Vec::new();
    for b in BUILTINS {
        r.set_shader(Some(&compose(Some(b.source), None)))
            .unwrap_or_else(|e| panic!("{} does not compile: {e}", b.name));
        let shaded = draw(&mut r, &f, &g);
        let share = changed_share(&plain, &shaded);
        let colors = distinct_colors(&shaded);
        report.push(format!("{:<15} changed {:>5.1}% of pixels, {colors} colors", b.name, share * 100.0));
        // cursor-glow only lights the area round the cursor, so it changes the least.
        let least = if b.name == "cursor-glow" { 0.005 } else { 0.05 };
        assert!(share >= least, "{} changed only {:.2}% of the pixels", b.name, share * 100.0);
        assert!(colors >= 4, "{} left {colors} colors: the text is gone", b.name);
        assert!(shaded.rgba.chunks_exact(4).all(|p| p[3] == 255), "{} changed the alpha of an opaque frame", b.name);
    }
    r.set_shader(None).expect("removing the shader");
    assert_eq!(draw(&mut r, &f, &g), plain, "without a shader the frame is the plain frame again");
    eprintln!("{}", report.join("\n"));
}

/// task-2593: each cursor effect, run alone, changes the pixels near the cursor and keeps the text
/// and an opaque frame. sparkle lights only a few twinkling points, which may all be dark at the
/// moment the frame is drawn, so it is only required to compile and keep the frame.
#[test]
fn every_cursor_effect_changes_the_frame_near_the_cursor() {
    let mut g = grid();
    let f = frame(&mut g);
    let Some(mut r) = VelloGpuRenderer::new_offscreen(f.width, f.height, false) else {
        eprintln!("cursor effect test skipped: no GPU adapter");
        return;
    };
    let plain = draw(&mut r, &f, &g);
    r.set_shader_cursor(Some((40.0, 12.0)));
    let mut report = Vec::new();
    for e in EFFECTS {
        r.set_shader(Some(&compose(None, Some(e)))).unwrap_or_else(|err| panic!("{} does not compile: {err}", e.name));
        let shaded = draw(&mut r, &f, &g);
        let share = changed_share(&plain, &shaded);
        report.push(format!("{:<12} changed {:>5.2}% of pixels", e.name, share * 100.0));
        if e.name != "sparkle" {
            assert!(share > 0.0, "cursor effect {} changed nothing", e.name);
        }
        assert!(distinct_colors(&shaded) >= 4, "cursor effect {} lost the text", e.name);
        assert!(shaded.rgba.chunks_exact(4).all(|p| p[3] == 255), "{} changed the alpha of an opaque frame", e.name);
    }
    r.set_shader_cursor(None);
    r.set_shader(Some(&compose(None, EFFECTS.first()))).expect("glow");
    assert_eq!(draw(&mut r, &f, &g), plain, "with no cursor shown a cursor effect draws nothing");
    eprintln!("{}", report.join("\n"));
}
