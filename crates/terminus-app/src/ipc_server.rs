//! The IPC server: a named pipe on Windows and a Unix socket elsewhere, readable and writable by
//! the current user only (`docs/TDD.md` section 4.10). Each connection gets a thread that reads one
//! JSON request line at a time, hands it to the handler and writes the answer. There is no network
//! listener.

use std::io::{BufRead, BufReader, Write};
use std::sync::Arc;

use terminus_protocol::{Response, decode_request, encode_response};

/// Answers one request. Called on the connection's thread.
pub type Handler = Arc<dyn Fn(terminus_protocol::Request) -> Response + Send + Sync>;

/// Why the server could not start.
#[derive(Debug)]
pub enum ServeError {
    /// Another instance owns the socket.
    AlreadyRunning,
    /// Something else went wrong.
    Io(std::io::Error),
}

/// Serves requests from one connection until it closes.
fn serve_connection(reader: impl std::io::Read, mut writer: impl Write, handler: &Handler) {
    let mut reader = BufReader::new(reader);
    let mut line = String::new();
    loop {
        line.clear();
        match reader.read_line(&mut line) {
            Ok(0) | Err(_) => return,
            Ok(_) => {}
        }
        if line.trim().is_empty() {
            continue;
        }
        let response = match decode_request(&line) {
            Ok(req) => handler(req),
            Err(e) => {
                Response { id: 0, ok: false, result: serde_json::Value::Null, error: Some(format!("bad request: {e}")) }
            }
        };
        if writer.write_all(encode_response(&response).as_bytes()).and_then(|_| writer.flush()).is_err() {
            return;
        }
    }
}

/// A handler that passes each request to the thread that owns the `App` through `send`, waits for
/// the outcome, and for `wait-prompt` blocks on this connection's thread rather than the app's.
/// `send` returns false when the app is shutting down.
pub fn bridge(
    send: impl Fn(terminus_protocol::ActionCall, crossbeam_channel::Sender<crate::app::Outcome>) -> bool
    + Send
    + Sync
    + 'static,
) -> Handler {
    Arc::new(move |req: terminus_protocol::Request| {
        let (tx, rx) = crossbeam_channel::bounded(1);
        if !send(req.call, tx) {
            return Response::from_result(
                req.id,
                Err(terminus_protocol::ActionError::Failed("Terminus is shutting down".into())),
            );
        }
        let result = match rx.recv() {
            Ok(crate::app::Outcome::Done(r)) => r,
            Ok(crate::app::Outcome::Wait(waiter, timeout)) => {
                waiter.wait(timeout).map(|code| serde_json::json!({ "exit_code": code })).map_err(|e| {
                    if e == "timed out" {
                        terminus_protocol::ActionError::Timeout
                    } else {
                        terminus_protocol::ActionError::Failed(e)
                    }
                })
            }
            Err(_) => Err(terminus_protocol::ActionError::Failed("Terminus is shutting down".into())),
        };
        Response::from_result(req.id, result)
    })
}

/// The DACL of the socket as SDDL (Windows) or its mode bits as octal (Unix), for the test that
/// checks only the owner can connect.
pub fn describe_access(name: &str) -> std::io::Result<String> {
    imp::describe_access(name)
}

/// Starts the server on `name` with an accept thread. Fails with `AlreadyRunning` when another
/// instance has the name.
pub fn start(name: &str, handler: Handler) -> Result<(), ServeError> {
    imp::start(name, handler)
}

#[cfg(windows)]
mod imp {
    use std::fs::File;
    use std::os::windows::io::{FromRawHandle, RawHandle};

    use windows::Win32::Foundation::{
        ERROR_ACCESS_DENIED, ERROR_PIPE_BUSY, ERROR_PIPE_CONNECTED, HANDLE, HLOCAL, INVALID_HANDLE_VALUE, LocalFree,
    };
    use windows::Win32::Security::Authorization::{
        ConvertSidToStringSidW, ConvertStringSecurityDescriptorToSecurityDescriptorW, SDDL_REVISION_1,
    };
    use windows::Win32::Security::{
        GetTokenInformation, PSECURITY_DESCRIPTOR, SECURITY_ATTRIBUTES, TOKEN_QUERY, TOKEN_USER, TokenUser,
    };
    use windows::Win32::Storage::FileSystem::{
        FILE_FLAG_FIRST_PIPE_INSTANCE, FILE_FLAGS_AND_ATTRIBUTES, PIPE_ACCESS_DUPLEX,
    };
    use windows::Win32::System::Pipes::{
        ConnectNamedPipe, CreateNamedPipeW, PIPE_READMODE_BYTE, PIPE_REJECT_REMOTE_CLIENTS, PIPE_TYPE_BYTE,
        PIPE_UNLIMITED_INSTANCES, PIPE_WAIT,
    };
    use windows::Win32::System::Threading::{GetCurrentProcess, OpenProcessToken};
    use windows::core::{HSTRING, PWSTR};

