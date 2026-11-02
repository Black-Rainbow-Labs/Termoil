//! S2: what the inbox ConPTY and the bundled ConPTY pass through for APC, DCS, OSC 133 and OSC 8.
//!
//! These tests assert the behaviour observed on 2026-11-01 (Windows 11 build 10.0.26100.6584, bundled
//! ConPTY 1.25.260930003) and fail if a Windows update or a new bundled pair changes it. The result
//! and the decision for kitty graphics are written up in docs/decisions/conpty.md.
#![cfg(windows)]

mod common;

use common::*;
use termoil_pty::ConptyConfig;

const ESC: &str = "\x1b";

/// The four test sequences, each wrapped in letters so they can be located in the output.
fn sequences() -> Vec<(&'static str, String)> {
    vec![
        ("APC kitty graphics query", format!("{ESC}_Ga=q,i=1;AAAA{ESC}\\")),
        ("DCS sixel", format!("{ESC}Pq#0;2;0;0;0#0~~{ESC}\\")),
        ("OSC 133;A", format!("{ESC}]133;A{ESC}\\")),
        ("OSC 8 hyperlink open", format!("{ESC}]8;;http://example.com{ESC}\\")),
    ]
}

/// Writes `payload` through a child on `config` and returns every byte the terminal side received.
fn bytes_for(name: &str, config: &ConptyConfig, payload: &str) -> Vec<u8> {
    let file = scratch_dir("s2").join(format!("payload-{name}.bin"));
    std::fs::write(&file, format!("BEGIN_S2\n{payload}\nEND_S2\n")).unwrap();
    let opts = command(&helper_path(), &["raw", file.to_str().unwrap()], size(100, 30));
    let session = Session::start(&opts, config);
    assert!(session.wait_for("END_S2"), "{name}: {}", show_bytes(&session.bytes()));
    let bytes = session.bytes();
    let log_dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../_agent_output/task-2458-conpty");
    std::fs::create_dir_all(&log_dir).ok();
    std::fs::write(log_dir.join(format!("s2-{name}.txt")), show_bytes(&bytes)).ok();
    bytes
}

/// The payload with all four sequences on separate lines.
fn combined_payload() -> String {
    sequences().iter().map(|(label, seq)| format!("[{label}]{seq}\n")).collect()
}

/// The bundled ConPTY delivers APC, DCS, OSC 133 and OSC 8 to the terminal byte for byte.
#[test]
fn bundled_conpty_passes_all_four_sequences_unchanged() {
    if !bundled_present() {
        println!("bundled conpty not fetched; skipping S2 for the bundled pair");
        return;
    }
    let (_, config) = configs().into_iter().find(|(n, _)| *n == "bundled").unwrap();
    let bytes = bytes_for("bundled", &config, &combined_payload());
    for (label, seq) in sequences() {
        assert!(
            find(&bytes, seq.as_bytes()).is_some(),
            "bundled ConPTY changed or dropped {label}; got {}",
            show_bytes(&bytes)
        );
    }
}

/// The inbox ConPTY drops APC and DCS, passes OSC 133 unchanged, and passes OSC 8 with an added id parameter.
#[test]
fn inbox_conpty_strips_apc_and_dcs_and_passes_osc() {
    let (_, config) = configs().into_iter().find(|(n, _)| *n == "inbox").unwrap();
    let bytes = bytes_for("inbox", &config, &combined_payload());
    let text = show_bytes(&bytes);
    let seqs = sequences();
    assert!(find(&bytes, seqs[0].1.as_bytes()).is_none() && !text.contains("Ga=q"), "inbox now passes APC: {text}");
    assert!(
        find(&bytes, seqs[1].1.as_bytes()).is_none() && !text.contains("#0;2;0;0;0"),
        "inbox now passes DCS: {text}"
    );
    assert!(find(&bytes, seqs[2].1.as_bytes()).is_some(), "inbox changed OSC 133: {text}");
    assert!(
        text.contains("ESC ]8;id=") && text.contains(";http://example.com"),
        "inbox changed OSC 8 handling: {text}"
    );
}

/// Flags 0x2, 0x4 and 0x8 do not make the inbox ConPTY pass APC or DCS (no passthrough flag exists there).
#[test]
fn inbox_flags_do_not_enable_passthrough() {
    let combined = combined_payload();
    for flags in [0x2u32, 0x4, 0x8, 0x10, 0x20] {
        let config = ConptyConfig { choice: termoil_pty::ConptyChoice::Inbox, flags: Some(flags) };
        let bytes = bytes_for(&format!("inbox-flags-{flags:x}"), &config, &combined);
        assert!(!show_bytes(&bytes).contains("Ga=q"), "inbox flag {flags:#x} now passes APC");
        assert!(!show_bytes(&bytes).contains("#0;2;0;0;0"), "inbox flag {flags:#x} now passes DCS");
    }
}

/// A kitty graphics transfer split into 4096 byte chunks (about 1 MiB of base64) arrives through the bundled ConPTY complete.
#[test]
fn bundled_conpty_passes_a_chunked_kitty_transfer() {
    if !bundled_present() {
        println!("bundled conpty not fetched; skipping");
        return;
    }
    let (_, config) = configs().into_iter().find(|(n, _)| *n == "bundled").unwrap();
    let chunk = "QUJD".repeat(1024);
    let mut payload = String::new();
    for i in 0..256 {
        let more = if i == 255 { 0 } else { 1 };
        let head = if i == 0 { "a=T,f=24,s=512,v=512,m=1" } else { &format!("m={more}") };
        payload.push_str(&format!("{ESC}_G{head};{chunk}{ESC}\\"));
    }
    let bytes = bytes_for("bundled-chunked", &config, &payload);
    let received = show_bytes(&bytes);
    assert_eq!(received.matches("ESC _G").count(), 256, "chunk count");
    assert_eq!(received.matches("QUJDQUJD").count(), 256 * 512, "payload bytes");
}
