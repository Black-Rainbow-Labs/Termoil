//! The blurred backdrop behind a translucent window (`window.blur`, PRD 5.2: background blur
//! where the compositor supports it). winit does it on macOS and on Wayland compositors with the
//! KDE blur protocol; on Windows 11 the system acrylic backdrop does it through DWM.

use winit::window::Window;

/// Asks the compositor to blur what shows through a translucent window, when `blur` is set.
pub fn apply(window: &Window, translucent: bool, blur: bool) {
    if !translucent || !blur {
        return;
    }
    window.set_blur(true);
    imp::acrylic(window);
}

#[cfg(windows)]
mod imp {
    use windows::Win32::Foundation::HWND;
    use windows::Win32::Graphics::Dwm::{DWMWINDOWATTRIBUTE, DwmSetWindowAttribute};
    use winit::raw_window_handle::{HasWindowHandle, RawWindowHandle};
    use winit::window::Window;

    /// `DWMWA_SYSTEMBACKDROP_TYPE`, Windows 11 22H2 and later.
    const SYSTEMBACKDROP_TYPE: DWMWINDOWATTRIBUTE = DWMWINDOWATTRIBUTE(38);
    /// `DWMSBT_TRANSIENTWINDOW`: the acrylic backdrop.
    const ACRYLIC: i32 = 3;

    /// Sets the acrylic system backdrop. Older Windows rejects the attribute and the window stays
    /// translucent without blur.
    pub fn acrylic(window: &Window) {
        let Ok(handle) = window.window_handle() else { return };
        let RawWindowHandle::Win32(h) = handle.as_raw() else { return };
        let hwnd = HWND(h.hwnd.get() as *mut core::ffi::c_void);
        let value = ACRYLIC;
        // SAFETY: the window is live and owned by this thread; the attribute value is a valid i32
        // of the size passed.
        let _ = unsafe {
            DwmSetWindowAttribute(hwnd, SYSTEMBACKDROP_TYPE, &value as *const i32 as *const _, size_of::<i32>() as u32)
        };
    }
}

#[cfg(not(windows))]
mod imp {
    use winit::window::Window;

    /// winit's `set_blur` covers macOS and Wayland.
    pub fn acrylic(_window: &Window) {}
}
