//! Times every window message handled while the first window is being shown, for the startup
//! probe (P4, `docs/decisions/startup.md`). On Windows, `ShowWindow` sends its messages
//! synchronously, so the probe's `shown` phase alone cannot say whose handler the time went to.
//! Two thread hooks record when each message enters and leaves the window procedure. Only runs
//! when `TERMOIL_STARTUP_PROBE` is set.

/// Starts recording messages on this thread.
pub fn begin() {
    if std::env::var_os("TERMOIL_STARTUP_PROBE").is_some() {
        imp::begin();
    }
}

/// Stops recording.
pub fn end() {
    imp::end();
}

/// The messages recorded, as a JSON array of `[message, entered ms, duration ms]`.
pub fn json() -> String {
    imp::json()
}

#[cfg(windows)]
mod imp {
    use std::cell::RefCell;

    use windows::Win32::Foundation::{LPARAM, LRESULT, WPARAM};
    use windows::Win32::System::Threading::GetCurrentThreadId;
    use windows::Win32::UI::WindowsAndMessaging::{
        CWPRETSTRUCT, CWPSTRUCT, CallNextHookEx, HHOOK, SetWindowsHookExW, UnhookWindowsHookEx, WH_CALLWNDPROC,
        WH_CALLWNDPROCRET,
    };

    thread_local! {
        static HOOKS: RefCell<Vec<HHOOK>> = const { RefCell::new(Vec::new()) };
        static OPEN: RefCell<Vec<(u32, f64)>> = const { RefCell::new(Vec::new()) };
        static DONE: RefCell<Vec<(u32, f64, f64, usize)>> = const { RefCell::new(Vec::new()) };
    }

    /// Called before a message reaches a window procedure.
    unsafe extern "system" fn before(code: i32, wp: WPARAM, lp: LPARAM) -> LRESULT {
        if code >= 0 {
            // SAFETY: for WH_CALLWNDPROC, lParam points at a CWPSTRUCT for the length of the call.
            let m = unsafe { &*(lp.0 as *const CWPSTRUCT) };
            OPEN.with(|o| o.borrow_mut().push((m.message, crate::gui::since_start())));
        }
        // SAFETY: passes the hook on as the documentation requires.
        unsafe { CallNextHookEx(None, code, wp, lp) }
    }

    /// Called after a window procedure returned.
    unsafe extern "system" fn after(code: i32, wp: WPARAM, lp: LPARAM) -> LRESULT {
        if code >= 0 {
            // SAFETY: for WH_CALLWNDPROCRET, lParam points at a CWPRETSTRUCT for the length of the call.
            let m = unsafe { &*(lp.0 as *const CWPRETSTRUCT) };
            let open = OPEN.with(|o| {
                let mut o = o.borrow_mut();
                let at = o.iter().rposition(|(msg, _)| *msg == m.message)?;
                let depth = at;
                let (_, t) = o.remove(at);
                Some((t, depth))
            });
            if let Some((t, depth)) = open {
                let now = crate::gui::since_start();
                DONE.with(|d| d.borrow_mut().push((m.message, t, now - t, depth)));
            }
        }
        // SAFETY: passes the hook on as the documentation requires.
        unsafe { CallNextHookEx(None, code, wp, lp) }
    }

    /// Installs both hooks on this thread.
    pub fn begin() {
        // SAFETY: thread hooks with procedures in this module; removed by `end`.
        let tid = unsafe { GetCurrentThreadId() };
        for (kind, f) in [
            (WH_CALLWNDPROC, before as unsafe extern "system" fn(i32, WPARAM, LPARAM) -> LRESULT),
            (WH_CALLWNDPROCRET, after),
        ] {
            // SAFETY: as above.
            if let Ok(h) = unsafe { SetWindowsHookExW(kind, Some(f), None, tid) } {
                HOOKS.with(|v| v.borrow_mut().push(h));
            }
        }
    }

    /// Removes the hooks.
    pub fn end() {
        HOOKS.with(|v| {
            for h in v.borrow_mut().drain(..) {
                // SAFETY: each handle came from SetWindowsHookExW on this thread.
                let _ = unsafe { UnhookWindowsHookEx(h) };
            }
        });
    }

    /// The recorded messages, outermost and nested, in the order they finished.
    pub fn json() -> String {
        DONE.with(|d| {
            let rows: Vec<String> =
                d.borrow().iter().map(|(m, t, dur, depth)| format!("[{m},{t:.2},{dur:.2},{depth}]")).collect();
            format!("[{}]", rows.join(","))
        })
    }
}

#[cfg(not(windows))]
mod imp {
    /// Other platforms have no synchronous show to attribute.
    pub fn begin() {}

    /// Nothing to remove.
    pub fn end() {}

    /// No messages.
    pub fn json() -> String {
        "[]".into()
    }
}
