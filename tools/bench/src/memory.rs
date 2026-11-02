//! P3, memory (docs/TEST-SUITE.md section 13): 100 000 and 1 000 000 lines of 200 columns, typical
//! text and dense attribute text, measured after the idle compression; and ten idle tabs. Gate:
//! under 150 MB and under 400 MB. Each case runs in a child process of its own so one case's
//! allocations do not count against the next.

use termoil_vt::pagelist::ScrollbackLimit;
use termoil_vt::{Terminal, TerminalConfig};

/// The cases: name, lines, dense attributes, terminals.
const CASES: [(&str, usize, bool, usize); 5] = [
    ("100k_typical", 100_000, false, 1),
    ("100k_dense", 100_000, true, 1),
    ("1m_typical", 1_000_000, false, 1),
    ("1m_dense", 1_000_000, true, 1),
    ("ten_idle_tabs", 2_000, false, 10),
];

/// The gate for a case in MB: 400 for a million lines, 150 otherwise.
fn gate(lines: usize) -> f64 {
    if lines >= 1_000_000 { 400.0 } else { 150.0 }
}

/// One line of 200 columns: words, or with `dense` an SGR change on every word.
fn line(i: usize, dense: bool) -> Vec<u8> {
    let words =
        ["compile", "termoil", "warning:", "src/main.rs", "ok", "finished", "error[E0382]", "->", "release", "target"];
    let mut out = Vec::with_capacity(320);
    let mut col = 0;
    let mut k = i.wrapping_mul(2_654_435_761);
    while col < 192 {
        k = k.wrapping_mul(6_364_136_223_846_793_005).wrapping_add(1);
        let w = words[(k >> 33) % words.len()];
        if dense {
            out.extend_from_slice(format!("\x1b[{};{}m", 30 + (k >> 40) % 8, 40 + (k >> 44) % 8).as_bytes());
        }
        out.extend_from_slice(w.as_bytes());
        out.push(b' ');
        col += w.len() + 1;
    }
    if dense {
        out.extend_from_slice(b"\x1b[m");
    }
    out.extend_from_slice(b"\r\n");
    out
}

/// Runs one case in this process and prints `mb private, mb working set`.
pub fn run_one(lines: usize, dense: bool, tabs: usize) {
    let config = TerminalConfig {
        scrollback: ScrollbackLimit { bytes: usize::MAX / 2, lines: None },
        ..TerminalConfig::default()
    };
    let mut terms: Vec<Terminal> = (0..tabs).map(|_| Terminal::new(200, 60, config.clone())).collect();
    for t in &mut terms {
        let mut buf = Vec::with_capacity(1 << 20);
        for i in 0..lines {
            buf.extend_from_slice(&line(i, dense));
            if buf.len() > (1 << 20) - 400 {
                t.advance(&buf);
                buf.clear();
            }
        }
        t.advance(&buf);
        t.primary_mut().pages.compress_cold(4);
    }
    let (private, working) = process_memory();
    let history: usize = terms.iter().map(|t| t.primary().pages.history_rows()).sum();
    println!("{:.1} {:.1} {history}", private as f64 / 1e6, working as f64 / 1e6);
}

/// This process's private bytes and working set.
#[cfg(windows)]
fn process_memory() -> (u64, u64) {
    use windows::Win32::System::ProcessStatus::{
        GetProcessMemoryInfo, PROCESS_MEMORY_COUNTERS, PROCESS_MEMORY_COUNTERS_EX,
    };
    use windows::Win32::System::Threading::GetCurrentProcess;
    let mut c = PROCESS_MEMORY_COUNTERS_EX::default();
    // SAFETY: the counters struct is the size passed and lives for the call.
    let ok = unsafe {
        GetProcessMemoryInfo(
            GetCurrentProcess(),
            (&raw mut c).cast::<PROCESS_MEMORY_COUNTERS>(),
            size_of::<PROCESS_MEMORY_COUNTERS_EX>() as u32,
        )
    };
    if ok.is_err() {
        return (0, 0);
    }
    (c.PrivateUsage as u64, c.WorkingSetSize as u64)
}

/// This process's resident set from /proc (Linux), reported for both numbers.
#[cfg(not(windows))]
fn process_memory() -> (u64, u64) {
    let rss = std::fs::read_to_string("/proc/self/statm")
        .ok()
        .and_then(|s| s.split_whitespace().nth(1).and_then(|v| v.parse::<u64>().ok()))
        .unwrap_or(0)
        * 4096;
    (rss, rss)
}

/// Runs every case in a child process and prints the table and the gates; returns JSON rows.
pub fn measure() -> Vec<serde_json::Value> {
    let exe = std::env::current_exe().expect("own path");
    let mut rows = Vec::new();
    println!("P3: 200 columns, scrollback unlimited, after compressing cold pages");
    for (name, lines, dense, tabs) in CASES {
        let out = std::process::Command::new(&exe)
            .args(["memory-one", &lines.to_string(), if dense { "dense" } else { "typical" }, &tabs.to_string()])
            .output()
            .expect("run case");
        let text = String::from_utf8_lossy(&out.stdout);
        let v: Vec<f64> = text.split_whitespace().filter_map(|x| x.parse().ok()).collect();
        let (private, working, history) =
            (v.first().copied().unwrap_or(0.0), v.get(1).copied().unwrap_or(0.0), v.get(2).copied().unwrap_or(0.0));
        let limit = gate(lines);
        let pass = private < limit;
        println!(
            "{name:<14} private {private:8.1} MB  working set {working:8.1} MB  history rows {history:9.0}  gate {limit:.0} MB {}",
            if pass { "PASS" } else { "FAIL" }
        );
        rows.push(serde_json::json!({ "case": name, "lines": lines, "dense": dense, "tabs": tabs, "private_mb": private, "working_set_mb": working, "gate_mb": limit, "pass": pass }));
    }
    rows
}
