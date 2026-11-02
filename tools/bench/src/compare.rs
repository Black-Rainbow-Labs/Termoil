//! `termoil-bench compare`: the P1 comparison from `docs/decisions/throughput.md`, repeated with
//! every release (`docs/TEST-SUITE.md` section 13).
//!
//! The identical bytes go through vte 0.15 (parser only, counting `Perform` calls),
//! alacritty_terminal 0.26 (a `Term` at 200 by 60 with 100,000 lines of history, through
//! `vte::ansi::Processor`), and termoil-vt parser only and full (`Terminal` at 200 by 60 with the
//! default scrollback). Ghostty 1.4 is measured by `tools/bench-ghostty.ps1` in WSL, which appends
//! a row to `_agent_output/bench/ghostty.jsonl`; the latest row is shown beside `mixed`.
//!
//! This harness was copied from task-2459's comparison (`_agent_output/task-2459-throughput/harness`).
//! vte and alacritty_terminal are dependencies of tools/bench only, never of termoil-vt, which CI
//! builds for wasm.

use std::time::Instant;

use crate::streams;

/// Bytes per `advance` call, the same slices `throughput` and the app use.
const SLICE: usize = 64 * 1024;

/// The terminal size every full run uses.
const COLS: usize = 200;
const ROWS: usize = 60;

/// alacritty_terminal's history is in lines; 100,000 lines of `mixed` is about 9.5 MB.
const ALACRITTY_HISTORY: usize = 100_000;

/// One stream's results in MB/s (bytes / seconds / 1e6, median of the samples).
pub struct Row {
    pub stream: String,
    pub bytes: u64,
    pub vte_parser: f64,
    pub alacritty_full: f64,
    pub tvt_parser: f64,
    pub tvt_full: f64,
    pub tvt_full_seconds: f64,
    pub ghostty_full: Option<f64>,
}

/// Counts parser callbacks for the vte parser only run.
#[derive(Default)]
struct Counter {
    n: u64,
}

impl vte::Perform for Counter {
    /// Counts a printed character.
    fn print(&mut self, _c: char) {
        self.n += 1;
    }
    /// Counts a C0 or C1 control.
    fn execute(&mut self, _b: u8) {
        self.n += 1;
    }
    /// Counts a DCS start.
    fn hook(&mut self, _p: &vte::Params, _i: &[u8], _ig: bool, _a: char) {
        self.n += 1;
    }
    /// Counts a DCS data byte.
    fn put(&mut self, _b: u8) {
        self.n += 1;
    }
    /// Counts a DCS end.
    fn unhook(&mut self) {
        self.n += 1;
    }
    /// Counts an OSC.
    fn osc_dispatch(&mut self, _p: &[&[u8]], _b: bool) {
        self.n += 1;
    }
    /// Counts a CSI.
    fn csi_dispatch(&mut self, _p: &vte::Params, _i: &[u8], _ig: bool, _a: char) {
        self.n += 1;
    }
    /// Counts an ESC sequence.
    fn esc_dispatch(&mut self, _i: &[u8], _ig: bool, _b: u8) {
        self.n += 1;
    }
}

/// The grid size alacritty_terminal is created with.
struct Dims;

impl alacritty_terminal::grid::Dimensions for Dims {
    /// Lines on the screen; history is added by the config.
    fn total_lines(&self) -> usize {
        ROWS
    }
    /// Lines on the screen.
    fn screen_lines(&self) -> usize {
        ROWS
    }
    /// Columns.
    fn columns(&self) -> usize {
        COLS
    }
}

/// Feeds `data` `repeat` times through the vte parser with a counting performer; returns seconds.
fn run_vte(data: &[u8], repeat: usize) -> f64 {
    let mut p = vte::Parser::new();
    let mut c = Counter::default();
    let start = Instant::now();
    for _ in 0..repeat {
        for chunk in data.chunks(SLICE) {
            p.advance(&mut c, chunk);
        }
    }
    let s = start.elapsed().as_secs_f64();
    std::hint::black_box(c.n);
    s
}

/// Feeds `data` `repeat` times through alacritty_terminal's `Term` and ansi `Processor`; returns seconds.
fn run_alacritty(data: &[u8], repeat: usize) -> f64 {
    use alacritty_terminal::event::VoidListener;
    use alacritty_terminal::term::{Config, Term};
    use alacritty_terminal::vte::ansi::Processor;
    let config = Config { scrolling_history: ALACRITTY_HISTORY, ..Config::default() };
    let mut term = Term::new(config, &Dims, VoidListener);
    let mut proc: Processor = Processor::new();
    let start = Instant::now();
    for _ in 0..repeat {
        for chunk in data.chunks(SLICE) {
            proc.advance(&mut term, chunk);
        }
    }
    let s = start.elapsed().as_secs_f64();
    std::hint::black_box(&term);
    s
}

