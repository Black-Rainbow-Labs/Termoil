//! Reading a named shared memory object, for the kitty graphics protocol's shared memory transmission
//! (`t=s`). `terminus-vt` has no operating system code of its own, so the host hands it this reader
//! (`TerminalConfig::shared_memory`).
//!
//! On Unix the name is a POSIX shared memory object (`shm_open`, `fstat`, `mmap`, `munmap`, `close`) and the
//! terminal unlinks it after reading. On Windows the name is a named file mapping (`OpenFileMappingW`,
//! `MapViewOfFile`, `UnmapViewOfFile`, `CloseHandle`); the client owns the mapping, so nothing is deleted.

/// Why a shared memory read failed.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ShmError {
    /// The name is invalid, the object does not exist, or the offset and size lie outside it.
    Unreadable,
    /// The requested size is over the quota.
    TooLarge,
    /// This platform has no shared memory transmission.
    #[cfg_attr(any(unix, windows), allow(dead_code))]
    Unsupported,
}

/// Checks `offset`, `size` (0 means to the end) and the quota against an object of `total` bytes, and
/// returns how many bytes to copy.
#[cfg(any(unix, windows))]
fn checked_length(total: u64, offset: u64, size: u64, max_bytes: usize) -> Result<usize, ShmError> {
    if offset > total {
        return Err(ShmError::Unreadable);
    }
    let len = if size > 0 { size } else { total - offset };
    if len > max_bytes as u64 {
        return Err(ShmError::TooLarge);
    }
    if len == 0 || offset.checked_add(len).is_none_or(|end| end > total) {
        return Err(ShmError::Unreadable);
    }
    Ok(len as usize)
}

/// There is no shared memory on this platform.
#[cfg(not(any(unix, windows)))]
pub fn read_shared_memory(_name: &str, _offset: u64, _size: u64, _max_bytes: usize) -> Result<Vec<u8>, ShmError> {
    Err(ShmError::Unsupported)
}

/// Reads `size` bytes (the rest of the object when 0) from `offset` of the POSIX shared memory object
/// `name`, then unlinks the object. A leading slash is added when the name has none.
#[cfg(unix)]
pub fn read_shared_memory(name: &str, offset: u64, size: u64, max_bytes: usize) -> Result<Vec<u8>, ShmError> {
    let object = name.strip_prefix('/').unwrap_or(name);
    if object.is_empty() || object.contains('/') || object.contains('\0') {
        return Err(ShmError::Unreadable);
    }
    let cname = std::ffi::CString::new(format!("/{}", object)).map_err(|_| ShmError::Unreadable)?;
    // SAFETY: `cname` is a valid NUL terminated string that outlives the call.
    let fd = unsafe { libc::shm_open(cname.as_ptr(), libc::O_RDONLY, 0) };
    if fd < 0 {
        return Err(ShmError::Unreadable);
    }
    let result = copy_from_descriptor(fd, offset, size, max_bytes);
    // SAFETY: `fd` is the descriptor opened above and is closed exactly once; `cname` is still valid.
    unsafe {
        libc::close(fd);
        libc::shm_unlink(cname.as_ptr());
    }
    result
}

/// Maps the object behind `fd` read only, copies the requested range out and unmaps it.
#[cfg(unix)]
fn copy_from_descriptor(fd: libc::c_int, offset: u64, size: u64, max_bytes: usize) -> Result<Vec<u8>, ShmError> {
    // SAFETY: an all zero `stat` is a valid value for `fstat` to fill in.
    let mut st: libc::stat = unsafe { std::mem::zeroed() };
    // SAFETY: `fd` is open and `st` is a valid writable `stat`.
    if unsafe { libc::fstat(fd, &mut st) } != 0 || st.st_size < 0 {
        return Err(ShmError::Unreadable);
    }
    let len = checked_length(st.st_size as u64, offset, size, max_bytes)?;
    let map_len = offset as usize + len;
    // SAFETY: a null hint, a nonzero length and a valid read only descriptor; the result is checked below.
    let base = unsafe { libc::mmap(std::ptr::null_mut(), map_len, libc::PROT_READ, libc::MAP_SHARED, fd, 0) };
    if base == libc::MAP_FAILED {
        return Err(ShmError::Unreadable);
    }
    // SAFETY: the mapping is `map_len` bytes long and `offset + len == map_len`, so the range is inside it.
    let bytes = unsafe { std::slice::from_raw_parts((base as *const u8).add(offset as usize), len) }.to_vec();
    // SAFETY: `base` and `map_len` are exactly what `mmap` returned and was given; no slice into it remains.
    unsafe { libc::munmap(base, map_len) };
    Ok(bytes)
}

