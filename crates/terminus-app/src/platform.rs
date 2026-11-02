//! Platform services the window needs: the system clipboard and opening a file or URL with the
//! default handler.
//!
//! On Windows the clipboard is written directly against the Win32 clipboard API. `arboard`, which
//! the TDD names, depends on `clipboard-win` there, which is BSL 1.0 and outside the licence allow
//! list, so `arboard` is a dependency of the non Windows targets only. It serves macOS, Wayland and
//! X11, and on Linux it also reads and writes the primary selection.

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

/// The primary selection, the text most recently selected in any program. Only Linux has one;
/// every other platform returns None. Middle click pastes this on Linux.
pub fn primary_selection() -> Option<String> {
    #[cfg(target_os = "linux")]
    {
        imp::get_primary()
    }
    #[cfg(not(target_os = "linux"))]
    {
        None
    }
}

/// Makes `text` the primary selection, so a middle click in any program pastes it. Only Linux
/// has a primary selection; elsewhere this does nothing. Errors are logged and otherwise ignored.
pub fn set_primary_selection(text: &str) {
    #[cfg(target_os = "linux")]
    if let Err(e) = imp::set_primary(text) {
        eprintln!("terminus: primary selection write failed: {e}");
    }
    #[cfg(not(target_os = "linux"))]
    let _ = text;
}

/// The application id notifications are sent under; the installer's Start menu shortcut carries
/// the same id, which is what lets Windows show them.
pub const APP_ID: &str = "Terminus.Terminus";

/// Tells Windows which application this process is, so its windows group under the Terminus
/// shortcut and its notifications are allowed. Called once at start.
pub fn set_app_id() {
    #[cfg(windows)]
    {
        let id = windows::core::HSTRING::from(APP_ID);
        // SAFETY: the string lives for the call; the function only copies it.
        let _ = unsafe { windows::Win32::UI::Shell::SetCurrentProcessExplicitAppUserModelID(&id) };
    }
}

/// Shows a desktop notification on a background thread (the platform call can take tens of
/// milliseconds). Failures are logged: an unregistered development build cannot show toasts.
pub fn notify(title: &str, body: &str) {
    let (title, body) = (title.to_string(), body.to_string());
    let _ = std::thread::Builder::new().name("terminus notification".into()).spawn(move || {
        if let Err(e) = imp_notify(&title, &body) {
            eprintln!("terminus: notification not shown: {e}");
        }
    });
}

/// Escapes text for the toast XML.
#[cfg(windows)]
fn xml_escape(s: &str) -> String {
    s.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;").replace('"', "&quot;")
}

/// Shows a Windows toast notification.
#[cfg(windows)]
fn imp_notify(title: &str, body: &str) -> Result<(), String> {
    use windows::Data::Xml::Dom::XmlDocument;
    use windows::UI::Notifications::{ToastNotification, ToastNotificationManager};
    use windows::core::HSTRING;
    // SAFETY: initialising the Windows Runtime on this new thread; it is torn down when the
    // thread ends.
    let _ =
        unsafe { windows::Win32::System::WinRT::RoInitialize(windows::Win32::System::WinRT::RO_INIT_MULTITHREADED) };
    let xml = format!(
        "<toast><visual><binding template=\"ToastGeneric\"><text>{}</text><text>{}</text></binding></visual></toast>",
        xml_escape(title),
        xml_escape(body)
    );
    let doc = XmlDocument::new().map_err(|e| e.to_string())?;
    doc.LoadXml(&HSTRING::from(xml)).map_err(|e| e.to_string())?;
    let toast = ToastNotification::CreateToastNotification(&doc).map_err(|e| e.to_string())?;
    let notifier =
        ToastNotificationManager::CreateToastNotifierWithId(&HSTRING::from(APP_ID)).map_err(|e| e.to_string())?;
    notifier.Show(&toast).map_err(|e| e.to_string())
}

/// Desktop notifications on macOS and Linux arrive with those apps.
#[cfg(not(windows))]
fn imp_notify(_title: &str, _body: &str) -> Result<(), String> {
    Err("notifications on this platform arrive with the macOS and Linux apps".into())
}

