//! Windows backend: a pseudoconsole, a child created into a kill on close job object.

use crate::conpty_api::{ConptyApi, PSEUDOCONSOLE_GLYPH_WIDTH_GRAPHEMES, load_api};
use crate::{ConptyConfig, ConptySource, ExitStatus, PtySize, SpawnOptions};
use std::collections::BTreeMap;
use std::ffi::{OsStr, OsString, c_void};
use std::fs::File;
use std::io::{self, Read, Write};
use std::os::windows::ffi::OsStrExt;
use std::os::windows::io::{AsRawHandle, FromRawHandle, OwnedHandle};
use windows::Win32::Foundation::{CloseHandle, HANDLE, INVALID_HANDLE_VALUE, WAIT_OBJECT_0};
use windows::Win32::System::Console::{COORD, HPCON};
use windows::Win32::System::JobObjects::{
    CreateJobObjectW, JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE, JOBOBJECT_EXTENDED_LIMIT_INFORMATION,
    JobObjectExtendedLimitInformation, SetInformationJobObject, TerminateJobObject,
};
use windows::Win32::System::Pipes::CreatePipe;
use windows::Win32::System::Threading::{
    CREATE_UNICODE_ENVIRONMENT, CreateProcessW, DeleteProcThreadAttributeList, EXTENDED_STARTUPINFO_PRESENT,
    GetExitCodeProcess, INFINITE, InitializeProcThreadAttributeList, LPPROC_THREAD_ATTRIBUTE_LIST,
    PROC_THREAD_ATTRIBUTE_JOB_LIST, PROC_THREAD_ATTRIBUTE_PSEUDOCONSOLE, PROCESS_INFORMATION, STARTF_USESTDHANDLES,
    STARTUPINFOEXW, STARTUPINFOW, TerminateProcess, UpdateProcThreadAttribute, WaitForSingleObject,
};
use windows::core::{PCWSTR, PWSTR};

/// The live state of one Windows pseudoterminal.
pub struct PtyImpl {
    api: ConptyApi,
    hpc: HPCON,
    process: OwnedHandle,
    job: OwnedHandle,
    pid: u32,
    reader: Option<File>,
    writer: Option<File>,
}

/// A pseudoconsole handle that can move to the thread that closes it.
struct SendHpc(HPCON);
// SAFETY: a pseudoconsole handle is a process wide handle value, usable from any thread.
unsafe impl Send for SendHpc {}

impl PtyImpl {
    /// Creates the pipes and pseudoconsole, then starts the child inside the pseudoconsole and a job object.
    pub fn spawn(opts: &SpawnOptions, config: &ConptyConfig) -> io::Result<PtyImpl> {
        let api = load_api(&config.choice)?;
        let (in_read, in_write) = make_pipe()?;
        let (out_read, out_write) = make_pipe()?;
        let hpc = create_console(&api, opts.size, &in_read, &out_write, config.flags)?;
        // ConPTY duplicated the console side ends; our copies must close or the pipes never report EOF.
        drop(in_read);
        drop(out_write);
        let job = create_kill_on_close_job()?;
        let (process, pid) = match start_child(opts, hpc, &job) {
            Ok(started) => started,
            Err(err) => {
                close_console_on_thread(api, hpc);
                return Err(err);
            }
        };
        Ok(PtyImpl {
            api,
            hpc,
            process,
            job,
            pid,
            reader: Some(File::from(out_read)),
            writer: Some(File::from(in_write)),
        })
    }

    /// Gives the output pipe to the caller once.
    pub fn take_reader(&mut self) -> Option<Box<dyn Read + Send>> {
        self.reader.take().map(|file| Box::new(file) as Box<dyn Read + Send>)
    }

    /// Gives the input pipe to the caller once.
    pub fn take_writer(&mut self) -> Option<Box<dyn Write + Send>> {
        self.writer.take().map(|file| Box::new(file) as Box<dyn Write + Send>)
    }

    /// Resizes the pseudoconsole.
    pub fn resize(&self, size: PtySize) -> io::Result<()> {
        // SAFETY: `self.hpc` is a live pseudoconsole.
        unsafe { (self.api.resize)(self.hpc, coord(size)) }.ok().map_err(io::Error::from)
    }