    use super::{Handler, ServeError, serve_connection};

    /// A security descriptor that lives as long as the server; freed never, since the server lives
    /// as long as the process.
    struct Sd(PSECURITY_DESCRIPTOR);
    // SAFETY: the descriptor is immutable after creation and only read by CreateNamedPipeW.
    unsafe impl Send for Sd {}

    /// The current user's SID as a string such as `S-1-5-21-...`.
    fn user_sid() -> std::io::Result<String> {
        // SAFETY: standard token query; the buffer is sized by the first call and outlives the SID
        // pointer read from it, and the string from ConvertSidToStringSidW is freed with LocalFree.
        unsafe {
            let mut token = HANDLE::default();
            OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &mut token)?;
            let mut len = 0u32;
            let _ = GetTokenInformation(token, TokenUser, None, 0, &mut len);
            let mut buf = vec![0u8; len as usize];
            GetTokenInformation(token, TokenUser, Some(buf.as_mut_ptr().cast()), len, &mut len)?;
            let user = &*(buf.as_ptr() as *const TOKEN_USER);
            let mut s = PWSTR::null();
            ConvertSidToStringSidW(user.User.Sid, &mut s)?;
            let out = s.to_string().unwrap_or_default();
            let _ = LocalFree(Some(HLOCAL(s.0.cast())));
            let _ = windows::Win32::Foundation::CloseHandle(token);
            Ok(out)
        }
    }

    /// A descriptor whose DACL grants the current user full access and nobody else anything.
    fn owner_only() -> std::io::Result<Sd> {
        let sddl = HSTRING::from(format!("D:P(A;;GA;;;{})", user_sid()?));
        let mut sd = PSECURITY_DESCRIPTOR::default();
        // SAFETY: the SDDL string is valid for the call; the descriptor is allocated by the system.
        unsafe { ConvertStringSecurityDescriptorToSecurityDescriptorW(&sddl, SDDL_REVISION_1, &mut sd, None)? };
        Ok(Sd(sd))
    }

    /// Creates one instance of the pipe.
    fn create(name: &HSTRING, sd: &Sd, first: bool) -> std::io::Result<HANDLE> {
        let sa = SECURITY_ATTRIBUTES {
            nLength: size_of::<SECURITY_ATTRIBUTES>() as u32,
            lpSecurityDescriptor: sd.0.0,
            bInheritHandle: false.into(),
        };
        let flags = if first { PIPE_ACCESS_DUPLEX | FILE_FLAG_FIRST_PIPE_INSTANCE } else { PIPE_ACCESS_DUPLEX };
        // SAFETY: the name and attributes are valid for the call.
        let h = unsafe {
            CreateNamedPipeW(
                name,
                FILE_FLAGS_AND_ATTRIBUTES(flags.0),
                PIPE_TYPE_BYTE | PIPE_READMODE_BYTE | PIPE_WAIT | PIPE_REJECT_REMOTE_CLIENTS,
                PIPE_UNLIMITED_INSTANCES,
                65536,
                65536,
                0,
                Some(&sa),
            )
        };
        if h == INVALID_HANDLE_VALUE { Err(std::io::Error::last_os_error()) } else { Ok(h) }
    }

    /// Opens the pipe and returns its DACL in SDDL form.
    pub fn describe_access(name: &str) -> std::io::Result<String> {
        use std::os::windows::io::AsRawHandle;
        use windows::Win32::Security::Authorization::{
            ConvertSecurityDescriptorToStringSecurityDescriptorW, GetSecurityInfo, SE_KERNEL_OBJECT,
        };
        use windows::Win32::Security::{DACL_SECURITY_INFORMATION, PSECURITY_DESCRIPTOR};
        let file = std::fs::OpenOptions::new().read(true).write(true).open(name)?;
        let mut sd = PSECURITY_DESCRIPTOR::default();
        // SAFETY: the handle is open for the call; the descriptor and string are allocated by the
        // system and freed with LocalFree.
        unsafe {
            let err = GetSecurityInfo(
                HANDLE(file.as_raw_handle()),
                SE_KERNEL_OBJECT,
                DACL_SECURITY_INFORMATION,
                None,
                None,
                None,
                None,
                Some(&mut sd),
            );
            if err.is_err() {
                return Err(std::io::Error::from_raw_os_error(err.0 as i32));
            }
            let mut s = PWSTR::null();
            ConvertSecurityDescriptorToStringSecurityDescriptorW(
                sd,
                SDDL_REVISION_1,
                DACL_SECURITY_INFORMATION,
                &mut s,
                None,
            )?;
            let out = s.to_string().unwrap_or_default();
            let _ = LocalFree(Some(HLOCAL(s.0.cast())));
            let _ = LocalFree(Some(HLOCAL(sd.0)));
            Ok(out)
        }
    }

    /// Creates the first pipe instance (proving no other instance runs) and starts accepting.
    pub fn start(name: &str, handler: Handler) -> Result<(), ServeError> {
        let wname = HSTRING::from(name);
        let sd = owner_only().map_err(ServeError::Io)?;
        let first = match create(&wname, &sd, true) {
            Ok(h) => h,
            Err(e)
                if e.raw_os_error() == Some(ERROR_ACCESS_DENIED.0 as i32)
                    || e.raw_os_error() == Some(ERROR_PIPE_BUSY.0 as i32) =>
            {
                return Err(ServeError::AlreadyRunning);
            }
            Err(e) => return Err(ServeError::Io(e)),
        };
        let first = first.0 as usize;
        std::thread::Builder::new()
            .name("terminus ipc accept".into())
            .spawn(move || accept_loop(wname, sd, HANDLE(first as *mut _), handler))
            .map_err(ServeError::Io)?;
        Ok(())
    }

    /// Waits for a client on the pending instance, hands it to a thread, and creates the next one.
    fn accept_loop(name: HSTRING, sd: Sd, mut pending: HANDLE, handler: Handler) {
        loop {
            // SAFETY: `pending` is a pipe instance this thread created.
            let connected = unsafe { ConnectNamedPipe(pending, None) };
            let ok = connected.is_ok()
                || connected.as_ref().err().is_some_and(|e| e.code() == ERROR_PIPE_CONNECTED.to_hresult());
            let handle = pending.0 as usize;
            match create(&name, &sd, false) {
                Ok(h) => pending = h,
                Err(_) => return,
            }
            if !ok {
                // SAFETY: the failed instance is ours to close.
                unsafe { drop(File::from_raw_handle(handle as RawHandle)) };
                continue;
            }
            let handler = handler.clone();
            let _ = std::thread::Builder::new().name("terminus ipc connection".into()).spawn(move || {
                // SAFETY: the connected instance is owned by this thread from here on.
                let file = unsafe { File::from_raw_handle(handle as RawHandle) };
                if let Ok(reader) = file.try_clone() {
                    serve_connection(reader, file, &handler);
                }
            });
        }
    }
}

