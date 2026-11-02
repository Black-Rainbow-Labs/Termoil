//! Showing a new window (P4, `docs/decisions/startup.md`). On Windows, showing and activating in
//! one `ShowWindow` call runs the activation work inside it before the first frame can reach the
//! screen: winit builds its keyboard layout table on `WM_SETFOCUS`, and the text services start
//! on `WM_IME_SYSTEM`. Measured, that was 36 to 73 ms of the 56 ms show. So the window is shown
//! without activating it, the first frame is presented, and then the window is brought to the
//! front. Other platforms show the window in one step.

use winit::window::Window;

/// Shows a window that has its first frame drawn. Returns true when it still has to be activated
/// with `activate` after its next frame is presented.
pub fn first(window: &Window) -> bool {
    imp::first(window)
}

/// Activates a window shown by `first`: tells winit it is visible, and brings it to the front.
pub fn activate(window: &Window) {
    window.set_visible(true);
    imp::activate(window);
}

#[cfg(windows)]
mod imp {
    use windows::Win32::Foundation::HWND;
    use windows::Win32::UI::WindowsAndMessaging::{SW_SHOWNOACTIVATE, SetForegroundWindow, ShowWindow};
    use winit::raw_window_handle::{HasWindowHandle, RawWindowHandle};
    use winit::window::Window;

    /// The window's handle.
    fn hwnd(window: &Window) -> Option<HWND> {
        match window.window_handle().ok()?.as_raw() {
            RawWindowHandle::Win32(h) => Some(HWND(h.hwnd.get() as *mut core::ffi::c_void)),
            _ => None,
        }
    }

    /// Shows the window without activating it.
    pub fn first(window: &Window) -> bool {
        let Some(h) = hwnd(window) else {
            window.set_visible(true);
            return false;
        };
        // SAFETY: the handle belongs to a live window owned by this thread.
        let _ = unsafe { ShowWindow(h, SW_SHOWNOACTIVATE) };
        true
    }

    /// Brings the window to the front. Windows allows it under the same rules as activating the
    /// first window of a newly started process; otherwise the taskbar button flashes.
    pub fn activate(window: &Window) {
        if let Some(h) = hwnd(window) {
            // SAFETY: as above.
            let _ = unsafe { SetForegroundWindow(h) };
        }
    }
}

#[cfg(not(windows))]
mod imp {
    use winit::window::Window;

    /// Shows the window.
    pub fn first(window: &Window) -> bool {
        window.set_visible(true);
        false
    }

    /// Nothing more to do.
    pub fn activate(_window: &Window) {}
}
