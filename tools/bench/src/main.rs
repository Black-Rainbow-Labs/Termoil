//! Terminus benchmarks: P1 parser throughput and P5 allocation counting
//! (`docs/TEST-SUITE.md` section 13).
//!
//! `terminus-bench throughput` feeds generated streams through `terminus-vt` alone on one thread
//! and prints MB/s per scenario; the gate scenario is `mixed`, a 1 GB stream of build log style
//! text with colours, progress bars and some UTF-8. `terminus-bench alloc` counts heap
//! allocations while the same stream is applied, split into the warm up (until the scrollback
//! reaches its limit) and the steady state. `terminus-bench compare` runs the same bytes through
//! vte, alacritty_terminal and terminus-vt and prints one table in MB/s; `terminus-bench
//! write-stream mixed <path>` writes the 1 GiB gate file for `tools/bench-ghostty.ps1`. Every run
//! appends JSON rows to `_agent_output/bench/` with the machine, build and commit.

mod compare;
mod gpuinit;
mod memory;
mod render;
mod streams;

use std::alloc::{GlobalAlloc, Layout, System};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Instant;

use terminus_vt::{Terminal, TerminalConfig};

/// A global allocator that counts calls, for P5.
struct Counting;

static ALLOCS: AtomicU64 = AtomicU64::new(0);

// SAFETY: forwards to the system allocator unchanged, only counting calls.
unsafe impl GlobalAlloc for Counting {
    /// Counts and allocates.
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        ALLOCS.fetch_add(1, Ordering::Relaxed);
        // SAFETY: the caller upholds GlobalAlloc's contract, which System requires.
        unsafe { System.alloc(layout) }
    }

    /// Frees.
    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        // SAFETY: as above.
        unsafe { System.dealloc(ptr, layout) }
    }

    /// Counts and reallocates.
    unsafe fn realloc(&self, ptr: *mut u8, layout: Layout, new_size: usize) -> *mut u8 {
        ALLOCS.fetch_add(1, Ordering::Relaxed);
        // SAFETY: as above.
        unsafe { System.realloc(ptr, layout, new_size) }
    }
}

#[global_allocator]
static GLOBAL: Counting = Counting;

/// Bytes the parser thread applies per terminal lock in the app; the bench feeds the same slices.
const SLICE: usize = 64 * 1024;

/// The P1 gate from `docs/TEST-SUITE.md` section 13 (restated in task-2459): the `mixed` stream
/// through `Terminal` on one core.
const P1_GATE_MB_PER_S: f64 = 250.0;

/// One measured result.
struct Sample {
    scenario: String,
    bytes: u64,
    seconds: f64,
}

/// Feeds `data` `repeat` times through a fresh 200 by 60 terminal; returns elapsed seconds.
fn run(data: &[u8], repeat: usize) -> f64 {
    let mut term = Terminal::new(200, 60, bench_config());
    let start = Instant::now();
    for _ in 0..repeat {
        for chunk in data.chunks(SLICE) {
            term.advance(chunk);
            let _ = term.take_replies();
            let _ = term.take_events();
        }
    }
    let secs = start.elapsed().as_secs_f64();
    std::hint::black_box(&term);
    secs
}

/// The terminal settings for a run: the defaults, with the scrollback budget overridable through
/// `TERMINUS_BENCH_SCROLLBACK` (bytes) to see how much of the cost is the cold scrollback pages.
fn bench_config() -> TerminalConfig {
    let mut c = TerminalConfig::default();
    if let Some(b) = std::env::var("TERMINUS_BENCH_SCROLLBACK").ok().and_then(|v| v.parse().ok()) {
        c.scrollback.bytes = b;
    }
    c
}

/// Measures every scenario; `mixed` is fed to 1 GB, the others to 256 MB.
fn throughput(samples: usize, only: Option<&str>) -> Vec<Sample> {
    let mut out = Vec::new();
    for (name, data) in streams::scenarios() {
        if only.is_some_and(|o| o != name) {
            continue;
        }
        let repeat = streams::repeat_for(data.len(), streams::target_bytes(name));
        let mut times = Vec::new();
        for _ in 0..samples {
            times.push(run(&data, repeat));
        }
        times.sort_by(f64::total_cmp);
        let median = times[times.len() / 2];
        let bytes = (data.len() * repeat) as u64;
        println!(
            "{name:<22} {:>9.1} MB/s  ({:.3} s for {} MB, median of {samples})",
            bytes as f64 / median / 1e6,
            median,
            bytes >> 20
        );
        out.push(Sample { scenario: name.to_string(), bytes, seconds: median });
    }
    out
}

