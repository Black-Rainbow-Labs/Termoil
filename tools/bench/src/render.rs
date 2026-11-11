//! R4, the M2 gate (docs/TEST-SUITE.md section 10, docs/TDD.md section 4.7): a 200 by 60 grid,
//! a full screen scroll of mixed coloured text every frame, and the time from taking the render
//! state to the finished frame. The gate is p99 under 8 ms with `vello_gpu` on this machine.
//!
//! Each frame appends one line, which scrolls every row, so every frame rebuilds and redraws the
//! whole grid: the worst case for a renderer that redraws per frame.

use std::time::{Duration, Instant};

use termoil_font::{FontConfig, FontGrid};
use termoil_render::{BuildOptions, Colors, FrameBuilder, RenderState, Renderer, VelloCpuRenderer, VelloGpuRenderer};
use termoil_vt::{Terminal, TerminalConfig};

/// One renderer's frame times.
pub struct RenderResult {
    /// Renderer description.
    pub name: String,
    /// Frame times, sorted.
    pub times: Vec<Duration>,
    /// Mean time spent taking the render state and building the frame.
    pub build: Duration,
    /// Glyphs per frame.
    pub glyphs: usize,
    /// Encode times (the renderer building its scene), sorted.
    pub encode: Vec<Duration>,
}

impl RenderResult {
    /// The given percentile of the frame times.
    pub fn percentile(&self, p: f64) -> Duration {
        let i = ((self.times.len() as f64 - 1.0) * p).round() as usize;
        self.times[i.min(self.times.len() - 1)]
    }
}

/// A line of mixed coloured text 200 columns wide, different for each `n`.
fn line(n: usize) -> Vec<u8> {
    let words = [
        "cargo",
        "build",
        "termoil",
        "warning:",
        "src/page.rs:42",
        "Finished",
        "release",
        "=>",
        "!=",
        "fn",
        "let",
        "0x1f",
        "─────",
        "test",
        "ok",
    ];
    let mut out = Vec::new();
    let mut col = 0;
    let mut k = n;
    while col < 190 {
        k = k.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
        let w = words[(k >> 33) % words.len()];
        out.extend_from_slice(format!("\x1b[{}m{w} ", 31 + (k >> 40) % 7).as_bytes());
        col += w.chars().count() + 1;
    }
    out.extend_from_slice(b"\x1b[0m\r\n");
    out
}

/// Runs `frames` frames through `renderer` and returns the sorted frame times.
fn run(renderer: &mut dyn Renderer, grid: &mut FontGrid, frames: usize) -> RenderResult {
    let mut term = Terminal::new(200, 60, TerminalConfig::default());
    for n in 0..60 {
        term.advance(&line(n));
    }
    let mut state = RenderState::new(200, 60, Colors::dark_default());
    let opts = BuildOptions::default();
    let mut builder = FrameBuilder::new();
    let mut times = Vec::with_capacity(frames);
    let (mut build_total, mut glyphs) = (Duration::ZERO, 0);
    let mut encode = Vec::with_capacity(frames);
    for n in 0..frames + 20 {
        term.advance(&line(n + 60));
        let start = Instant::now();
        state.update(&mut term, 0);
        let frame = builder.build(&state, grid, &opts);
        let built = start.elapsed();
        let stats = renderer.render(&frame, grid).expect("render");
        if n >= 20 {
            times.push(start.elapsed());
            build_total += built;
            glyphs = stats.glyphs;
            encode.push(stats.encode);
        }
    }
    times.sort();
    encode.sort();
    RenderResult { name: renderer.name(), build: build_total / frames as u32, times, glyphs, encode }
}

/// The font the gate uses: the configured default family at 13 points (Cascadia Code here).
fn gate_grid() -> FontGrid {
    FontGrid::new(FontConfig::default()).expect("default font")
}