    /// Reports the exit status when the child process has signalled.
    pub fn try_wait(&mut self) -> io::Result<Option<ExitStatus>> {
        // SAFETY: the process handle is owned and open.
        let state = unsafe { WaitForSingleObject(handle_of(&self.process), 0) };
        if state == WAIT_OBJECT_0 { self.exit_status().map(Some) } else { Ok(None) }
    }

    /// Waits for the child process to end.
    pub fn wait(&mut self) -> io::Result<ExitStatus> {
        // SAFETY: the process handle is owned and open.
        unsafe { WaitForSingleObject(handle_of(&self.process), INFINITE) };
        self.exit_status()
    }

    /// Terminates every process in the job, which is the child and its descendants, and the child
    /// itself in case it could not join the job (a Store app in its package's job).
    pub fn kill(&mut self) -> io::Result<()> {
        // SAFETY: the job and process handles are owned and open.
        let job = unsafe { TerminateJobObject(handle_of(&self.job), 1) };
        // SAFETY: as above; terminating a process that already ended fails harmlessly.
        let _ = unsafe { TerminateProcess(handle_of(&self.process), 1) };
        job.map_err(io::Error::from)
    }

    /// The child's process id.
    pub fn pid(&self) -> u32 {
        self.pid
    }

    /// Bundled or inbox, as loaded at spawn time.
    pub fn conpty_source(&self) -> ConptySource {
        self.api.source
    }

    /// Reads the exit code of a process that has ended.
    fn exit_status(&self) -> io::Result<ExitStatus> {
        let mut code = 0u32;
        // SAFETY: the process handle is owned and `code` is a valid out pointer.
        unsafe { GetExitCodeProcess(handle_of(&self.process), &mut code) }.map_err(io::Error::from)?;
        Ok(ExitStatus::Code(code as i32))
    }
}

impl Drop for PtyImpl {
    /// Ends the tree, releases our pipe ends, and closes the pseudoconsole on its own thread because the close can block on a pipe nobody reads.
    fn drop(&mut self) {
        // SAFETY: the job handle is owned and open.
        unsafe { TerminateJobObject(handle_of(&self.job), 1).ok() };
        self.reader.take();
        self.writer.take();
        close_console_on_thread(self.api, self.hpc);
    }
}

/// Closes a pseudoconsole on a detached thread; ClosePseudoConsole can block until its output pipe is drained.
fn close_console_on_thread(api: ConptyApi, hpc: HPCON) {
    let (close, hpc) = (api.close, SendHpc(hpc));
    std::thread::spawn(move || {
        let hpc = hpc;
        // SAFETY: the pseudoconsole is closed exactly once, here.
        unsafe { close(hpc.0) };
    });
}

/// The raw handle of an owned handle.
fn handle_of(handle: &OwnedHandle) -> HANDLE {
    HANDLE(handle.as_raw_handle())
}

/// Converts a terminal size into the console API's coordinate pair.
fn coord(size: PtySize) -> COORD {
    COORD { X: size.cols as i16, Y: size.rows as i16 }
}

/// Creates an anonymous pipe whose ends are not inheritable and returns (read end, write end).
fn make_pipe() -> io::Result<(OwnedHandle, OwnedHandle)> {
    let (mut read, mut write) = (HANDLE::default(), HANDLE::default());
    // SAFETY: both out pointers are valid; null attributes make the handles non inheritable.
    unsafe { CreatePipe(&mut read, &mut write, None, 0) }.map_err(io::Error::from)?;
    // SAFETY: CreatePipe returned two open handles that nothing else owns.
    unsafe { Ok((OwnedHandle::from_raw_handle(read.0), OwnedHandle::from_raw_handle(write.0))) }
}

/// Creates the pseudoconsole, retrying without the grapheme flag when the implementation rejects it.
fn create_console(
    api: &ConptyApi, size: PtySize, input: &OwnedHandle, output: &OwnedHandle, flags: Option<u32>,
) -> io::Result<HPCON> {
    let default_flags = if api.source == ConptySource::Bundled { PSEUDOCONSOLE_GLYPH_WIDTH_GRAPHEMES } else { 0 };
    let first = flags.unwrap_or(default_flags);
    let attempt = |flags: u32| {
        let mut hpc = HPCON(0);
        // SAFETY: both handles are open pipe ends and `hpc` is a valid out pointer.
        unsafe { (api.create)(coord(size), handle_of(input), handle_of(output), flags, &mut hpc) }.ok().map(|()| hpc)
    };
    match attempt(first) {
        Ok(hpc) => Ok(hpc),
        Err(_) if flags.is_none() && first != 0 => attempt(0).map_err(io::Error::from),
        Err(err) => Err(io::Error::from(err)),
    }
}

