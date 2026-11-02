//! Reading transmitted image bytes from a file, a temporary file or shared memory (`t=f`, `t=t`, `t=s`).
//!
//! Every failure to read answers with the same `EBADF` message so a program cannot learn anything about
//! files it cannot read itself. None of this exists on wasm, where every medium but direct is refused.

use super::pixels::decode_base64;
use super::reply::KErr;

/// What the control data asks to read.
#[derive(Clone, Copy, Debug)]
#[cfg_attr(target_arch = "wasm32", allow(dead_code))]
pub(crate) struct MediaRequest {
    /// The medium: `f`, `t` or `s`.
    pub medium: u8,
    /// `S`: bytes to read, 0 for the whole file.
    pub size: u32,
    /// `O`: offset to start reading at.
    pub offset: u32,
    /// Whether the terminal allows file transfer at all.
    pub allowed: bool,
    /// Most bytes that may be read.
    pub max_bytes: usize,
    /// The host's shared memory reader.
    pub shared_memory: Option<super::SharedMemoryReader>,
}

/// The one error every read failure maps to.
fn unreadable() -> KErr {
    KErr::new("EBADF", "Failed to read image file")
}

/// Reads the bytes a file, temporary file or shared memory request names. `payload` is the base64 path or name.
pub(crate) fn read_media(request: &MediaRequest, payload: &[u8]) -> Result<Vec<u8>, KErr> {
    if !request.allowed {
        return Err(KErr::new("EPERM", "File transfer is not permitted"));
    }
    let name = decode_base64(payload)?;
    let name = String::from_utf8(name).map_err(|_| unreadable())?;
    read_named(request, &name)
}

/// On wasm there is no file system: the request fails like any unreadable file.
#[cfg(target_arch = "wasm32")]
fn read_named(_request: &MediaRequest, _name: &str) -> Result<Vec<u8>, KErr> {
    Err(unreadable())
}

/// Dispatches to the file or shared memory reader.
#[cfg(not(target_arch = "wasm32"))]
fn read_named(request: &MediaRequest, name: &str) -> Result<Vec<u8>, KErr> {
    match request.medium {
        b's' => read_shared_memory(request, name),
        b't' => {
            let data = read_file(request, name)?;
            remove_if_temporary(name);
            Ok(data)
        }
        _ => read_file(request, name),
    }
}

/// Paths the terminal refuses to read from because opening them can have side effects.
#[cfg(not(target_arch = "wasm32"))]
fn is_sensitive(path: &str) -> bool {
    ["/proc", "/sys", "/dev"].iter().any(|p| path == *p || path.starts_with(&format!("{}/", p)))
}

/// Reads a regular file (following symlinks), honouring the `S` size and `O` offset keys.
#[cfg(not(target_arch = "wasm32"))]
fn read_file(request: &MediaRequest, name: &str) -> Result<Vec<u8>, KErr> {
    if name.is_empty() || (is_sensitive(name) && !name.starts_with("/dev/shm/")) {
        return Err(unreadable());
    }
    let meta = std::fs::metadata(name).map_err(|_| unreadable())?;
    if !meta.is_file() {
        return Err(unreadable());
    }
    read_range(request, name)
}

/// Reads `S` bytes from offset `O` (the whole file when `S` is 0), at most `max_bytes`.
#[cfg(not(target_arch = "wasm32"))]
fn read_range(request: &MediaRequest, name: &str) -> Result<Vec<u8>, KErr> {
    use std::io::{Read, Seek, SeekFrom};
    let mut file = std::fs::File::open(name).map_err(|_| unreadable())?;
    file.seek(SeekFrom::Start(request.offset as u64)).map_err(|_| unreadable())?;
    let wanted = if request.size > 0 { request.size as u64 } else { request.max_bytes as u64 };
    if wanted > request.max_bytes as u64 {
        return Err(KErr::new("EFBIG", "Image data is too large"));
    }
    let mut out = Vec::new();
    file.take(if request.size > 0 { wanted } else { wanted + 1 }).read_to_end(&mut out).map_err(|_| unreadable())?;
    if request.size > 0 && (out.len() as u64) < wanted {
        return Err(unreadable());
    }
    if out.len() as u64 > wanted {
        return Err(KErr::new("EFBIG", "Image data is too large"));
    }
    Ok(out)
}

/// Shared memory: the host's reader opens the named object and copies the requested range.
#[cfg(not(target_arch = "wasm32"))]
fn read_shared_memory(request: &MediaRequest, name: &str) -> Result<Vec<u8>, KErr> {
    use super::ShmError;
    let Some(read_object) = request.shared_memory else {
        return Err(KErr::new("EINVAL", "This terminal cannot read shared memory"));
    };
    read_object(name, request.offset as u64, request.size as u64, request.max_bytes).map_err(|e| match e {
        ShmError::Unreadable => unreadable(),
        ShmError::TooLarge => KErr::new("EFBIG", "Image data is too large"),
        ShmError::Unsupported => KErr::new("EINVAL", "Shared memory transmission is not supported on this platform"),
    })
}

/// Deletes a `t=t` file, but only inside a known temporary directory and only when the path contains
/// `tty-graphics-protocol`, as the protocol requires.
#[cfg(not(target_arch = "wasm32"))]
fn remove_if_temporary(name: &str) {
    if !name.contains("tty-graphics-protocol") {
        return;
    }
    let Ok(real) = std::fs::canonicalize(name) else { return };
    let mut roots = vec![std::env::temp_dir(), std::path::PathBuf::from("/tmp"), std::path::PathBuf::from("/dev/shm")];
    if let Some(dir) = std::env::var_os("TMPDIR") {
        roots.push(dir.into());
    }
    let inside = roots.iter().filter_map(|r| std::fs::canonicalize(r).ok()).any(|r| real.starts_with(r));
    if inside && real.to_string_lossy().contains("tty-graphics-protocol") {
        let _ = std::fs::remove_file(&real);
    }
}