/// Measures every renderer and prints the table. Returns the vello_gpu result for the gate.
pub fn measure(frames: usize) -> Vec<RenderResult> {
    let mut grid = gate_grid();
    let m = *grid.metrics();
    let (w, h) = (200 * m.width + 4, 60 * m.height + 4);
    println!(
        "R4: 200x60 cells of {}x{} px ({}x{} px frame), font {} at {:.2} px, {frames} frames, full screen scroll every frame",
        m.width,
        m.height,
        w,
        h,
        grid.face(grid.primary(termoil_font::FontStyle::Regular)).family,
        grid.size_px()
    );
    let mut results = Vec::new();
    for atlas in [false, true] {
        match VelloGpuRenderer::new_offscreen(w, h, atlas) {
            Some(mut r) => results.push(run(&mut r, &mut grid, frames)),
            None => println!("vello_gpu: no GPU adapter"),
        }
    }
    let mut raster = termoil_render::RasterRenderer::new(w, h, termoil_render::DEFAULT_GLYPH_CAPACITY);
    results.push(run(&mut raster, &mut grid, frames));
    for (threads, atlas) in [(0, true), (4, true)] {
        let mut r = VelloCpuRenderer::new(w, h, threads, atlas);
        let mut res = run(&mut r, &mut grid, frames.min(300));
        res.name = format!("{} with {threads} extra threads", res.name);
        results.push(res);
    }
    for r in &results {
        println!(
            "{:<70} p50 {:>6.2} ms  p90 {:>6.2} ms  p99 {:>6.2} ms  max {:>6.2} ms  build {:>5.2} ms  encode p50 {:>5.2} p99 {:>5.2} ms  {} glyphs",
            r.name,
            ms(r.percentile(0.5)),
            ms(r.percentile(0.9)),
            ms(r.percentile(0.99)),
            ms(r.times[r.times.len() - 1]),
            ms(r.build),
            ms(r.encode[r.encode.len() / 2]),
            ms(r.encode[r.encode.len() * 99 / 100]),
            r.glyphs
        );
    }
    results
}

/// Milliseconds as a float.
fn ms(d: Duration) -> f64 {
    d.as_secs_f64() * 1000.0
}

/// `cpu-mem <threads> <atlas 0|1> <cols> <rows>`: the CPU renderer's memory and frame time at a
/// grid size, with a full screen scroll every frame (task-2481). Prints private MB before the
/// renderer exists, once it exists, and after the frames, with the frame times.
pub fn cpu_memory(threads: u16, atlas: bool, cols: u16, rows: u16, raster: bool) {
    let mut grid = gate_grid();
    let m = *grid.metrics();
    let (w, h) = (u32::from(cols) * m.width + 4, u32::from(rows) * m.height + 4);
    let before = crate::memory::process_memory().0;
    let mut r: Box<dyn Renderer> = if raster {
        Box::new(termoil_render::RasterRenderer::new(w, h, termoil_render::DEFAULT_GLYPH_CAPACITY))
    } else {
        Box::new(VelloCpuRenderer::new(w, h, threads, atlas))
    };
    let made = crate::memory::process_memory().0;
    let mut term = Terminal::new(cols, rows, TerminalConfig::default());
    let mut state = RenderState::new(cols, rows, Colors::dark_default());
    let (opts, mut builder) = (BuildOptions::default(), FrameBuilder::new());
    let mut times = Vec::new();
    for n in 0..300 {
        term.advance(&line(n));
        let start = Instant::now();
        state.update(&mut term, 0);
        let frame = builder.build(&state, &mut grid, &opts);
        r.render(&frame, &grid).expect("render");
        times.push(start.elapsed());
    }
    times.sort();
    let after = crate::memory::process_memory().0;
    println!(
        "{} threads {threads} atlas {atlas} {w}x{h}: private before {:.1} MB, renderer made {:.1} MB, after frames {:.1} MB; frame p50 {:.2} ms p99 {:.2} ms",
        r.name(),
        before as f64 / 1e6,
        made as f64 / 1e6,
        after as f64 / 1e6,
        ms(times[150]),
        ms(times[296])
    );
}

