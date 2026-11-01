//! Terminus pseudoterminals: openpty on Unix and ConPTY on Windows, spawn, resize and typed exit status.

use std::io::{self, Read, Write};
use std::path::PathBuf;

#[cfg(unix)]
mod unix;
#[cfg(unix)]
use unix as imp;

#[cfg(windows)]
mod conpty_api;
#[cfg(windows)]
mod windows;
#[cfg(windows)]
use windows as imp;

mod options;

pub use options::{PtySize, SpawnOptions};

/// How a child process ended: a normal exit code or, on Unix, the signal that killed it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExitStatus {
    Code(i32),
    Signal(i32),
}

/// Which pseudoconsole implementation a Windows `Pty` is using.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConptySource {
    /// The `conpty.dll` and `OpenConsole.exe` pair that ships beside the executable.
    Bundled,
    /// The pseudoconsole built into kernel32 and the inbox conhost.exe.
    Inbox,
    /// Not Windows.
    NotApplicable,
}

/// Which Windows pseudoconsole `Pty::spawn_with_config` should use. Ignored on Unix.
#[derive(Debug, Clone, Default)]
pub enum ConptyChoice {
    /// Bundled from `TERMINUS_CONPTY_DIR` or the executable's directory when present, else inbox.
    #[default]
    Auto,
    /// Always the kernel32 pseudoconsole.
    Inbox,
    /// The `conpty.dll` in this directory; spawning fails when it cannot be loaded.
    Bundled(PathBuf),
}

/// Windows pseudoconsole settings for tests and diagnostics. Ignored on Unix.
#[derive(Debug, Clone, Default)]
pub struct ConptyConfig {
    /// Which pseudoconsole implementation to load.
    pub choice: ConptyChoice,
    /// CreatePseudoConsole flags. `None` selects the default for the implementation.
    pub flags: Option<u32>,
}

/// A running child process attached to a pseudoterminal. Dropping it ends the child and its tree.
pub struct Pty {
    inner: imp::PtyImpl,
}

impl Pty {
    /// Starts `opts.program` on a new pseudoterminal using the default Windows pseudoconsole choice.
    pub fn spawn(opts: &SpawnOptions) -> io::Result<Pty> {
        Self::spawn_with_config(opts, &ConptyConfig::default())
    }

    /// Starts a child like `spawn`, with an explicit Windows pseudoconsole implementation and flags.
    pub fn spawn_with_config(opts: &SpawnOptions, config: &ConptyConfig) -> io::Result<Pty> {
        Ok(Pty { inner: imp::PtyImpl::spawn(opts, config)? })
    }

    /// Hands out the child's output side once. Read it on its own thread; it returns EOF or an error when the pty closes.
    pub fn take_reader(&mut self) -> Option<Box<dyn Read + Send>> {
        self.inner.take_reader()
    }

    /// Hands out the child's input side once. Bytes written arrive as the child's keyboard input.
    pub fn take_writer(&mut self) -> Option<Box<dyn Write + Send>> {
        self.inner.take_writer()
    }

    /// Tells the child its terminal is now `size` (SIGWINCH on Unix, ResizePseudoConsole on Windows).
    pub fn resize(&self, size: PtySize) -> io::Result<()> {
        self.inner.resize(size)
    }

    /// Returns the exit status if the child has ended, without blocking.
    pub fn try_wait(&mut self) -> io::Result<Option<ExitStatus>> {
        self.inner.try_wait()
    }

    /// Blocks until the child ends and returns its exit status.
    pub fn wait(&mut self) -> io::Result<ExitStatus> {
        self.inner.wait()
    }

    /// Ends the child and everything it started (job object on Windows, process group on Unix).
    pub fn kill(&mut self) -> io::Result<()> {
        self.inner.kill()
    }

    /// The operating system process id of the child.
    pub fn pid(&self) -> u32 {
        self.inner.pid()
    }

    /// Which pseudoconsole implementation is in use: bundled or inbox on Windows, not applicable elsewhere.
    pub fn conpty_source(&self) -> ConptySource {
        self.inner.conpty_source()
    }
}
