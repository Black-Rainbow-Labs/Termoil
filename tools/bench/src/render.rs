//! R4, the M2 gate (docs/TEST-SUITE.md section 10, docs/TDD.md section 4.7): a 200 by 60 grid,
//! a full screen scroll of mixed coloured text every frame, and the time from taking the render
//! state to the finished frame. The gate is p99 under 8 ms with `vello_gpu` on this machine.
//!
//! Each frame appends one line, which scrolls every row, so every frame rebuilds and redraws the
//! whole grid: the worst case for a renderer that redraws per frame.

use std::time::{Duration, Instant};

use terminus_font::{FontConfig, FontGrid};
use terminus_render::{BuildOptions, Colors, FrameBuilder, RenderState, Renderer, VelloCpuRenderer, VelloGpuRenderer};
use terminus_vt::{Terminal, TerminalConfig};

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
        "terminus",
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
        grid.face(grid.primary(terminus_font::FontStyle::Regular)).family,
        grid.size_px()
    );
    let mut results = Vec::new();
    for atlas in [false, true] {
        match VelloGpuRenderer::new_offscreen(w, h, atlas) {
            Some(mut r) => results.push(run(&mut r, &mut grid, frames)),
            None => println!("vello_gpu: no GPU adapter"),
        }
    }
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