/// The neon-glow shader with the pulse cursor effect, composed the way `termoil-app`'s
/// `cursor_effects::compose` does it, so the bench measures the pass a person sees (task-2630).
fn neon_glow_with_pulse() -> String {
    let base = include_str!("../../../crates/termoil-app/shaders/neon-glow.wgsl")
        .replace("fn termoil_shade(", "fn termoil_base_shade(");
    let common = include_str!("../../../crates/termoil-app/cursor-effects/common.wgsl");
    let pulse = include_str!("../../../crates/termoil-app/cursor-effects/pulse.wgsl");
    format!(
        "{base}\nconst TERMOIL_SHADER_CURSOR: f32 = 0.0;\n{common}\n{pulse}\n\
         fn termoil_shade(uv: vec2<f32>, color: vec4<f32>) -> vec4<f32> {{\n    \
         return termoil_cursor_effect(uv, termoil_base_shade(uv, color));\n}}\n"
    )
}

/// Times `frames` frames on `renderer`. With `scroll` every frame appends a line (a person
/// scrolling or output streaming); without it every frame shows the same text, which is what an
/// animated shader draws at the display rate while the terminal is idle. Returns sorted times.
fn shader_frames(
    renderer: &mut VelloGpuRenderer, grid: &mut FontGrid, size: (u16, u16), scroll: bool, frames: usize,
) -> Vec<Duration> {
    let mut term = Terminal::new(size.0, size.1, TerminalConfig::default());
    for n in 0..usize::from(size.1) {
        term.advance(&line(n));
    }
    let mut state = RenderState::new(size.0, size.1, Colors::dark_default());
    let (opts, mut builder) = (BuildOptions::default(), FrameBuilder::new());
    let mut times = Vec::with_capacity(frames);
    for n in 0..frames + 10 {
        if scroll {
            term.advance(&line(n + 1000));
        }
        let start = Instant::now();
        state.update(&mut term, 0);
        let frame = builder.build(&state, grid, &opts);
        renderer.render(&frame, grid).expect("render");
        if n >= 10 {
            times.push(start.elapsed());
        }
    }
    times.sort();
    times
}

/// `shader [frames]` (task-2630): frame times on a Retina sized window (a 3024 by 1964 pixel
/// display, font at twice its size) with the neon-glow shader and the pulse cursor effect, for a
/// frame where the text scrolled and a frame where nothing changed, against the same frames with
/// no shader. Offscreen frames wait for the GPU, so each time is the whole frame.
pub fn shader_measure(frames: usize) {
    let mut grid = FontGrid::new(FontConfig { size_px: FontConfig::default().size_px * 2.0, ..FontConfig::default() })
        .expect("default font");
    let m = *grid.metrics();
    let (w, h) = (3024u32, 1964u32);
    let size = (((w - 4) / m.width) as u16, ((h - 4) / m.height) as u16);
    println!("shader bench: {w}x{h} px, {}x{} cells of {}x{} px, {frames} frames", size.0, size.1, m.width, m.height);
    let source = neon_glow_with_pulse();
    for shader in [false, true] {
        for scroll in [true, false] {
            let Some(mut r) = VelloGpuRenderer::new_offscreen(w, h, true) else {
                println!("vello_gpu: no GPU adapter");
                return;
            };
            if shader {
                r.set_shader(Some(&source)).expect("neon-glow with pulse compiles");
            }
            let t = shader_frames(&mut r, &mut grid, size, scroll, frames);
            println!(
                "{:<26} {:<16} p50 {:>6.2} ms  p90 {:>6.2} ms  p99 {:>6.2} ms  max {:>6.2} ms",
                if shader { "neon-glow + pulse" } else { "no shader" },
                if scroll { "scrolling" } else { "nothing changed" },
                ms(t[t.len() / 2]),
                ms(t[t.len() * 9 / 10]),
                ms(t[t.len() * 99 / 100]),
                ms(t[t.len() - 1])
            );
        }
    }
}