/// The audible bell: the system's default sound.
pub fn beep() {
    #[cfg(windows)]
    // SAFETY: MessageBeep takes a sound id and has no other inputs.
    let _ = unsafe {
        windows::Win32::System::Diagnostics::Debug::MessageBeep(windows::Win32::UI::WindowsAndMessaging::MB_OK)
    };
    #[cfg(not(windows))]
    {
        use std::io::Write;
        let _ = std::io::stderr().write_all(b"\x07");
    }
}

/// True when a program of this name is on PATH (with `.exe` or `.cmd` on Windows).
pub fn on_path(name: &str) -> bool {
    let Some(path) = std::env::var_os("PATH") else { return false };
    let names: Vec<String> =
        if cfg!(windows) { vec![format!("{name}.exe"), format!("{name}.cmd")] } else { vec![name.to_string()] };
    std::env::split_paths(&path).any(|d| names.iter().any(|n| d.join(n).is_file()))
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
    use std::sync::Mutex;

    /// The one clipboard connection for the process. On X11 the process serves the clipboard
    /// contents from a thread owned by this value, so it must outlive each copy.
    static BOARD: Mutex<Option<arboard::Clipboard>> = Mutex::new(None);

    /// Runs `f` with the shared clipboard connection, opening it on first use.
    fn with_board<T>(f: impl FnOnce(&mut arboard::Clipboard) -> Result<T, arboard::Error>) -> Result<T, String> {
        let mut guard = BOARD.lock().unwrap_or_else(|e| e.into_inner());
        if guard.is_none() {
            *guard = Some(arboard::Clipboard::new().map_err(|e| e.to_string())?);
        }
        let board = guard.as_mut().expect("clipboard was just opened");
        f(board).map_err(|e| e.to_string())
    }

    /// Reads the clipboard's text.
    pub fn get_text() -> Option<String> {
        with_board(|b| b.get_text()).ok()
    }

    /// Writes the clipboard's text.
    pub fn set_text(text: &str) -> Result<(), String> {
        with_board(|b| b.set_text(text.to_string()))
    }

    /// Reads the primary selection, which Linux fills with the last text selected.
    #[cfg(target_os = "linux")]
    pub fn get_primary() -> Option<String> {
        use arboard::{GetExtLinux, LinuxClipboardKind};
        with_board(|b| b.get().clipboard(LinuxClipboardKind::Primary).text()).ok()
    }

    /// Writes the primary selection.
    #[cfg(target_os = "linux")]
    pub fn set_primary(text: &str) -> Result<(), String> {
        use arboard::{LinuxClipboardKind, SetExtLinux};
        with_board(|b| b.set().clipboard(LinuxClipboardKind::Primary).text(text.to_string()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// True when a display server is reachable, which the clipboard round trip needs.
    fn display_present() -> bool {
        cfg!(windows) || std::env::var_os("DISPLAY").is_some() || std::env::var_os("WAYLAND_DISPLAY").is_some()
    }

    /// Text written to the system clipboard is read back unchanged, including non ASCII text.
    #[test]
    fn clipboard_round_trips_text() {
        if !display_present() {
            eprintln!("skipped: no display server for the clipboard");
            return;
        }
        let text = "terminus clipboard \u{e9}\u{4e16}\u{1f600}\nline two";
        set_clipboard(text);
        assert_eq!(imp::get_text().as_deref(), Some(text));
    }

    /// Text written to the primary selection is read back, and the clipboard is left alone.
    #[cfg(target_os = "linux")]
    #[test]
    fn primary_selection_is_separate_from_the_clipboard() {
        if !display_present() {
            eprintln!("skipped: no display server for the primary selection");
            return;
        }
        set_clipboard("clipboard text");
        set_primary_selection("primary text");
        assert_eq!(primary_selection().as_deref(), Some("primary text"));
        assert_eq!(imp::get_text().as_deref(), Some("clipboard text"));
    }
}
