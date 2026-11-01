//! Platform services the window needs: the system clipboard and opening a file or URL with the
//! default handler.
//!
//! On Windows the clipboard is written directly against the Win32 clipboard API. `arboard`, which
//! the TDD names, depends on `clipboard-win`, which is BSL 1.0 and outside the licence allow list.
//! macOS and Linux are filled in at M4.

use crate::app::Clipboard;

/// The system clipboard.
pub struct SystemClipboard;

impl Clipboard for SystemClipboard {
    /// The clipboard's text, when it holds text.
    fn get(&mut self) -> Option<String> {
        imp::get_text()
    }

    /// Writes the system clipboard.
    fn set(&mut self, text: &str) {
        set_clipboard(text);
    }
}

/// Puts text on the system clipboard. Errors are logged and otherwise ignored, because a failed
/// copy is not a reason to stop the terminal.
pub fn set_clipboard(text: &str) {
    if let Err(e) = imp::set_text(text) {
        eprintln!("terminus: clipboard write failed: {e}");
    }
}

/// Opens a path or URL with the platform's default handler.
pub fn open(target: &str) {
    let result = if cfg!(windows) {
        std::process::Command::new("cmd").args(["/c", "start", "", target]).spawn()
    } else if cfg!(target_os = "macos") {
        std::process::Command::new("open").arg(target).spawn()
    } else {
        std::process::Command::new("xdg-open").arg(target).spawn()
    };
    if let Err(e) = result {
        eprintln!("terminus: could not open {target}: {e}");
    }
}

#[cfg(windows)]
mod imp {
    use windows::Win32::Foundation::{GlobalFree, HANDLE, HGLOBAL};
    use windows::Win32::System::DataExchange::{
        CloseClipboard, EmptyClipboard, GetClipboardData, IsClipboardFormatAvailable, OpenClipboard, SetClipboardData,
    };
    use windows::Win32::System::Memory::{GMEM_MOVEABLE, GlobalAlloc, GlobalLock, GlobalSize, GlobalUnlock};
    use windows::Win32::System::Ole::CF_UNICODETEXT;

    /// Opens the clipboard, retrying briefly while another program holds it.
    fn open() -> Result<(), String> {
        for _ in 0..10 {
            // SAFETY: OpenClipboard with no owner window is allowed; it is closed by the caller.
            if unsafe { OpenClipboard(None) }.is_ok() {
                return Ok(());
            }
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
        Err("the clipboard is in use by another program".into())
    }

    /// Reads CF_UNICODETEXT.
    pub fn get_text() -> Option<String> {
        // SAFETY: the format check needs no open clipboard.
        if unsafe { IsClipboardFormatAvailable(CF_UNICODETEXT.0 as u32) }.is_err() {
            return None;
        }
        open().ok()?;
        // SAFETY: the clipboard is open; the handle is owned by the clipboard and only read while
        // locked; the clipboard is closed on every path.
        unsafe {
            let h = GetClipboardData(CF_UNICODETEXT.0 as u32).ok();
            let text = h.and_then(|h| {
                let g = HGLOBAL(h.0);
                let p = GlobalLock(g) as *const u16;
                if p.is_null() {
                    return None;
                }
                let max = GlobalSize(g) / 2;
                let len = (0..max).take_while(|&i| *p.add(i) != 0).count();
                let s = String::from_utf16_lossy(std::slice::from_raw_parts(p, len));
                let _ = GlobalUnlock(g);
                Some(s)
            });
            let _ = CloseClipboard();
            text
        }
    }

    /// Writes CF_UNICODETEXT.
    pub fn set_text(text: &str) -> Result<(), String> {
        let wide: Vec<u16> = text.encode_utf16().chain(Some(0)).collect();
        open()?;
        // SAFETY: the clipboard is open; the global block is filled while locked and ownership
        // passes to the clipboard on success, or it is freed on failure.
        unsafe {
            let _ = EmptyClipboard();
            let result = (|| {
                let g = GlobalAlloc(GMEM_MOVEABLE, wide.len() * 2).map_err(|e| e.to_string())?;
                let p = GlobalLock(g) as *mut u16;
                if p.is_null() {
                    let _ = GlobalFree(Some(g));
                    return Err("could not lock clipboard memory".to_string());
                }
                std::ptr::copy_nonoverlapping(wide.as_ptr(), p, wide.len());
                let _ = GlobalUnlock(g);
                if let Err(e) = SetClipboardData(CF_UNICODETEXT.0 as u32, Some(HANDLE(g.0))) {
                    let _ = GlobalFree(Some(g));
                    return Err(e.to_string());
                }
                Ok(())
            })();
            let _ = CloseClipboard();
            result
        }
    }
}

#[cfg(not(windows))]
mod imp {
    /// Not built until M4.
    pub fn get_text() -> Option<String> {
        None
    }

    /// Not built until M4.
    pub fn set_text(_text: &str) -> Result<(), String> {
        Err("the clipboard on this platform arrives at M4".into())
    }
}