/// Counts allocations while 1 GB of the mixed stream is applied.
fn allocations() -> (u64, u64, u64) {
    let data = streams::mixed(64 << 20);
    let mut term = Terminal::new(200, 60, TerminalConfig::default());
    let mut warm = 0;
    let mut steady_bytes = 0u64;
    let before = ALLOCS.load(Ordering::Relaxed);
    let mut steady_start = None;
    for _ in 0..16 {
        for chunk in data.chunks(SLICE) {
            term.advance(chunk);
            if steady_start.is_none() && term.primary().pages.pruned_pages() > 0 {
                steady_start = Some(ALLOCS.load(Ordering::Relaxed));
                warm = ALLOCS.load(Ordering::Relaxed) - before;
            }
            if steady_start.is_some() {
                steady_bytes += chunk.len() as u64;
            }
        }
    }
    let steady = ALLOCS.load(Ordering::Relaxed) - steady_start.unwrap_or(before);
    println!("warm up allocations: {warm}");
    println!(
        "steady state: {steady} allocations over {} MB ({:.6} per byte)",
        steady_bytes >> 20,
        steady as f64 / steady_bytes.max(1) as f64
    );
    (warm, steady, steady_bytes)
}

/// The commit being measured.
fn commit() -> String {
    std::process::Command::new("git")
        .args(["rev-parse", "--short", "HEAD"])
        .output()
        .ok()
        .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
        .unwrap_or_default()
}

/// The folder benchmark rows are written to: `_agent_output/bench/` at the repository root.
fn bench_dir() -> std::path::PathBuf {
    std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../_agent_output/bench")
}

/// Appends JSON rows to `_agent_output/bench/<file>`.
fn record(file: &str, rows: &[serde_json::Value]) {
    let dir = bench_dir();
    let _ = std::fs::create_dir_all(&dir);
    let mut text = String::new();
    for r in rows {
        text.push_str(&r.to_string());
        text.push('\n');
    }
    use std::io::Write;
    if let Ok(mut f) = std::fs::OpenOptions::new().create(true).append(true).open(dir.join(file)) {
        let _ = f.write_all(text.as_bytes());
    }
}

/// The machine description every row carries.
fn machine() -> serde_json::Value {
    serde_json::json!({
        "cpu": std::env::var("PROCESSOR_IDENTIFIER").unwrap_or_default(),
        "host": std::env::var("COMPUTERNAME").or_else(|_| std::env::var("HOSTNAME")).unwrap_or_default(),
        "os": std::env::consts::OS,
        "profile": if cfg!(debug_assertions) { "debug" } else { "release" },
        "commit": commit(),
        "version": env!("CARGO_PKG_VERSION"),
    })
}