/// Creates a job object that kills every process in it when its last handle closes.
fn create_kill_on_close_job() -> io::Result<OwnedHandle> {
    // SAFETY: null attributes and name create an anonymous job.
    let job = unsafe { CreateJobObjectW(None, PCWSTR::null()) }.map_err(io::Error::from)?;
    // SAFETY: CreateJobObjectW returned an open handle that nothing else owns.
    let owned = unsafe { OwnedHandle::from_raw_handle(job.0) };
    let mut info = JOBOBJECT_EXTENDED_LIMIT_INFORMATION::default();
    info.BasicLimitInformation.LimitFlags = JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;
    let size = std::mem::size_of::<JOBOBJECT_EXTENDED_LIMIT_INFORMATION>() as u32;
    // SAFETY: `info` is a valid structure of the class and size named.
    unsafe {
        SetInformationJobObject(job, JobObjectExtendedLimitInformation, (&raw const info).cast::<c_void>(), size)
    }
    .map_err(io::Error::from)?;
    Ok(owned)
}

/// A process thread attribute list holding the pseudoconsole and job list attributes.
struct AttributeList {
    buffer: Vec<u64>,
    job_handles: Box<[HANDLE; 1]>,
}

impl AttributeList {
    /// Builds the list that attaches a new process to `hpc` and, when `job` is given, puts it in the
    /// job from its first instruction.
    fn new(hpc: HPCON, job: Option<&OwnedHandle>) -> io::Result<AttributeList> {
        let count = if job.is_some() { 2 } else { 1 };
        let mut size = 0usize;
        // SAFETY: a null list with a size out pointer asks for the required size and fails by design.
        unsafe { InitializeProcThreadAttributeList(None, count, None, &mut size).ok() };
        let handle = job.map_or(HANDLE::default(), handle_of);
        let mut list = AttributeList { buffer: vec![0u64; size.div_ceil(8)], job_handles: Box::new([handle]) };
        // SAFETY: the buffer is at least `size` bytes and 8 byte aligned.
        unsafe { InitializeProcThreadAttributeList(Some(list.pointer()), count, None, &mut size) }
            .map_err(io::Error::from)?;
        // SAFETY: the pseudoconsole attribute value is the HPCON itself, passed by value.
        unsafe {
            UpdateProcThreadAttribute(
                list.pointer(),
                0,
                PROC_THREAD_ATTRIBUTE_PSEUDOCONSOLE as usize,
                Some(hpc.0 as *const c_void),
                std::mem::size_of::<HPCON>(),
                None,
                None,
            )
        }
        .map_err(io::Error::from)?;
        if job.is_none() {
            return Ok(list);
        }
        let jobs = list.job_handles.as_ptr().cast::<c_void>();
        // SAFETY: the job list value points to one handle that `list` keeps alive.
        unsafe {
            UpdateProcThreadAttribute(
                list.pointer(),
                0,
                PROC_THREAD_ATTRIBUTE_JOB_LIST as usize,
                Some(jobs),
                std::mem::size_of::<HANDLE>(),
                None,
                None,
            )
        }
        .map_err(io::Error::from)?;
        Ok(list)
    }

    /// The list as the pointer type the Win32 functions take.
    fn pointer(&mut self) -> LPPROC_THREAD_ATTRIBUTE_LIST {
        LPPROC_THREAD_ATTRIBUTE_LIST(self.buffer.as_mut_ptr().cast::<c_void>())
    }
}

impl Drop for AttributeList {
    /// Releases the attribute list.
    fn drop(&mut self) {
        // SAFETY: the list was initialised in `new` and is deleted once.
        unsafe { DeleteProcThreadAttributeList(self.pointer()) };
    }
}