/// Whether `name` is a plain mapping name, optionally prefixed with `Local\` or `Global\`.
#[cfg(windows)]
fn valid_windows_name(name: &str) -> bool {
    let plain = name.strip_prefix("Local\\").or_else(|| name.strip_prefix("Global\\")).unwrap_or(name);
    !plain.is_empty() && !plain.contains(['/', '\\', '\0'])
}

/// Reads `size` bytes (the rest of the mapping when 0) from `offset` of the named file mapping `name`.
/// The mapping belongs to the client, so it is only unmapped and its handle closed here.
#[cfg(windows)]
pub fn read_shared_memory(name: &str, offset: u64, size: u64, max_bytes: usize) -> Result<Vec<u8>, ShmError> {
    use windows::Win32::Foundation::CloseHandle;
    use windows::Win32::System::Memory::{FILE_MAP_READ, MapViewOfFile, OpenFileMappingW};
    use windows::core::PCWSTR;
    if !valid_windows_name(name) {
        return Err(ShmError::Unreadable);
    }
    let wide: Vec<u16> = name.encode_utf16().chain(std::iter::once(0)).collect();
    // SAFETY: `wide` is a NUL terminated UTF-16 string that outlives the call.
    let handle =
        unsafe { OpenFileMappingW(FILE_MAP_READ.0, false, PCWSTR(wide.as_ptr())) }.map_err(|_| ShmError::Unreadable)?;
    // SAFETY: `handle` is a file mapping handle just opened; a zero length maps the whole object.
    let view = unsafe { MapViewOfFile(handle, FILE_MAP_READ, 0, 0, 0) };
    let result =
        if view.Value.is_null() { Err(ShmError::Unreadable) } else { copy_from_view(view, offset, size, max_bytes) };
    // SAFETY: `handle` was opened above and is closed exactly once.
    unsafe {
        let _ = CloseHandle(handle);
    }
    result
}

/// Copies the requested range out of a mapped view and unmaps it.
#[cfg(windows)]
fn copy_from_view(
    view: windows::Win32::System::Memory::MEMORY_MAPPED_VIEW_ADDRESS, offset: u64, size: u64, max_bytes: usize,
) -> Result<Vec<u8>, ShmError> {
    use windows::Win32::System::Memory::{MEMORY_BASIC_INFORMATION, UnmapViewOfFile, VirtualQuery};
    let mut info = MEMORY_BASIC_INFORMATION::default();
    // SAFETY: `view` is a mapped address and `info` is a valid writable structure of the size passed.
    let got = unsafe { VirtualQuery(Some(view.Value), &mut info, std::mem::size_of::<MEMORY_BASIC_INFORMATION>()) };
    let result = if got == 0 {
        Err(ShmError::Unreadable)
    } else {
        // The region size is the mapping size rounded up to a whole page.
        checked_length(info.RegionSize as u64, offset, size, max_bytes).map(|len| {
            // SAFETY: `offset + len <= RegionSize`, the readable size of the view, so the range is mapped.
            unsafe { std::slice::from_raw_parts((view.Value as *const u8).add(offset as usize), len) }.to_vec()
        })
    };
    // SAFETY: `view` came from `MapViewOfFile` and the copy above is the last use of it.
    unsafe {
        let _ = UnmapViewOfFile(view);
    }
    result
}