#[cfg(unix)]
mod imp {
    use std::os::unix::fs::PermissionsExt;
    use std::os::unix::net::{UnixListener, UnixStream};

    use super::{Handler, ServeError, serve_connection};

    /// The socket file's permission bits in octal.
    pub fn describe_access(name: &str) -> std::io::Result<String> {
        Ok(format!("{:o}", std::fs::metadata(name)?.permissions().mode() & 0o777))
    }

    /// Binds the socket (removing a stale one left by a crash), restricts it to the owner, and
    /// starts accepting.
    pub fn start(name: &str, handler: Handler) -> Result<(), ServeError> {
        if UnixStream::connect(name).is_ok() {
            return Err(ServeError::AlreadyRunning);
        }
        let _ = std::fs::remove_file(name);
        let listener = UnixListener::bind(name).map_err(ServeError::Io)?;
        std::fs::set_permissions(name, std::fs::Permissions::from_mode(0o600)).map_err(ServeError::Io)?;
        std::thread::Builder::new()
            .name("terminus ipc accept".into())
            .spawn(move || {
                for stream in listener.incoming().flatten() {
                    let handler = handler.clone();
                    let _ = std::thread::Builder::new().name("terminus ipc connection".into()).spawn(move || {
                        if let Ok(reader) = stream.try_clone() {
                            serve_connection(reader, stream, &handler);
                        }
                    });
                }
            })
            .map_err(ServeError::Io)?;
        Ok(())
    }
}