/// Parses the streams with a handler that does nothing, to separate parsing from applying.
fn parse_only() {
    for (name, data) in streams::scenarios() {
        let mut parser = terminus_vt::parser::Parser::default();
        let mut count = 0u64;
        let repeat = ((256u64 << 20) / data.len() as u64).max(1);
        let start = Instant::now();
        for _ in 0..repeat {
            for chunk in data.chunks(SLICE) {
                parser.advance(chunk, &mut |_a: terminus_vt::parser::Action<'_>| count += 1);
            }
        }
        let secs = start.elapsed().as_secs_f64();
        let bytes = data.len() as u64 * repeat;
        println!(
            "parse {name:<22} {:>9.1} MB/s  {:.1} bytes per action",
            bytes as f64 / secs / 1e6,
            bytes as f64 / count.max(1) as f64
        );
    }
}

/// The P1 gate line both `throughput` and `compare` print, from the `mixed` rate in MB/s.
fn p1_gate_line(mixed: f64) -> String {
    let gate = P1_GATE_MB_PER_S;
    let verdict = if mixed >= gate { "PASS" } else { "FAIL" };
    format!("P1 gate (mixed >= {gate} MB/s, Terminal 200x60, default scrollback): {mixed:.0} MB/s {verdict}")
}

/// `throughput [samples] [scenario]`: terminus-vt full on every scenario, rows to p1.jsonl, the gate line.
fn throughput_command(args: &[String]) {
    let samples = args.get(2).and_then(|s| s.parse().ok()).unwrap_or(3);
    let results = throughput(samples, args.get(3).map(String::as_str));
    let rows: Vec<serde_json::Value> = results.iter().map(|s| serde_json::json!({"machine": machine(), "source": "throughput", "scenario": s.scenario, "bytes": s.bytes, "seconds": s.seconds, "mb_per_s": s.bytes as f64 / s.seconds / 1e6})).collect();
    record("p1.jsonl", &rows);
    if let Some(m) = results.iter().find(|s| s.scenario == "mixed") {
        println!("{}", p1_gate_line(m.bytes as f64 / m.seconds / 1e6));
    }
}

/// `compare [samples] [scenario|all]`: the comparison table, p1.jsonl rows carrying the
/// comparison numbers, and the gate line from the terminus-vt full `mixed` row.
fn compare_command(args: &[String]) {
    let samples = args.get(2).and_then(|s| s.parse().ok()).unwrap_or(3);
    let ghostty = compare::latest_ghostty(&bench_dir());
    println!("{}", composition_line("mixed", &streams::mixed(64 << 20)));
    let rows = compare::run(samples, args.get(3).map(String::as_str), ghostty);
    let json: Vec<serde_json::Value> = rows.iter().map(|r| serde_json::json!({"machine": machine(), "source": "compare", "scenario": r.stream, "bytes": r.bytes, "seconds": r.tvt_full_seconds, "mb_per_s": r.tvt_full, "comparison": compare::json(r)})).collect();
    record("p1.jsonl", &json);
    if let Some(m) = rows.iter().find(|r| r.stream == "mixed") {
        println!("{}", p1_gate_line(m.tvt_full));
    }
}

/// One line describing a block: its length, checksum and composition, to check against the pin in
/// `streams::tests` and the table in `docs/decisions/throughput.md`.
fn composition_line(name: &str, block: &[u8]) -> String {
    let c = streams::composition(block);
    format!(
        "{name} block: {} bytes, fnv1a {:#018x}, {} lines, {:.1} bytes per line, {:.1} escape sequences per KB, {:.1} UTF-8 multibyte per KB, {:.1} % of bytes inside escape sequences",
        c.bytes,
        streams::fnv1a(block),
        c.lines,
        c.bytes_per_line,
        c.escapes_per_kb,
        c.utf8_multibyte_per_kb,
        c.fraction_in_escape * 100.0
    )
}

/// `write-stream <scenario> <path>`: writes the scenario's block repeated to its target size, so
/// `write-stream mixed` writes the 1 GiB P1 gate file (sixteen copies of `mixed(64 << 20)`).
fn write_stream_command(args: &[String]) {
    use std::io::Write;
    let (Some(name), Some(path)) = (args.get(2), args.get(3)) else {
        eprintln!("usage: terminus-bench write-stream <scenario> <path>");
        std::process::exit(2);
    };
    let Some((_, block)) = streams::scenarios().into_iter().find(|(n, _)| n == name) else {
        eprintln!("unknown scenario {name}");
        std::process::exit(2);
    };
    let repeat = streams::repeat_for(block.len(), streams::target_bytes(name));
    let mut f = std::io::BufWriter::new(std::fs::File::create(path).expect("create the stream file"));
    for _ in 0..repeat {
        f.write_all(&block).expect("write the stream file");
    }
    f.flush().expect("flush the stream file");
    println!("{}", composition_line(name, &block));
    println!("wrote {} bytes ({repeat} copies of the block) to {path}", block.len() * repeat);
}

/// Entry point: `throughput [samples] [scenario]`, `compare [samples] [scenario|all]`,
/// `write-stream <scenario> <path>`, `parse`, `alloc`, `memory`, `gpu-init`, or `render [frames]`.
fn main() {
    let args: Vec<String> = std::env::args().collect();
    match args.get(1).map(String::as_str) {
        Some("parse") => parse_only(),
        Some("compare") => compare_command(&args),
        Some("write-stream") => write_stream_command(&args),
        Some("memory") => {
            let rows: Vec<serde_json::Value> = memory::measure()
                .into_iter()
                .map(|mut r| {
                    r["machine"] = machine();
                    r
                })
                .collect();
            record("p3.jsonl", &rows);
        }
        Some("memory-one") => memory::run_one(
            args[2].parse().unwrap_or(100_000),
            args.get(3).is_some_and(|k| k == "dense"),
            args.get(4).and_then(|t| t.parse().ok()).unwrap_or(1),
        ),
        Some("gpu-init") => gpuinit::run(args.get(2).map(String::as_str).unwrap_or("every")),
        Some("render") => {
            let frames = args.get(2).and_then(|s| s.parse().ok()).unwrap_or(1200);
            let results = render::measure(frames);
            let rows: Vec<serde_json::Value> = results.iter().map(|r| serde_json::json!({"machine": machine(), "renderer": r.name, "frames": r.times.len(), "p50_ms": r.percentile(0.5).as_secs_f64() * 1e3, "p90_ms": r.percentile(0.9).as_secs_f64() * 1e3, "p99_ms": r.percentile(0.99).as_secs_f64() * 1e3, "build_ms": r.build.as_secs_f64() * 1e3, "glyphs": r.glyphs})).collect();
            record("r4.jsonl", &rows);
            let gpu = results.iter().filter(|r| r.name.starts_with("vello_gpu")).map(|r| r.percentile(0.99)).min();
            match gpu {
                Some(p99) => println!(
                    "R4 gate (vello_gpu p99 < 8 ms): {:.2} ms {}",
                    p99.as_secs_f64() * 1e3,
                    if p99.as_secs_f64() < 0.008 { "PASS" } else { "FAIL" }
                ),
                None => println!("R4 gate: no GPU"),
            }
        }
        Some("alloc") => {
            let (warm, steady, bytes) = allocations();
            record(
                "p5.jsonl",
                &[
                    serde_json::json!({"machine": machine(), "warm_allocs": warm, "steady_allocs": steady, "steady_bytes": bytes}),
                ],
            );
            if steady > bytes / (1 << 20) {
                eprintln!("P5 FAIL: more than one allocation per MB in the steady state");
                std::process::exit(1);
            }
        }
        _ => throughput_command(&args),
    }
}
