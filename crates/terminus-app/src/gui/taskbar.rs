//! The taskbar button's progress (OSC 9;4, PRD 5.1): Windows shows it on the window's taskbar
//! button through `ITaskbarList3`. Other platforms show it in the tab label and the window title
//! only (`view::with_progress`).

use winit::window::Window;

use crate::app::Progress;

/// Shows `progress` on the window's taskbar button, or clears it for None.
pub fn show(window: &Window, progress: Option<Progress>) {
    imp::show(window, progress);
}

#[cfg(windows)]
mod imp {
    use std::cell::OnceCell;

    use windows::Win32::Foundation::HWND;
    use windows::Win32::System::Com::{
        CLSCTX_INPROC_SERVER, COINIT_APARTMENTTHREADED, CoCreateInstance, CoInitializeEx,
    };
    use windows::Win32::UI::Shell::{
        ITaskbarList3, TBPF_ERROR, TBPF_INDETERMINATE, TBPF_NOPROGRESS, TBPF_NORMAL, TBPF_PAUSED, TaskbarList,
    };
    use winit::raw_window_handle::{HasWindowHandle, RawWindowHandle};
    use winit::window::Window;

    use crate::app::Progress;

    thread_local! {
        /// The taskbar object, made once on the UI thread (None when it could not be made).
        static TASKBAR: OnceCell<Option<ITaskbarList3>> = const { OnceCell::new() };
    }

    /// Makes the taskbar object.
    fn taskbar() -> Option<ITaskbarList3> {
        // SAFETY: COM is initialised for this thread (a second call is harmless) before the object
        // is created; both calls take no pointers of ours.
        unsafe {
            let _ = CoInitializeEx(None, COINIT_APARTMENTTHREADED);
            let list: ITaskbarList3 = CoCreateInstance(&TaskbarList, None, CLSCTX_INPROC_SERVER).ok()?;
            list.HrInit().ok()?;
            Some(list)
        }
    }

    /// Sets the window's taskbar progress state and value.
    pub fn show(window: &Window, progress: Option<Progress>) {
        let Ok(handle) = window.window_handle() else { return };
        let RawWindowHandle::Win32(h) = handle.as_raw() else { return };
        let hwnd = HWND(h.hwnd.get() as *mut core::ffi::c_void);
        TASKBAR.with(|cell| {
            let Some(list) = cell.get_or_init(taskbar) else { return };
            let flag = match progress.map(|p| p.state) {
                Some(1) => TBPF_NORMAL,
                Some(2) => TBPF_ERROR,
                Some(3) => TBPF_INDETERMINATE,
                Some(4) => TBPF_PAUSED,
                _ => TBPF_NOPROGRESS,
            };
            // SAFETY: `hwnd` is a live window owned by this thread; the calls take plain values.
            unsafe {
                let _ = list.SetProgressState(hwnd, flag);
                if let Some(p) = progress.filter(|p| p.state != 3) {
                    let _ = list.SetProgressValue(hwnd, p.value.min(100) as u64, 100);
                }
            }
        });
    }
}

#[cfg(not(windows))]
mod imp {
    use winit::window::Window;

    use crate::app::Progress;

    /// No taskbar progress outside Windows; the label carries it.
    pub fn show(_window: &Window, _progress: Option<Progress>) {}
}