/// Feeds `data` `repeat` times through the termoil-vt parser with a counting handler; returns seconds.
fn run_tvt_parser(data: &[u8], repeat: usize) -> f64 {
    let mut parser = termoil_vt::parser::Parser::default();
    let mut count = 0u64;
    let start = Instant::now();
    for _ in 0..repeat {
        for chunk in data.chunks(SLICE) {
            parser.advance(chunk, &mut |_a: termoil_vt::parser::Action<'_>| count += 1);
        }
    }
    let s = start.elapsed().as_secs_f64();
    std::hint::black_box(count);
    s
}

/// Feeds `data` `repeat` times through a termoil-vt `Terminal` with the default configuration; returns seconds.
fn run_tvt_full(data: &[u8], repeat: usize) -> f64 {
    let mut term = termoil_vt::Terminal::new(COLS as u16, ROWS as u16, termoil_vt::TerminalConfig::default());
    let start = Instant::now();
    for _ in 0..repeat {
        for chunk in data.chunks(SLICE) {
            term.advance(chunk);
            let _ = term.take_replies();
            let _ = term.take_events();
        }
    }
    let s = start.elapsed().as_secs_f64();
    std::hint::black_box(&term);
    s
}

/// Runs `f` `samples` times and returns the median seconds.
fn median(samples: usize, f: impl Fn() -> f64) -> f64 {
    let mut times: Vec<f64> = (0..samples.max(1)).map(|_| f()).collect();
    times.sort_by(f64::total_cmp);
    times[times.len() / 2]
}

/// Measures one stream through every engine.
fn measure(name: &str, data: &[u8], samples: usize, ghostty: Option<f64>) -> Row {
    let repeat = streams::repeat_for(data.len(), streams::target_bytes(name));
    let bytes = (data.len() * repeat) as u64;
    let rate = |s: f64| bytes as f64 / s / 1e6;
    let tvt_full_seconds = median(samples, || run_tvt_full(data, repeat));
    Row {
        stream: name.to_string(),
        bytes,
        vte_parser: rate(median(samples, || run_vte(data, repeat))),
        alacritty_full: rate(median(samples, || run_alacritty(data, repeat))),
        tvt_parser: rate(median(samples, || run_tvt_parser(data, repeat))),
        tvt_full: rate(tvt_full_seconds),
        tvt_full_seconds,
        ghostty_full: if name == "mixed" { ghostty } else { None },
    }
}

/// Which streams `compare` runs: every scenario except the `micro_` ones, or only `only`, or
/// every scenario when `only` is `all`.
fn selected(name: &str, only: Option<&str>) -> bool {
    match only {
        None => !name.starts_with("micro_"),
        Some("all") => true,
        Some(o) => o == name,
    }
}

/// Measures the selected streams, printing each row as it finishes, then the whole table.
pub fn run(samples: usize, only: Option<&str>, ghostty: Option<f64>) -> Vec<Row> {
    let mut rows = Vec::new();
    for (name, data) in streams::scenarios() {
        if !selected(name, only) {
            continue;
        }
        let row = measure(name, &data, samples, ghostty);
        eprintln!("{}", line(&row));
        rows.push(row);
    }
    println!("{}", table(&rows, samples));
    rows
}

/// Formats an MB/s figure, or blank when not measured.
fn cell(v: Option<f64>) -> String {
    v.map(|v| format!("{v:.0}")).unwrap_or_default()
}

/// One table line for a row.
fn line(r: &Row) -> String {
    format!(
        "| {} ({} MiB) | {:.0} | {:.0} | {} | {:.0} | {:.0} |",
        r.stream,
        r.bytes >> 20,
        r.vte_parser,
        r.alacritty_full,
        cell(r.ghostty_full),
        r.tvt_parser,
        r.tvt_full
    )
}

/// The comparison as one Markdown table in MB/s.
pub fn table(rows: &[Row], samples: usize) -> String {
    let mut t = format!(
        "MB/s, bytes / seconds / 1e6, median of {samples}. Full engines at {COLS} by {ROWS}.\n\n\
         | stream | vte 0.15 parser only | alacritty_terminal 0.26 full, {ALACRITTY_HISTORY} lines | Ghostty 1.4 full | termoil-vt parser only | termoil-vt full, default scrollback |\n\
         |---|---|---|---|---|---|\n"
    );
    for r in rows {
        t.push_str(&line(r));
        t.push('\n');
    }
    t
}

/// The comparison numbers as the JSON object a p1.jsonl row carries.
pub fn json(r: &Row) -> serde_json::Value {
    serde_json::json!({
        "vte_0_15_parser": r.vte_parser,
        "alacritty_terminal_0_26_full_100k_lines": r.alacritty_full,
        "ghostty_1_4_full": r.ghostty_full,
        "termoil_vt_parser": r.tvt_parser,
        "termoil_vt_full": r.tvt_full,
    })
}

/// The newest Ghostty `mixed` figure in `ghostty.jsonl` under `dir`, written by `tools/bench-ghostty.ps1`.
pub fn latest_ghostty(dir: &std::path::Path) -> Option<f64> {
    let text = std::fs::read_to_string(dir.join("ghostty.jsonl")).ok()?;
    text.lines()
        .rev()
        .filter_map(|l| serde_json::from_str::<serde_json::Value>(l).ok())
        .find(|v| v["stream"] == "mixed")
        .and_then(|v| v["mb_per_s"].as_f64())
}
