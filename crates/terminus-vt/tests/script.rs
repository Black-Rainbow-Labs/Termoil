//! U2: control function behaviour, from text scripts in `corpus/vt/*.test`.
//!
//! A script is a list of steps, one per line:
//!
//! ```text
//! # comment
//! SIZE 10 4                  new terminal, 10 columns by 4 rows
//! PUSH "ab\e[2Ccd\r\n"       feed bytes; \e is ESC, \xNN a byte, \uXXXX a code point (braced hex)
//! RESIZE 8 4                 resize the terminal
//! CURSOR 1 0                 the cursor is at row 1, column 0 (0 based)
//! ROW 0 "ab  cd"             row 0's text, trailing blanks removed
//! SCREEN                     every row's text, one per line, until END
//! |ab  cd
//! END
//! CELL 0 1 "b" w=1 bold      the full dump of one cell
//! ROWDUMP 0 ...              the full dump of one row
//! REPLY "\e[0n"              the bytes the terminal sent back since the last REPLY
//! MODE ?25 1                 the DECRQM answer for a mode
//! TITLE "text"               the window title
//! EVENT Bell                 the next queued event (Debug form)
//! HISTORY 2                  rows of scrollback
//! HISTROW 0 "text"           a scrollback row
//! WRAP 0 1                   row 0's wrap flag
//! PEN fg=p1 bold             the cursor's pen
//! ALT 1                      the alternate screen is shown
//! KITTY 5                    the kitty keyboard flags
//! ```
//!
//! Every script runs three times: bytes whole, one byte at a time, and in seeded random chunks.
//! With `TERMINUS_BLESS=1` the first run rewrites each expectation with the actual value.

use std::collections::VecDeque;
use std::path::{Path, PathBuf};

use terminus_vt::dump;
use terminus_vt::modes::ModeKind;
use terminus_vt::{Event, Terminal, TerminalConfig};

/// How PUSH bytes are fed.
#[derive(Clone, Copy, Debug)]
enum Feed {
    Whole,
    Bytes,
    Random(u64),
}

/// The state of one script run.
struct Run {
    term: Terminal,
    replies: Vec<u8>,
    events: VecDeque<Event>,
    feed: Feed,
    rng: u64,
}

impl Run {
    /// A run with a default sized terminal.
    fn new(feed: Feed) -> Run {
        Run {
            term: Terminal::new(80, 24, TerminalConfig::default()),
            replies: Vec::new(),
            events: VecDeque::new(),
            feed,
            rng: 0x9e37_79b9,
        }
    }

    /// Feeds bytes the way this run feeds them, collecting replies and events.
    fn push(&mut self, bytes: &[u8]) {
        match self.feed {
            Feed::Whole => self.term.advance(bytes),
            Feed::Bytes => bytes.iter().for_each(|b| self.term.advance(&[*b])),
            Feed::Random(seed) => {
                self.rng ^= seed;
                let mut i = 0;
                while i < bytes.len() {
                    self.rng ^= self.rng << 13;
                    self.rng ^= self.rng >> 7;
                    self.rng ^= self.rng << 17;
                    let n = 1 + (self.rng % 9) as usize;
                    let end = (i + n).min(bytes.len());
                    self.term.advance(&bytes[i..end]);
                    i = end;
                }
            }
        }
        self.replies.extend(self.term.take_replies());
        self.events.extend(self.term.take_events());
    }
}

/// Decodes the escapes in a quoted script string.
fn unescape(s: &str) -> Vec<u8> {
    let inner = s.trim().strip_prefix('"').and_then(|s| s.strip_suffix('"')).unwrap_or(s.trim());
    let mut out = Vec::new();
    let mut chars = inner.chars().peekable();
    while let Some(c) = chars.next() {
        if c != '\\' {
            let mut buf = [0u8; 4];
            out.extend_from_slice(c.encode_utf8(&mut buf).as_bytes());
            continue;
        }
        match chars.next() {
            Some('e') => out.push(0x1b),
            Some('r') => out.push(b'\r'),
            Some('n') => out.push(b'\n'),
            Some('t') => out.push(b'\t'),
            Some('a') => out.push(0x07),
            Some('b') => out.push(0x08),
            Some('\\') => out.push(b'\\'),
            Some('"') => out.push(b'"'),
            Some('x') => {
                let hex: String = chars.by_ref().take(2).collect();
                out.push(u8::from_str_radix(&hex, 16).expect("\\x needs two hex digits"));
            }
            Some('u') => {
                assert_eq!(chars.next(), Some('{'), "write code points as \\u{{e9}}");
                let hex: String = chars.by_ref().take_while(|c| *c != '}').collect();
                let c = char::from_u32(u32::from_str_radix(&hex, 16).expect("hex inside \\u{{}}"))
                    .expect("valid code point");
                let mut buf = [0u8; 4];
                out.extend_from_slice(c.encode_utf8(&mut buf).as_bytes());
            }
            other => panic!("unknown escape \\{other:?}"),
        }
    }
    out
}

