//! P2, the input latency benchmark (docs/TEST-SUITE.md section 13), after Contour's method.
//!
//! `termoil-gui --bench-latency <keys> <out.json>` opens a window whose pane runs
//! `termoil-gui --echo-child`: a program that prints a prompt line, then echoes every byte it
//! reads, wrapping at 60 columns. Keys are injected on the UI thread 25 to 35 ms apart, through the
//! same path a typed key takes. For each key the time from injection to the end of the first paint
//! whose frame shows the echoed character is recorded. Presents are unpaced (immediate mode) so the
//! display rate does not hide the pipeline. The result is written as JSON with p50, p90, p99 and max.

use std::time::{Duration, Instant};

use termoil_input::{Key, Modifiers, RawKey};

/// Columns the echo child wraps at.
pub const WRAP: usize = 60;

/// The benchmark state.
pub struct LatencyBench {
    /// Keys to inject.
    pub total: usize,
    /// Where to write the result.
    pub out: String,
    sent: usize,
    pending: Option<(Instant, usize)>,
    next_at: Option<Instant>,
    start_row: Option<usize>,
    samples: Vec<f64>,
    seed: u64,
}

impl LatencyBench {
    /// A benchmark of `total` keys writing to `out`.
    pub fn new(total: usize, out: String) -> LatencyBench {
        LatencyBench {
            total: total.clamp(1, 1500),
            out,
            sent: 0,
            pending: None,
            next_at: None,
            start_row: None,
            samples: Vec::new(),
            seed: 0x9e37_79b9_7f4a_7c15,
        }
    }

    /// The character key `k` sends.
    pub fn char_for(k: usize) -> char {
        (b'a' + (k % 26) as u8) as char
    }

    /// When the next key is due, if any.
    pub fn next_at(&self) -> Option<Instant> {
        self.next_at
    }

    /// The next key to inject now, when the previous one has been seen and the gap has passed.
    pub fn due(&mut self, now: Instant) -> Option<RawKey> {
        if self.start_row.is_none() || self.pending.is_some() || self.sent >= self.total {
            return None;
        }
        if self.next_at.is_some_and(|t| now < t) {
            return None;
        }
        let k = self.sent;
        self.sent += 1;
        self.pending = Some((now, k));
        self.next_at = None;
        let c = Self::char_for(k);
        Some(RawKey::new(Key::Char(c), Modifiers::NONE).with_text(&c.to_string()))
    }

    /// Called after each paint with a lookup of the drawn cell at (row, column). Records the
    /// pending key when its echo is in the frame; finds the first echo row from the prompt.
    pub fn after_paint(
        &mut self, painted: Instant, cell: impl Fn(usize, usize) -> Option<char>,
        prompt_row: impl Fn() -> Option<usize>,
    ) {
        if self.start_row.is_none() {
            self.start_row = prompt_row().map(|r| r + 1);
            self.next_at = Some(painted + Duration::from_millis(200));
            return;
        }
        let Some((t0, k)) = self.pending else { return };
        let row = self.start_row.unwrap_or(0) + k / WRAP;
        if cell(row, k % WRAP) == Some(Self::char_for(k)) {
            self.samples.push(painted.duration_since(t0).as_secs_f64() * 1000.0);
            self.pending = None;
            self.next_at = Some(painted + Duration::from_millis(25 + self.random() % 11));
        }
    }

    /// True when every key has been measured.
    pub fn done(&self) -> bool {
        self.samples.len() >= self.total
    }

    /// The result as JSON.
    pub fn report(&self, renderer: &str) -> String {
        let mut s = self.samples.clone();
        s.sort_by(f64::total_cmp);
        let q = |p: f64| s.get(((s.len() as f64 - 1.0) * p).round() as usize).copied().unwrap_or(0.0);
        format!(
            "{{\"bench\":\"p2_latency\",\"keys\":{},\"p50_ms\":{:.3},\"p90_ms\":{:.3},\"p99_ms\":{:.3},\"max_ms\":{:.3},\"renderer\":{:?},\"present\":\"immediate\"}}\n",
            s.len(),
            q(0.5),
            q(0.9),
            q(0.99),
            s.last().copied().unwrap_or(0.0),
            renderer
        )
    }

    /// A small xorshift generator for the gaps.
    fn random(&mut self) -> u64 {
        self.seed ^= self.seed << 13;
        self.seed ^= self.seed >> 7;
        self.seed ^= self.seed << 17;
        self.seed
    }
}

