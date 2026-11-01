//! E2: initial size, resize delivery, and large output arriving complete and in order.

mod common;

use common::*;
use std::io::Read;
#[cfg(unix)]
use std::io::Write;
use std::time::Instant;
use terminus_pty::Pty;

/// PowerShell that prints `SIZE=<w>x<h>` once the console is `want_w` by `want_h`.
fn size_script(want_w: u16, want_h: u16) -> String {
    format!(
        "$d=(Get-Date).AddSeconds(25); do {{ $s=$Host.UI.RawUI.WindowSize; if ($s.Width -eq {want_w} -and $s.Height -eq {want_h}) {{ break }}; Start-Sleep -Milliseconds 50 }} while ((Get-Date) -lt $d); Write-Host ('SIZE=' + $s.Width + 'x' + $s.Height + '.')"
    )
}

/// The size given at spawn is the size the child sees.
#[cfg(windows)]
#[test]
fn initial_size_reaches_child() {
    for (name, config) in configs() {
        let script = size_script(100, 30);
        let mut session =
            Session::start(&command("powershell.exe", &["-NoProfile", "-Command", &script], size(100, 30)), &config);
        assert!(session.wait_for("SIZE="), "{name}: {}", show_bytes(&session.bytes()));
        assert!(session.wait_for("."), "{name}");
        session.pty.wait().unwrap();
        assert!(session.plain_text().contains("SIZE=100x30."), "{name}: {}", session.plain_text());
    }
}

/// A resize after the child started is visible to the child.
#[cfg(windows)]
#[test]
fn resize_reaches_child() {
    for (name, config) in configs() {
        let script = format!("Write-Host 'READY.'; {}", size_script(132, 43));
        let mut session =
            Session::start(&command("powershell.exe", &["-NoProfile", "-Command", &script], size(100, 30)), &config);
        assert!(session.wait_for("READY."), "{name}: {}", show_bytes(&session.bytes()));
        session.pty.resize(size(132, 43)).expect("resize");
        assert!(session.wait_for("SIZE="), "{name}");
        session.pty.wait().unwrap();
        assert!(session.plain_text().contains("SIZE=132x43."), "{name}: {}", session.plain_text());
    }
}

/// On Unix `stty size` reports the initial size and the size after a resize.
#[cfg(unix)]
#[test]
fn unix_size_and_resize() {
    let script = "stty size; read x; stty size; echo TERMINUS_MARK";
    let mut session = Session::start(&command("/bin/sh", &["-c", script], size(100, 30)), &Default::default());
    assert!(session.wait_for("30 100"));
    session.pty.resize(size(132, 43)).unwrap();
    session.writer.write_all(b"\n").unwrap();
    assert!(session.wait_for("TERMINUS_MARK"));
    assert!(session.plain_text().contains("43 132"));
}

/// Extracts every `LINE <n>` number from output that has had escapes removed.
fn line_numbers(text: &str) -> Vec<u32> {
    let mut numbers = Vec::new();
    let mut rest = text;
    while let Some(at) = rest.find("LINE ") {
        let digits: String = rest[at + 5..].chars().take_while(|c| c.is_ascii_digit()).collect();
        if digits.len() == 7 {
            numbers.push(digits.parse().unwrap());
        }
        rest = &rest[at + 5..];
    }
    numbers
}

/// Reads from `reader` on this thread until `marker` has been seen or the deadline passes; returns all bytes and the elapsed time.
fn read_until_marker(mut reader: Box<dyn Read + Send>, marker: &[u8]) -> (Vec<u8>, std::time::Duration, bool) {
    let started = Instant::now();
    let mut all: Vec<u8> = Vec::with_capacity(80 << 20);
    let mut chunk = vec![0u8; 64 * 1024];
    let mut found = false;
    while started.elapsed() < DEADLINE * 4 {
        let n = match reader.read(&mut chunk) {
            Ok(0) | Err(_) => break,
            Ok(n) => n,
        };
        let from = all.len().saturating_sub(marker.len());
        all.extend_from_slice(&chunk[..n]);
        if find(&all[from..], marker).is_some() {
            found = true;
            break;
        }
    }
    (all, started.elapsed(), found)
}

/// 64 MB of numbered lines arrive complete and in order; prints the single thread read throughput.
#[test]
fn large_output_is_complete_and_ordered() {
    const COUNT: u32 = 5_000_000;
    for (name, config) in configs() {
        let mut pty =
            Pty::spawn_with_config(&command(&helper_path(), &["numbers", &COUNT.to_string()], size(120, 40)), &config)
                .expect("spawn");
        let reader = pty.take_reader().unwrap();
        let (bytes, elapsed, found) = read_until_marker(reader, format!("LINE {COUNT:07}").as_bytes());
        assert!(found, "{name}: last line never arrived after {} bytes", bytes.len());
        pty.wait().unwrap();
        let sent_mb = f64::from(COUNT) * 13.0 / 1_048_576.0;
        let recv_mb = bytes.len() as f64 / 1_048_576.0;
        println!(
            "THROUGHPUT {name}: child wrote {sent_mb:.1} MiB, received {recv_mb:.1} MiB in {:.2} s = {:.1} MiB/s of child output, {:.1} MiB/s of received bytes",
            elapsed.as_secs_f64(),
            sent_mb / elapsed.as_secs_f64(),
            recv_mb / elapsed.as_secs_f64()
        );
        let numbers = line_numbers(&strip_escapes(&bytes));
        assert_eq!(numbers.len(), COUNT as usize, "{name}: line count");
        assert!(numbers.iter().enumerate().all(|(i, n)| *n as usize == i + 1), "{name}: lines out of order or missing");
    }
}
