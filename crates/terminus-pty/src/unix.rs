//! Unix backend: openpty, a child with its own session and controlling terminal, SIGHUP on drop.

use crate::{ConptyConfig, ConptySource, ExitStatus, PtySize, SpawnOptions};
use rustix::fd::{AsFd, BorrowedFd, OwnedFd};
use rustix::process::{Pid, Signal, ioctl_tiocsctty, kill_process_group, setsid};
use rustix::termios::{Winsize, tcsetwinsize};
use std::fs::File;
use std::io::{self, Read, Write};
use std::os::unix::process::{CommandExt, ExitStatusExt};
use std::process::{Child, Command, Stdio};

/// The live state of one Unix pseudoterminal.
pub struct PtyImpl {
    controller: OwnedFd,
    child: Child,
    reader: Option<File>,
    writer: Option<File>,
}

impl PtyImpl {
    /// Opens a pty, starts the child on its user side, and keeps the controller side for I/O and resizing.
    pub fn spawn(opts: &SpawnOptions, _config: &ConptyConfig) -> io::Result<PtyImpl> {
        let pty = rustix_openpty::openpty(None, Some(&winsize(opts.size)))?;
        let mut command = build_command(opts);
        command.stdin(Stdio::from(pty.user.try_clone()?));
        command.stdout(Stdio::from(pty.user.try_clone()?));
        command.stderr(Stdio::from(pty.user.try_clone()?));
        // SAFETY: the closure runs between fork and exec and only calls async signal safe system calls.
        unsafe { command.pre_exec(become_session_leader) };
        let child = command.spawn()?;
        drop(command);
        drop(pty.user);
        let reader = File::from(pty.controller.try_clone()?);
        let writer = File::from(pty.controller.try_clone()?);
        Ok(PtyImpl { controller: pty.controller, child, reader: Some(reader), writer: Some(writer) })
    }

    /// Gives the output side to the caller once.
    pub fn take_reader(&mut self) -> Option<Box<dyn Read + Send>> {
        self.reader.take().map(|file| Box::new(file) as Box<dyn Read + Send>)
    }

    /// Gives the input side to the caller once.
    pub fn take_writer(&mut self) -> Option<Box<dyn Write + Send>> {
        self.writer.take().map(|file| Box::new(file) as Box<dyn Write + Send>)
    }

    /// Sets the window size on the pty, which makes the kernel send SIGWINCH to the foreground group.
    pub fn resize(&self, size: PtySize) -> io::Result<()> {
        tcsetwinsize(self.controller.as_fd(), winsize(size))?;
        Ok(())
    }

    /// Returns the exit status if the child has ended.
    pub fn try_wait(&mut self) -> io::Result<Option<ExitStatus>> {
        Ok(self.child.try_wait()?.map(typed_status))
    }

    /// Blocks until the child ends.
    pub fn wait(&mut self) -> io::Result<ExitStatus> {
        Ok(typed_status(self.child.wait()?))
    }

    /// Sends SIGKILL to the child's process group, which holds the child and the jobs it started.
    pub fn kill(&mut self) -> io::Result<()> {
        signal_group(self.child.id(), Signal::KILL)
    }

    /// The child's process id.
    pub fn pid(&self) -> u32 {
        self.child.id()
    }

    /// Always `NotApplicable` on Unix.
    pub fn conpty_source(&self) -> ConptySource {
        ConptySource::NotApplicable
    }
}

impl Drop for PtyImpl {
    /// Hangs up the child's process group, then reaps the child if it has already ended.
    fn drop(&mut self) {
        signal_group(self.child.id(), Signal::HUP).ok();
        self.child.try_wait().ok();
    }
}

/// Sends `signal` to the process group whose id is `pgid`.
fn signal_group(pgid: u32, signal: Signal) -> io::Result<()> {
    let pid = Pid::from_raw(pgid as i32).ok_or_else(|| io::Error::other("invalid process id"))?;
    kill_process_group(pid, signal)?;
    Ok(())
}

/// Converts the standard exit status into the typed code or signal.
fn typed_status(status: std::process::ExitStatus) -> ExitStatus {
    match (status.code(), status.signal()) {
        (Some(code), _) => ExitStatus::Code(code),
        (None, Some(signal)) => ExitStatus::Signal(signal),
        (None, None) => ExitStatus::Code(-1),
    }
}

/// Converts a terminal size into the kernel's window size structure.
fn winsize(size: PtySize) -> Winsize {
    Winsize { ws_row: size.rows, ws_col: size.cols, ws_xpixel: size.pixel_width, ws_ypixel: size.pixel_height }
}

/// Runs in the child before exec: starts a new session and makes the pty on stdin its controlling terminal.
fn become_session_leader() -> io::Result<()> {
    setsid()?;
    // SAFETY: file descriptor 0 is the pty user side that Command installed as stdin.
    let stdin = unsafe { BorrowedFd::borrow_raw(0) };
    ioctl_tiocsctty(stdin)?;
    Ok(())
}

/// Builds the command with program, arguments, cwd and environment, including the macOS and Linux login shell forms.
fn build_command(opts: &SpawnOptions) -> Command {
    let mut command =
        if opts.login && cfg!(target_os = "macos") { macos_login_command(opts) } else { plain_command(opts) };
    if opts.login && cfg!(not(target_os = "macos")) {
        command.arg0(format!("-{}", program_basename(&opts.program)));
    }
    if let Some(cwd) = &opts.cwd {
        command.current_dir(cwd);
    }
    for (name, value) in &opts.env {
        command.env(name, value);
    }
    command
}

/// The program with its arguments, started directly.
fn plain_command(opts: &SpawnOptions) -> Command {
    let mut command = Command::new(&opts.program);
    command.args(&opts.args);
    command
}

/// `/usr/bin/login -fp[q] $USER program args`, quiet when ~/.hushlogin exists.
fn macos_login_command(opts: &SpawnOptions) -> Command {
    let user = std::env::var("USER").unwrap_or_default();
    let hushed = std::env::var_os("HOME").is_some_and(|home| std::path::Path::new(&home).join(".hushlogin").exists());
    let mut command = Command::new("/usr/bin/login");
    command.arg(if hushed { "-fpq" } else { "-fp" }).arg(user).arg(&opts.program).args(&opts.args);
    command
}

/// The file name part of a program path, used for the leading dash login argv[0].
fn program_basename(program: &str) -> &str {
    program.rsplit('/').next().unwrap_or(program)
}