/// Starts the child attached to `hpc` and inside `job`, returning its process handle and id.
///
/// A Microsoft Store app (pwsh installed from the Store) runs inside a job the system makes for its
/// package. While one instance is alive, a second one created with our job in its attribute list is
/// refused with access denied. In that case the child is created suspended without the job list,
/// added to the job afterwards (best effort: a package job may not allow it, and then closing the
/// pseudoconsole still ends the shell), and resumed.
fn start_child(opts: &SpawnOptions, hpc: HPCON, job: &OwnedHandle) -> io::Result<(OwnedHandle, u32)> {
    // A packaged app (pwsh from the Microsoft Store) runs in a job Windows makes for its package.
    // Started inside this session's job, the package's job would sit under it, every later
    // instance of the package (in other sessions too) would join it, and ending this session would
    // end them all. So a packaged app starts outside the session's job; closing the pseudoconsole
    // still ends the console processes attached to it, and `kill` ends the process itself.
    let job = if is_packaged_app(&opts.program) { None } else { Some(job) };
    let info = match create_process(opts, hpc, job) {
        Ok(info) => info,
        // The program is already in a job hierarchy the session's job cannot join.
        Err(err) if job.is_some() && is_access_denied(&err) => create_process(opts, hpc, None)?,
        Err(err) => return Err(err),
    };
    // SAFETY: CreateProcessW returned two open handles that nothing else owns; the thread handle is not needed.
    unsafe { CloseHandle(info.hThread).ok() };
    // SAFETY: as above, the process handle becomes owned here.
    Ok((unsafe { OwnedHandle::from_raw_handle(info.hProcess.0) }, info.dwProcessId))
}

/// True when `program` resolves to a packaged app: an app execution alias or a file in a package
/// folder, both inside a folder named `WindowsApps`.
pub(crate) fn is_packaged_app(program: &str) -> bool {
    resolve_program(program).is_some_and(|p| p.components().any(|c| c.as_os_str().eq_ignore_ascii_case("WindowsApps")))
}

/// The file `program` names: itself when it has a folder in it, else the first match on PATH,
/// trying `.exe` when it has no extension. An app execution alias is a reparse point that does not
/// open as a file, so a match is any directory entry.
fn resolve_program(program: &str) -> Option<std::path::PathBuf> {
    let path = std::path::Path::new(program);
    if path.is_absolute() || path.components().count() > 1 {
        return Some(path.to_path_buf());
    }
    let names: Vec<String> =
        if path.extension().is_some() { vec![program.into()] } else { vec![program.into(), format!("{program}.exe")] };
    let dirs = std::env::var_os("PATH")?;
    std::env::split_paths(&dirs)
        .find_map(|dir| names.iter().map(|n| dir.join(n)).find(|c| std::fs::symlink_metadata(c).is_ok()))
}

/// True for the access denied error in its Win32, HRESULT and NTSTATUS forms.
fn is_access_denied(err: &io::Error) -> bool {
    matches!(err.raw_os_error(), Some(5) | Some(-2147024891) | Some(-1073283067))
}

/// Calls CreateProcessW with the pseudoconsole and, when given, the job in the attribute list.
fn create_process(opts: &SpawnOptions, hpc: HPCON, job: Option<&OwnedHandle>) -> io::Result<PROCESS_INFORMATION> {
    let mut command_line = to_wide(build_command_line(&opts.program, &opts.args));
    let environment = build_environment_block(&opts.env);
    let cwd = opts.cwd.as_ref().map(|dir| to_wide(dir.as_os_str()));
    let mut attributes = AttributeList::new(hpc, job)?;
    let mut startup = STARTUPINFOEXW::default();
    startup.StartupInfo.cb = std::mem::size_of::<STARTUPINFOEXW>() as u32;
    startup.lpAttributeList = attributes.pointer();
    // Without explicit invalid standard handles a child inherits this process's redirected stdio instead of the pseudoconsole.
    startup.StartupInfo.dwFlags = STARTF_USESTDHANDLES;
    startup.StartupInfo.hStdInput = INVALID_HANDLE_VALUE;
    startup.StartupInfo.hStdOutput = INVALID_HANDLE_VALUE;
    startup.StartupInfo.hStdError = INVALID_HANDLE_VALUE;
    let mut info = PROCESS_INFORMATION::default();
    let cwd_ptr = cwd.as_ref().map_or(PCWSTR::null(), |wide| PCWSTR(wide.as_ptr()));
    let flags = EXTENDED_STARTUPINFO_PRESENT | CREATE_UNICODE_ENVIRONMENT;
    let startup_ptr = (&raw const startup).cast::<STARTUPINFOW>();
    let env_ptr = Some(environment.as_ptr().cast::<c_void>());
    // SAFETY: every pointer refers to a buffer that lives until the call returns; the command line is mutable and nul terminated.
    unsafe {
        CreateProcessW(
            PCWSTR::null(),
            Some(PWSTR(command_line.as_mut_ptr())),
            None,
            None,
            false,
            flags,
            env_ptr,
            cwd_ptr,
            startup_ptr,
            &mut info,
        )
    }
    .map_err(io::Error::from)?;
    Ok(info)
}

