//! Terminus benchmarks: P1 parser throughput and P5 allocation counting
//! (`docs/TEST-SUITE.md` section 13).
//!
//! `terminus-bench throughput` feeds generated streams through `terminus-vt` alone on one thread
//! and prints MB/s per scenario; the gate scenario is `mixed`, a 1 GB stream of build log style
//! text with colours, progress bars and some UTF-8. `terminus-bench alloc` counts heap
//! allocations while the same stream is applied, split into the warm up (until the scrollback
//! reaches its limit) and the steady state. Every run appends JSON rows to
//! `_agent_output/bench/` with the machine, build and commit.

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
        let target: u64 = if name == "mixed" { 1 << 30 } else { 256 << 20 };
        let repeat = (target / data.len() as u64).max(1) as usize;
        let mut times = Vec::new();
        for _ in 0..samples {
            times.push(run(&data, repeat));
        }
        times.sort_by(f64::total_cmp);
        let median = times[times.len() / 2];
        let bytes = (data.len() * repeat) as u64;
        println!("{name:<22} {:>9.1} MB/s  ({:.3} s for {} MB, median of {samples})", bytes as f64 / median / 1e6, median, bytes >> 20);
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
    println!("steady state: {steady} allocations over {} MB ({:.6} per byte)", steady_bytes >> 20, steady as f64 / steady_bytes.max(1) as f64);
    (warm, steady, steady_bytes)
}

/// The commit being measured.
fn commit() -> String {
    std::process::Command::new("git").args(["rev-parse", "--short", "HEAD"]).output().ok().map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string()).unwrap_or_default()
}

/// Appends JSON rows to `_agent_output/bench/<file>`.
fn record(file: &str, rows: &[serde_json::Value]) {
    let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../_agent_output/bench");
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
        println!("parse {name:<22} {:>9.1} MB/s  {:.1} bytes per action", bytes as f64 / secs / 1e6, bytes as f64 / count.max(1) as f64);
    }
}

/// Entry point: `throughput [samples]`, `parse`, or `alloc`.
fn main() {
    let args: Vec<String> = std::env::args().collect();
    match args.get(1).map(String::as_str) {
        Some("parse") => parse_only(),
        Some("alloc") => {
            let (warm, steady, bytes) = allocations();
            record("p5.jsonl", &[serde_json::json!({"machine": machine(), "warm_allocs": warm, "steady_allocs": steady, "steady_bytes": bytes})]);
            if steady > bytes / (1 << 20) {
                eprintln!("P5 FAIL: more than one allocation per MB in the steady state");
                std::process::exit(1);
            }
        }
        _ => {
            let samples = args.get(2).and_then(|s| s.parse().ok()).unwrap_or(3);
            let results = throughput(samples, args.get(3).map(String::as_str));
            let rows: Vec<serde_json::Value> = results.iter().map(|s| serde_json::json!({"machine": machine(), "scenario": s.scenario, "bytes": s.bytes, "seconds": s.seconds, "mb_per_s": s.bytes as f64 / s.seconds / 1e6})).collect();
            record("p1.jsonl", &rows);
            let mixed = results.iter().find(|s| s.scenario == "mixed").map(|s| s.bytes as f64 / s.seconds / 1e9).unwrap_or(0.0);
            println!("P1 gate (mixed >= 1.0 GB/s): {mixed:.2} GB/s {}", if mixed >= 1.0 { "PASS" } else { "FAIL" });
        }
    }
}
