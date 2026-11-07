//! Unit tests for telemetry: names, permission, the clock, the payload limits and the spool. None of them
//! touch the process wide switch, so they run in parallel with the rest of the crate.

use std::collections::BTreeMap;
use std::time::{Duration, Instant};

use serde_json::{Value, json};

use super::activity::{ACTIVE_FOR, Activity, LONGEST_QUIET};
use super::payload::{self, Context, MOST_EVENTS, MOST_REQUESTS_AT_ONCE};
use super::sender::{claim_the_leftovers, still_wanted};
use super::*;

/// A scratch folder for one test, removed when it ends.
struct Scratch(std::path::PathBuf);

impl Scratch {
    /// A new empty folder named after the test.
    fn new(tag: &str) -> Scratch {
        let dir = std::env::temp_dir().join(format!("termoil-telemetry-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        Scratch(dir)
    }
}

impl Drop for Scratch {
    /// Removes the folder.
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

#[test]
fn a_name_with_a_path_or_spaces_is_never_sent() {
    assert_eq!(clean_name("new-tab"), "new-tab");
    assert_eq!(clean_name("C:\\Users\\jason"), "");
    assert_eq!(clean_name("/home/jason"), "");
    assert_eq!(clean_name("rm -rf"), "");
    assert_eq!(clean_name(&"a".repeat(150)).len(), 100);
}

#[test]
fn shells_are_sent_by_kind_and_anything_else_is_other() {
    use crate::shell_integration::detect;
    assert_eq!(shell_name(detect(r"C:\Program Files\PowerShell\7\pwsh.exe")), "pwsh");
    assert_eq!(shell_name(detect("/bin/bash")), "bash");
    assert_eq!(shell_name(detect(r"C:\secret\project\tool.exe")), "other");
}

#[test]
fn the_environment_and_the_mode_decide_whether_a_process_may_send() {
    assert_eq!(Permission::for_the_binary(false, false, None), Permission::Allowed);
    assert_eq!(Permission::for_the_binary(true, false, None), Permission::Headless);
    assert_eq!(Permission::for_the_binary(false, true, None), Permission::Benchmark);
    assert_eq!(Permission::for_the_binary(true, false, Some("on")), Permission::Allowed);
    assert_eq!(Permission::for_the_binary(false, false, Some("OFF")), Permission::Environment);
    assert_eq!(Permission::for_the_binary(true, true, Some("debug")), Permission::Allowed);
}

#[test]
fn a_bundled_theme_is_sent_by_its_name_and_any_other_as_other() {
    assert_eq!(
        theme_name(crate::config::DEFAULT_THEME),
        crate::config::DEFAULT_THEME.to_ascii_lowercase().replace(' ', "-")
    );
    assert!(!theme_name(crate::config::DEFAULT_THEME).is_empty());
    assert_eq!(theme_name("My Secret Theme"), "other");
    assert_eq!(theme_name(""), "other");
}

#[test]
fn launch_kinds() {
    assert_eq!(launch_kind(false, false), "plain");
    assert_eq!(launch_kind(true, false), "folder");
    assert_eq!(launch_kind(true, true), "command");
}

#[test]
fn active_time_is_focused_time_within_a_minute_of_input() {
    let t0 = Instant::now();
    let at = |s: u64| t0 + Duration::from_secs(s);
    let mut clock = Activity::new(t0);
    clock.focus(true, at(0));
    clock.input(at(10));
    clock.input(at(20));
    // Ten seconds between the two inputs, then a minute after the last one; nothing after that.
    let (open, focused, active) = clock.totals(at(200));
    assert_eq!(open, Duration::from_secs(200));
    assert_eq!(focused, Duration::from_secs(200));
    assert_eq!(active, Duration::from_secs(10) + ACTIVE_FOR);
}

#[test]
fn nothing_is_active_or_focused_while_no_window_has_the_focus() {
    let t0 = Instant::now();
    let at = |s: u64| t0 + Duration::from_secs(s);
    let mut clock = Activity::new(t0);
    clock.input(at(5));
    clock.focus(true, at(10));
    clock.input(at(10));
    clock.focus(false, at(30));
    clock.input(at(40));
    let (_, focused, active) = clock.totals(at(500));
    assert_eq!(focused, Duration::from_secs(20));
    assert_eq!(active, Duration::from_secs(20));
}

#[test]
fn a_long_quiet_stretch_counts_at_most_thirty_minutes_of_focus() {
    let t0 = Instant::now();
    let mut clock = Activity::new(t0);
    clock.focus(true, t0);
    let (_, focused, _) = clock.totals(t0 + Duration::from_secs(10 * 60 * 60));
    assert_eq!(focused, LONGEST_QUIET);
}

#[test]
fn engagement_is_reset_by_each_request() {
    let t0 = Instant::now();
    let at = |s: u64| t0 + Duration::from_secs(s);
    let mut clock = Activity::new(t0);
    clock.focus(true, at(0));
    clock.input(at(0));
    assert_eq!(clock.take_engagement(at(30)), Duration::from_secs(30));
    assert_eq!(clock.take_engagement(at(100)), Duration::from_secs(30));
    assert_eq!(clock.take_engagement(at(200)), Duration::ZERO);
    let (_, _, active) = clock.totals(at(200));
    assert_eq!(active, ACTIVE_FOR);
}

/// A context for the payload tests.
fn context() -> Context {
    Context { client_id: "ab".repeat(16), session_id: 42, version: "1.2.3".into(), debug: false }
}

#[test]
fn a_request_carries_at_most_twenty_five_events_and_at_most_four_requests_go_at_once() {
    let events: Vec<Value> = (0..200).map(|n| payload::event("feature", json!({ "uses": n }))).collect();
    let bodies = payload::bodies(&context(), events, 7);
    assert_eq!(bodies.len(), MOST_REQUESTS_AT_ONCE);
    for body in &bodies {
        let v: Value = serde_json::from_str(body).unwrap();
        assert_eq!(v["events"].as_array().unwrap().len(), MOST_EVENTS);
        assert_eq!(v["client_id"].as_str(), Some("abababababababababababababababab"));
        assert_eq!(v["timestamp_micros"].as_u64(), Some(7));
        assert_eq!(v["events"][0]["params"]["session_id"].as_str(), Some("42"));
        assert_eq!(v["events"][0]["params"]["engagement_time_msec"].as_u64(), Some(1));
        assert!(v["events"][0]["params"].get("debug_mode").is_none());
    }
}

#[test]
fn names_and_values_are_cut_to_ga4_limits() {
    let long = "x".repeat(300);
    let e = payload::event(&long, json!({ long.clone(): long.clone() }));
    assert_eq!(e["name"].as_str().unwrap().len(), 40);
    let (key, value) = e["params"].as_object().unwrap().iter().next().unwrap();
    assert_eq!((key.len(), value.as_str().unwrap().len()), (40, 100));
}

#[test]
fn features_go_most_used_first_and_what_does_not_fit_waits() {
    let mut tally = BTreeMap::new();
    for n in 0..30u32 {
        tally.insert((Category::Action, format!("a{n}")), n + 1);
    }
    let (rows, left) = payload::feature_rows(tally, 10);
    assert_eq!(rows.len(), 10);
    assert_eq!(rows[0]["params"]["feature"].as_str(), Some("a29"));
    assert_eq!(rows[0]["params"]["uses"].as_u64(), Some(30));
    assert_eq!(left.len(), 20);
    assert!(left.contains_key(&(Category::Action, "a0".to_string())));
}

#[test]
fn debug_mode_is_marked_on_every_event() {
    let mut ctx = context();
    ctx.debug = true;
    let body = payload::bodies(&ctx, vec![payload::heartbeat(1500)], 1).remove(0);
    let v: Value = serde_json::from_str(&body).unwrap();
    assert_eq!(v["events"][0]["params"]["debug_mode"].as_u64(), Some(1));
    assert_eq!(v["events"][0]["params"]["engagement_time_msec"].as_u64(), Some(1500));
}

#[test]
fn the_spool_drops_what_ga4_would_refuse_and_keeps_the_newest_two_hundred() {
    let now = 100 * 60 * 60 * 1_000_000u64;
    let line = |at: u64| format!("{{\"timestamp_micros\":{at}}}");
    let mut lines = vec![line(now - 73 * 60 * 60 * 1_000_000), "not json".to_string()];
    lines.extend((0..250).map(|n| line(now - n)));
    let kept = still_wanted(lines, now);
    assert_eq!(kept.len(), 200);
    assert_eq!(kept[0], line(now - 50));
}

#[test]
fn a_starting_process_claims_the_spool_and_files_of_processes_that_have_gone() {
    let scratch = Scratch::new("claim");
    let dir = &scratch.0;
    std::fs::write(dir.join("spool.jsonl"), "{}\n").unwrap();
    // A process id that is not running, and this process's own, which is never claimed.
    std::fs::write(dir.join("sending-4294967294-0.jsonl"), "{}\n").unwrap();
    let own = format!("sending-{}-0.jsonl", std::process::id());
    std::fs::write(dir.join(&own), "{}\n").unwrap();
    std::fs::write(dir.join("id.txt"), "x").unwrap();
    let claimed = claim_the_leftovers(dir, std::process::id());
    assert_eq!(claimed.len(), 2, "{claimed:?}");
    assert!(!dir.join("spool.jsonl").exists());
    assert!(dir.join(&own).exists() && dir.join("id.txt").exists());
}

#[test]
fn the_client_id_is_random_hex_kept_between_starts() {
    let scratch = Scratch::new("id");
    let (id, first) = client_id(&scratch.0);
    assert!(first && id.len() == 32 && id.chars().all(|c| c.is_ascii_hexdigit()));
    assert_eq!(client_id(&scratch.0), (id, false));
}

#[test]
fn a_new_crash_report_is_noticed_once() {
    let scratch = Scratch::new("crash");
    let crashes = scratch.0.join("crashes");
    std::fs::create_dir_all(&crashes).unwrap();
    assert!(!crashed_since_last_start(&crashes, &scratch.0));
    std::fs::write(crashes.join("crash-1.txt"), "boom").unwrap();
    assert!(crashed_since_last_start(&crashes, &scratch.0));
    assert!(!crashed_since_last_start(&crashes, &scratch.0));
}

#[test]
fn the_body_is_posted_through_curl() {
    use std::io::{BufRead, BufReader, Read, Write};
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let url = format!("http://{}/mp/collect", listener.local_addr().unwrap());
    let server = std::thread::spawn(move || {
        let (mut stream, _) = listener.accept().unwrap();
        let mut reader = BufReader::new(stream.try_clone().unwrap());
        let mut length = 0;
        loop {
            let mut header = String::new();
            reader.read_line(&mut header).unwrap();
            if header.trim().is_empty() {
                break;
            }
            if let Some(v) = header.to_ascii_lowercase().strip_prefix("content-length:") {
                length = v.trim().parse().unwrap();
            }
        }
        let mut body = vec![0u8; length];
        reader.read_exact(&mut body).unwrap();
        stream.write_all(b"HTTP/1.1 204 No Content\r\nConnection: close\r\n\r\n").unwrap();
        String::from_utf8(body).unwrap()
    });
    crate::update::fetch::post_json(&url, "{\"a\":1}", Duration::from_secs(10)).unwrap();
    assert_eq!(server.join().unwrap(), "{\"a\":1}");
}

#[test]
fn defaults_are_on_every_twenty_minutes() {
    let config = crate::config::Config::default();
    assert!(config.telemetry.enabled);
    assert_eq!(config.telemetry.interval(), Duration::from_secs(20 * 60));
}
