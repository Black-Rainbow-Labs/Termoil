//! Scripted runs for `terminus-headless run --script FILE` (the vttest goldens,
//! docs/TEST-SUITE.md section 5). A script drives an interactive program: it waits for text the
//! program prints, sends it input, and saves the screen at the points worth comparing.
//!
//! ```text
//! # a comment
//! wait "Enter choice number"     until the program prints this text after the last send
//! send "1\r"                     write bytes to the program (\r \n \e \t \\ \" and \xNN escapes)
//! snap cursor-1                  write the screen as text to <snap dir>/cursor-1.txt
//! ```
//!
//! `wait` looks only at bytes received since the last `send`, so a prompt still on the screen
//! from before the input never satisfies it. Waits end on a marker, never on a timer; the
//! timeout only turns a missing marker into a failure that shows the screen.

use std::time::Duration;

/// How long a `wait` may take before the run fails.
pub const WAIT_TIMEOUT: Duration = Duration::from_secs(10);

/// One script step.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Step {
    /// Wait until these bytes appear in the output received since the last send.
    Wait(Vec<u8>),
    /// Write these bytes to the program.
    Send(Vec<u8>),
    /// Save the screen under this name.
    Snap(String),
}

/// Parses a script. Errors name the line.
pub fn parse(text: &str) -> Result<Vec<Step>, String> {
    let mut steps = Vec::new();
    for (n, line) in text.lines().enumerate() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let (word, rest) = line.split_once(' ').unwrap_or((line, ""));
        let rest = rest.trim();
        let step = match word {
            "wait" => Step::Wait(unquote(rest).map_err(|e| format!("line {}: {e}", n + 1))?),
            "send" => Step::Send(unquote(rest).map_err(|e| format!("line {}: {e}", n + 1))?),
            "snap" if !rest.is_empty() && !rest.contains(['/', '\\']) => Step::Snap(rest.to_string()),
            _ => return Err(format!("line {}: cannot read '{line}'", n + 1)),
        };
        steps.push(step);
    }
    Ok(steps)
}

/// Reads a double quoted string with backslash escapes into bytes.
fn unquote(s: &str) -> Result<Vec<u8>, String> {
    let inner = s.strip_prefix('"').and_then(|s| s.strip_suffix('"')).ok_or("expected a quoted string")?;
    let mut out = Vec::new();
    let mut chars = inner.chars();
    while let Some(c) = chars.next() {
        if c != '\\' {
            let mut buf = [0u8; 4];
            out.extend_from_slice(c.encode_utf8(&mut buf).as_bytes());
            continue;
        }
        let b = match chars.next().ok_or("a backslash ends the string")? {
            'r' => b'\r',
            'n' => b'\n',
            't' => b'\t',
            'e' => 0x1b,
            '\\' => b'\\',
            '"' => b'"',
            'x' => {
                let hex: String = chars.by_ref().take(2).collect();
                u8::from_str_radix(&hex, 16).map_err(|_| format!("bad \\x escape '{hex}'"))?
            }
            other => return Err(format!("unknown escape \\{other}")),
        };
        out.push(b);
    }
    Ok(out)
}

/// A script being run: the steps, the next one, and the output since the last send.
pub struct Runner {
    steps: Vec<Step>,
    at: usize,
    since_send: Vec<u8>,
    waiting_since: Option<std::time::Instant>,
    snap_dir: std::path::PathBuf,
}

impl Runner {
    /// A runner for `steps` that writes snapshots into `snap_dir`.
    pub fn new(steps: Vec<Step>, snap_dir: std::path::PathBuf) -> Runner {
        Runner { steps, at: 0, since_send: Vec::new(), waiting_since: None, snap_dir }
    }

    /// Records program output.
    pub fn output(&mut self, bytes: &[u8]) {
        self.since_send.extend_from_slice(bytes);
    }

    /// True when every step has run.
    pub fn done(&self) -> bool {
        self.at >= self.steps.len()
    }

    /// Runs steps until one has to wait for more output. `screen` gives the lines of a snapshot. Fails when a wait has run past its timeout or a write fails.
    pub fn drive(
        &mut self, screen: &dyn Fn() -> Vec<String>, to_program: &mut dyn std::io::Write,
    ) -> Result<(), String> {
        while let Some(step) = self.steps.get(self.at) {
            match step {
                Step::Wait(marker) => {
                    if !contains(&self.since_send, marker) {
                        let since = *self.waiting_since.get_or_insert_with(std::time::Instant::now);
                        if since.elapsed() > WAIT_TIMEOUT {
                            let text = String::from_utf8_lossy(marker);
                            return Err(format!(
                                "step {}: '{text}' did not appear within {WAIT_TIMEOUT:?}",
                                self.at + 1
                            ));
                        }
                        return Ok(());
                    }
                    self.waiting_since = None;
                }
                Step::Send(bytes) => {
                    self.since_send.clear();
                    to_program.write_all(bytes).and_then(|_| to_program.flush()).map_err(|e| e.to_string())?;
                }
                Step::Snap(name) => {
                    let path = self.snap_dir.join(format!("{name}.txt"));
                    let text = screen().join("\n") + "\n";
                    std::fs::write(&path, text).map_err(|e| format!("cannot write {}: {e}", path.display()))?;
                }
            }
            self.at += 1;
        }
        Ok(())
    }
}

/// True when `needle` occurs in `haystack`.
pub fn contains(haystack: &[u8], needle: &[u8]) -> bool {
    needle.is_empty() || haystack.windows(needle.len()).any(|w| w == needle)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_every_step_and_escape() {
        let s = parse("# c\n\nwait \"Enter choice\"\nsend \"1\\r\\e[A\\x41\\\"\\\\\"\nsnap menu-1\n").unwrap();
        assert_eq!(
            s,
            vec![
                Step::Wait(b"Enter choice".to_vec()),
                Step::Send(b"1\r\x1b[AA\"\\".to_vec()),
                Step::Snap("menu-1".into())
            ]
        );
    }

    #[test]
    fn reports_the_line_of_an_error() {
        assert_eq!(parse("wait \"x\"\nsend x").unwrap_err(), "line 2: expected a quoted string");
        assert!(parse("snap ../x").is_err());
        assert!(parse("jump").is_err());
    }

    #[test]
    fn finds_a_marker() {
        assert!(contains(b"abc Push <RETURN>", b"<RETURN>"));
        assert!(!contains(b"abc", b"abcd"));
    }
}