/// Encodes a string as nul terminated UTF-16.
fn to_wide(text: impl AsRef<OsStr>) -> Vec<u16> {
    text.as_ref().encode_wide().chain(Some(0)).collect()
}

/// Joins a program and arguments into one command line using the quoting rules the MSVC runtime parses.
fn build_command_line(program: &str, args: &[String]) -> OsString {
    let mut line = String::new();
    line.push_str(&quote_argument(program, true));
    for arg in args {
        line.push(' ');
        line.push_str(&quote_argument(arg, false));
    }
    OsString::from(line)
}

/// Quotes one argument so that CommandLineToArgvW returns it unchanged.
fn quote_argument(arg: &str, always_quote: bool) -> String {
    let plain = !arg.is_empty() && !arg.contains([' ', '\t', '\n', '\x0b', '"']);
    if plain && !always_quote {
        return arg.to_string();
    }
    let mut quoted = String::from("\"");
    let mut backslashes = 0usize;
    for ch in arg.chars() {
        match ch {
            '\\' => backslashes += 1,
            '"' => {
                quoted.push_str(&"\\".repeat(backslashes * 2 + 1));
                quoted.push('"');
                backslashes = 0;
            }
            _ => {
                quoted.push_str(&"\\".repeat(backslashes));
                quoted.push(ch);
                backslashes = 0;
            }
        }
    }
    quoted.push_str(&"\\".repeat(backslashes * 2));
    quoted.push('"');
    quoted
}

/// Builds the UTF-16 environment block: the inherited environment with `extra` applied, sorted case insensitively as Windows requires.
fn build_environment_block(extra: &[(String, String)]) -> Vec<u16> {
    let mut merged: BTreeMap<String, (OsString, OsString)> = BTreeMap::new();
    let extra = extra.iter().map(|(n, v)| (OsString::from(n), OsString::from(v)));
    for (name, value) in std::env::vars_os().chain(extra) {
        merged.insert(name.to_string_lossy().to_uppercase(), (name, value));
    }
    let mut block: Vec<u16> = Vec::new();
    for (name, value) in merged.values() {
        block.extend(name.encode_wide());
        block.push(u16::from(b'='));
        block.extend(value.encode_wide());
        block.push(0);
    }
    block.push(0);
    block
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::windows::ffi::OsStringExt;

    /// A later entry replaces an inherited variable regardless of case.
    #[test]
    fn environment_block_applies_overrides() {
        let block = build_environment_block(&[("term_program".to_string(), "termoil".to_string())]);
        let entries: Vec<String> = block
            .split(|unit| *unit == 0)
            .filter(|e| !e.is_empty())
            .map(|e| OsString::from_wide(e).to_string_lossy().into_owned())
            .collect();
        let matches: Vec<_> = entries.iter().filter(|e| e.to_ascii_uppercase().starts_with("TERM_PROGRAM=")).collect();
        assert_eq!(matches.len(), 1);
        assert!(matches[0].ends_with("=termoil"));
    }

    /// Arguments with spaces, quotes and trailing backslashes are quoted the way CommandLineToArgvW reads them.
    #[test]
    fn quoting_matches_msvc_rules() {
        assert_eq!(quote_argument("plain", false), "plain");
        assert_eq!(quote_argument("a b", false), "\"a b\"");
        assert_eq!(quote_argument("say \"hi\"", false), "\"say \\\"hi\\\"\"");
        assert_eq!(quote_argument("dir\\", false), "dir\\");
        assert_eq!(quote_argument("my dir\\", false), "\"my dir\\\\\"");
    }
}