/// The echo child: prints a prompt line, then echoes each byte read, wrapping at 60 columns.
pub fn echo_child() -> std::io::Result<()> {
    let io = raw_console();
    io.write(
        b"termoil latency echo
",
    )?;
    let mut buf = [0u8; 64];
    let mut col = 0usize;
    loop {
        let n = io.read(&mut buf)?;
        if n == 0 {
            return Ok(());
        }
        let mut out = Vec::with_capacity(n + 2);
        for &b in &buf[..n] {
            if b == 3 || b == 4 {
                return Ok(());
            }
            out.push(b);
            col += 1;
            if col == WRAP {
                out.extend_from_slice(
                    b"
",
                );
                col = 0;
            }
        }
        io.write(&out)?;
    }
}

/// The console's input and output handles, read and written as bytes.
#[cfg(windows)]
struct RawIo {
    input: windows::Win32::Foundation::HANDLE,
    output: windows::Win32::Foundation::HANDLE,
}

#[cfg(windows)]
impl RawIo {
    /// Reads bytes as they arrive.
    fn read(&self, buf: &mut [u8]) -> std::io::Result<usize> {
        let mut n = 0u32;
        // SAFETY: the handle is this process's standard input and the buffer is valid for its length.
        unsafe { windows::Win32::Storage::FileSystem::ReadFile(self.input, Some(buf), Some(&mut n), None) }
            .map_err(std::io::Error::from)?;
        Ok(n as usize)
    }

    /// Writes all bytes.
    fn write(&self, data: &[u8]) -> std::io::Result<()> {
        let mut done = 0;
        while done < data.len() {
            let mut n = 0u32;
            // SAFETY: the handle is this process's standard output and the slice is valid.
            unsafe {
                windows::Win32::Storage::FileSystem::WriteFile(self.output, Some(&data[done..]), Some(&mut n), None)
            }
            .map_err(std::io::Error::from)?;
            done += n as usize;
        }
        Ok(())
    }
}

/// Puts the console in raw mode (no line editing, no echo, no input processing, VT input and
/// output) and returns its handles.
#[cfg(windows)]
fn raw_console() -> RawIo {
    use windows::Win32::System::Console::{
        CONSOLE_MODE, ENABLE_PROCESSED_OUTPUT, ENABLE_VIRTUAL_TERMINAL_INPUT, ENABLE_VIRTUAL_TERMINAL_PROCESSING,
        GetStdHandle, STD_INPUT_HANDLE, STD_OUTPUT_HANDLE, SetConsoleMode,
    };
    // SAFETY: the standard handles belong to this process; a failure leaves the default modes.
    unsafe {
        let input = GetStdHandle(STD_INPUT_HANDLE).unwrap_or_default();
        let output = GetStdHandle(STD_OUTPUT_HANDLE).unwrap_or_default();
        let _ = SetConsoleMode(input, CONSOLE_MODE(ENABLE_VIRTUAL_TERMINAL_INPUT.0));
        let _ = SetConsoleMode(output, ENABLE_PROCESSED_OUTPUT | ENABLE_VIRTUAL_TERMINAL_PROCESSING);
        RawIo { input, output }
    }
}

/// Standard input and output on Unix, with the terminal in raw mode.
#[cfg(unix)]
struct RawIo;

#[cfg(unix)]
impl RawIo {
    /// Reads bytes from standard input as they arrive.
    fn read(&self, buf: &mut [u8]) -> std::io::Result<usize> {
        use std::io::Read;
        std::io::stdin().read(buf)
    }

    /// Writes bytes to standard output.
    fn write(&self, data: &[u8]) -> std::io::Result<()> {
        use std::io::Write;
        let mut o = std::io::stdout();
        o.write_all(data)?;
        o.flush()
    }
}

/// Puts standard input's terminal in raw mode (no line editing, no echo, no signal characters)
/// and keeps output processing on, so a line feed still returns the cursor to column one.
#[cfg(unix)]
fn raw_console() -> RawIo {
    use rustix::termios::{OptionalActions, OutputModes, tcgetattr, tcsetattr};
    let stdin = rustix::stdio::stdin();
    if let Ok(mut modes) = tcgetattr(stdin) {
        modes.make_raw();
        modes.output_modes.insert(OutputModes::OPOST | OutputModes::ONLCR);
        let _ = tcsetattr(stdin, OptionalActions::Now, &modes);
    }
    RawIo
}