/// Quotes a string for a script, escaping what [`unescape`] reads back.
fn quote(bytes: &[u8]) -> String {
    let text = String::from_utf8_lossy(bytes);
    let mut out = String::from("\"");
    for c in text.chars() {
        match c {
            '\x1b' => out.push_str("\\e"),
            '\r' => out.push_str("\\r"),
            '\n' => out.push_str("\\n"),
            '\t' => out.push_str("\\t"),
            '\x07' => out.push_str("\\a"),
            '\\' => out.push_str("\\\\"),
            '"' => out.push_str("\\\""),
            c if (c as u32) < 0x20 || c as u32 == 0x7f => out.push_str(&format!("\\x{:02x}", c as u32)),
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

/// Splits `KEYWORD rest`.
fn split_word(line: &str) -> (&str, &str) {
    let line = line.trim_start();
    match line.split_once(' ') {
        Some((a, b)) => (a, b),
        None => (line, ""),
    }
}

/// Parses `a b` as two numbers.
fn two_nums(rest: &str) -> (u16, u16) {
    let mut it = rest.split_whitespace().map(|s| s.parse::<u16>().expect("a number"));
    (it.next().expect("first number"), it.next().expect("second number"))
}

/// The actual value for an expectation line, in the same syntax, so bless can write it back.
fn actual_line(run: &mut Run, word: &str, rest: &str) -> Option<String> {
    let t = &run.term;
    Some(match word {
        "CURSOR" => {
            let c = &t.screen().cursor;
            format!("CURSOR {} {}{}", c.row, c.col, if c.pending_wrap { " wrap" } else { "" })
        }
        "ROW" => {
            let r: u16 = rest.split_whitespace().next()?.parse().ok()?;
            format!("ROW {r} {}", quote(dump::text_dump(t).get(r as usize)?.as_bytes()))
        }
        "CELL" => {
            let (r, c) = two_nums(rest);
            let pages = &t.screen().pages;
            let s = pages.with_row(pages.active_y(r), |p, i| dump::cell_string(p, i, c as usize));
            format!("CELL {r} {c} {s}")
        }
        "ROWDUMP" => {
            let r: u16 = rest.split_whitespace().next()?.parse().ok()?;
            format!("ROWDUMP {r} {}", dump::row_dump(t, r))
        }
        "REPLY" => format!("REPLY {}", quote(&std::mem::take(&mut run.replies))),
        "MODE" => {
            let spec = rest.split_whitespace().next()?;
            let (kind, num) = match spec.strip_prefix('?') {
                Some(n) => (ModeKind::Dec, n),
                None => (ModeKind::Ansi, spec),
            };
            format!("MODE {spec} {}", t.modes().report(kind, num.parse().ok()?))
        }
        "TITLE" => format!("TITLE {}", quote(t.title().as_bytes())),
        "EVENT" => {
            format!("EVENT {}", run.events.pop_front().map(|e| format!("{e:?}")).unwrap_or_else(|| "none".into()))
        }
        "HISTORY" => format!("HISTORY {}", t.primary().pages.history_rows()),
        "HISTROW" => {
            let r: usize = rest.split_whitespace().next()?.parse().ok()?;
            format!("HISTROW {r} {}", quote(dump::history_text(t).get(r)?.as_bytes()))
        }
        "WRAP" => {
            let r: u16 = rest.split_whitespace().next()?.parse().ok()?;
            let pages = &t.screen().pages;
            let wrap = pages.row_header(pages.active_y(r)).has(terminus_vt::cell::row_flags::WRAP);
            format!("WRAP {r} {}", wrap as u8)
        }
        "PEN" => {
            let s = &t.screen().cursor.style;
            let mut page = terminus_vt::page::Page::new(1, 1);
            page.push_row();
            let id = page.intern_style(s);
            page.write_cell(0, 0, terminus_vt::cell::Cell::new('x', id));
            let full = dump::cell_string(&page, 0, 0);
            format!("PEN{}", full.strip_prefix("\"x\" w=1").unwrap_or(&full))
        }
        "ALT" => format!("ALT {}", t.is_alt_screen() as u8),
        "KITTY" => format!("KITTY {}", t.screen().kitty_keyboard.flags()),
        _ => return None,
    })
}

/// Normalises an expectation line for comparison. The package version and its DA2 number are
/// written `{version}` and `{vnum}` in scripts so a release bump does not change expectations.
fn norm(s: &str) -> String {
    let v = env!("CARGO_PKG_VERSION");
    let mut parts = v.split('.').map(|p| p.parse::<u32>().unwrap_or(0));
    let num = parts.next().unwrap_or(0) * 10000 + parts.next().unwrap_or(0) * 100 + parts.next().unwrap_or(0);
    let s = s.replace(v, "{version}").replace(&format!(">1;{num};0c"), ">1;{vnum};0c");
    s.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// Runs one script; returns the rewritten text when blessing.
fn run_script(path: &Path, text: &str, feed: Feed, bless: bool) -> String {
    let mut run = Run::new(feed);
    let lines: Vec<&str> = text.lines().collect();
    let mut out: Vec<String> = Vec::with_capacity(lines.len());
    let mut i = 0;
    while i < lines.len() {
        let line = lines[i];
        let (word, rest) = split_word(line);
        i += 1;
        match word {
            "" | "#" => out.push(line.to_string()),
            w if w.starts_with('#') => out.push(line.to_string()),
            "SIZE" => {
                let (c, r) = two_nums(rest);
                run = Run {
                    term: Terminal::new(c, r, TerminalConfig::default()),
                    replies: Vec::new(),
                    events: VecDeque::new(),
                    feed,
                    rng: 0x9e37_79b9,
                };
                out.push(line.to_string());
            }
            "RESIZE" => {
                let (c, r) = two_nums(rest);
                run.term.resize(c, r);
                run.replies.extend(run.term.take_replies());
                run.events.extend(run.term.take_events());
                out.push(line.to_string());
            }
            "PUSH" => {
                run.push(&unescape(rest));
                out.push(line.to_string());
            }
            "SCREEN" => {
                let mut expected = Vec::new();
                while i < lines.len() && lines[i].trim() != "END" {
                    expected.push(lines[i].strip_prefix('|').unwrap_or(lines[i]).to_string());
                    i += 1;
                }
                i += 1;
                let actual = dump::text_dump(&run.term);
                if !bless && let Some(d) = dump::diff(&expected, &actual) {
                    panic!("{} line {i} ({feed:?}): SCREEN differs at {d}", path.display());
                }
                out.push("SCREEN".into());
                out.extend(actual.iter().map(|r| format!("|{r}")));
                out.push("END".into());
            }
            _ => {
                let actual = actual_line(&mut run, word, rest)
                    .unwrap_or_else(|| panic!("{} line {i}: unknown step {word}", path.display()));
                if !bless && norm(&actual) != norm(line) {
                    panic!(
                        "{} line {i} ({feed:?}):\n  expected: {}\n  actual:   {}",
                        path.display(),
                        line.trim(),
                        actual
                    );
                }
                out.push(actual);
            }
        }
    }
    let mut s = out.join("\n");
    s.push('\n');
    s
}

/// The script files.
fn scripts() -> Vec<PathBuf> {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../corpus/vt");
    let mut v: Vec<PathBuf> = std::fs::read_dir(&dir)
        .expect("corpus/vt exists")
        .filter_map(|e| e.ok().map(|e| e.path()))
        .filter(|p| p.extension().is_some_and(|x| x == "test"))
        .collect();
    v.sort();
    v
}

#[test]
fn control_function_scripts() {
    let bless = std::env::var("TERMINUS_BLESS").is_ok_and(|v| v == "1");
    let files = scripts();
    assert!(!files.is_empty(), "no scripts found");
    let mut cases = 0;
    for path in &files {
        let text = std::fs::read_to_string(path).expect("readable script").replace("\r\n", "\n");
        cases += text
            .lines()
            .filter(|l| {
                !l.trim().is_empty()
                    && !l.trim_start().starts_with('#')
                    && !matches!(split_word(l).0, "SIZE" | "PUSH" | "RESIZE" | "END")
                    && !l.starts_with('|')
            })
            .count();
        let blessed = run_script(path, &text, Feed::Whole, bless);
        if bless && blessed != text {
            std::fs::write(path, &blessed).expect("write blessed script");
            eprintln!("blessed {}", path.display());
        }
        let text = if bless { blessed } else { text };
        run_script(path, &text, Feed::Bytes, false);
        run_script(path, &text, Feed::Random(0x5eed), false);
    }
    eprintln!("U2: {} scripts, {cases} expectations, each run three ways", files.len());
}
