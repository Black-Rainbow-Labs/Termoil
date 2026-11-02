//! The audible bell on macOS: AppKit's `NSBeep`, which plays the alert sound chosen in System Settings.

#[link(name = "AppKit", kind = "framework")]
unsafe extern "C" {
    fn NSBeep();
}

/// Plays the system alert sound.
pub fn ring() {
    // SAFETY: NSBeep takes no arguments, has no preconditions and may be called from any thread.
    unsafe { NSBeep() };
}
