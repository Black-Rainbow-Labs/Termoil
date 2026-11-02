//! Loads the pseudoconsole entry points from the bundled conpty.dll or from kernel32.

use crate::{ConptyChoice, ConptySource};
use std::ffi::{CString, c_void};
use std::io;
use std::os::windows::ffi::OsStrExt;
use std::path::{Path, PathBuf};
use windows::Win32::Foundation::{HANDLE, HMODULE};
use windows::Win32::System::Console::{COORD, HPCON};
use windows::Win32::System::LibraryLoader::{GetModuleHandleW, GetProcAddress, LoadLibraryW};
use windows::core::{HRESULT, PCSTR, PCWSTR, w};

/// Signature of CreatePseudoConsole and ConptyCreatePseudoConsole.
pub type CreateFn = unsafe extern "system" fn(COORD, HANDLE, HANDLE, u32, *mut HPCON) -> HRESULT;
/// Signature of ResizePseudoConsole and ConptyResizePseudoConsole.
pub type ResizeFn = unsafe extern "system" fn(HPCON, COORD) -> HRESULT;
/// Signature of ClosePseudoConsole and ConptyClosePseudoConsole.
pub type CloseFn = unsafe extern "system" fn(HPCON);

/// Flag asking ConPTY to measure glyph width by grapheme clusters, as the terminal does.
/// Value 0x08 (PSEUDOCONSOLE_GLYPH_WIDTH_GRAPHEMES) is defined in `inc/conpty.h` of the MIT licensed
/// Microsoft.Windows.Console.ConPTY NuGet package (microsoft/terminal, src/winconpty), version 1.25.
pub const PSEUDOCONSOLE_GLYPH_WIDTH_GRAPHEMES: u32 = 0x08;

/// The three pseudoconsole functions and where they came from.
#[derive(Clone, Copy)]
pub struct ConptyApi {
    pub create: CreateFn,
    pub resize: ResizeFn,
    pub close: CloseFn,
    pub source: ConptySource,
}

/// Loads the pseudoconsole implementation named by `choice`, falling back to kernel32 for `Auto`.
pub fn load_api(choice: &ConptyChoice) -> io::Result<ConptyApi> {
    match choice {
        ConptyChoice::Inbox => load_inbox(),
        ConptyChoice::Bundled(dir) => load_bundled(dir),
        ConptyChoice::Auto => {
            for dir in auto_search_dirs() {
                if let Ok(api) = load_bundled(&dir) {
                    return Ok(api);
                }
            }
            load_inbox()
        }
    }
}

/// Directories searched for conpty.dll: TERMOIL_CONPTY_DIR first, then the executable's directory.
fn auto_search_dirs() -> Vec<PathBuf> {
    let mut dirs = Vec::new();
    if let Some(dir) = std::env::var_os("TERMOIL_CONPTY_DIR") {
        dirs.push(PathBuf::from(dir));
    }
    if let Some(parent) = std::env::current_exe().ok().as_deref().and_then(Path::parent) {
        dirs.push(parent.to_path_buf());
    }
    dirs
}

/// Loads `conpty.dll` from `dir`; the matching OpenConsole.exe must sit beside it.
fn load_bundled(dir: &Path) -> io::Result<ConptyApi> {
    let dll = dir.join("conpty.dll");
    if !dll.is_file() {
        return Err(io::Error::new(io::ErrorKind::NotFound, format!("{} not found", dll.display())));
    }
    let wide: Vec<u16> = dll.as_os_str().encode_wide().chain(Some(0)).collect();
    // SAFETY: `wide` is a nul terminated UTF-16 path that outlives the call.
    let module = unsafe { LoadLibraryW(PCWSTR(wide.as_ptr())) }.map_err(io::Error::from)?;
    let create = ["ConptyCreatePseudoConsole", "CreatePseudoConsole"];
    let resize = ["ConptyResizePseudoConsole", "ResizePseudoConsole"];
    let close = ["ConptyClosePseudoConsole", "ClosePseudoConsole"];
    resolve_api(module, &create, &resize, &close, ConptySource::Bundled)
}

/// Uses the pseudoconsole functions exported by kernel32.dll.
fn load_inbox() -> io::Result<ConptyApi> {
    // SAFETY: kernel32.dll is always loaded in a Windows process.
    let module = unsafe { GetModuleHandleW(w!("kernel32.dll")) }.map_err(io::Error::from)?;
    resolve_api(
        module,
        &["CreatePseudoConsole"],
        &["ResizePseudoConsole"],
        &["ClosePseudoConsole"],
        ConptySource::Inbox,
    )
}

/// Looks the three functions up in `module`, trying each candidate export name in order.
fn resolve_api(
    module: HMODULE, create: &[&str], resize: &[&str], close: &[&str], source: ConptySource,
) -> io::Result<ConptyApi> {
    let create = lookup(module, create)?;
    let resize = lookup(module, resize)?;
    let close = lookup(module, close)?;
    // SAFETY: each address is the export documented with exactly this signature.
    unsafe {
        Ok(ConptyApi {
            create: std::mem::transmute::<*const c_void, CreateFn>(create),
            resize: std::mem::transmute::<*const c_void, ResizeFn>(resize),
            close: std::mem::transmute::<*const c_void, CloseFn>(close),
            source,
        })
    }
}

/// Returns the address of the first export in `names` that `module` has.
fn lookup(module: HMODULE, names: &[&str]) -> io::Result<*const c_void> {
    for name in names {
        let c_name = CString::new(*name).expect("export names have no nul");
        // SAFETY: `c_name` is a nul terminated string that outlives the call.
        if let Some(address) = unsafe { GetProcAddress(module, PCSTR(c_name.as_ptr().cast())) } {
            return Ok(address as *const c_void);
        }
    }
    Err(io::Error::new(io::ErrorKind::NotFound, format!("none of {names:?} exported")))
}
