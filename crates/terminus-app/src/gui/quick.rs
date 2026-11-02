//! The quick terminal (`docs/TDD.md` section 4.8): a borderless window along one edge of the screen,
//! always on top, shown and hidden by the `quick-terminal` action and by a global hotkey
//! (`window.quick_terminal_hotkey`, `ctrl+\`` by default). It keeps its sessions while hidden.

use winit::dpi::{PhysicalPosition, PhysicalSize};
use winit::event_loop::EventLoopProxy;
use winit::window::{Window, WindowLevel};

use crate::config::{Edge, KeyChord};
use crate::gui::UserEvent;

/// The share of the screen the quick terminal covers across its edge.
pub const SHARE: f64 = 0.4;

/// Places a quick terminal window along `edge` of its monitor and sizes it.
pub fn place(window: &Window, edge: Edge) {
    let Some(m) = window.current_monitor().or_else(|| window.primary_monitor()) else { return };
    let (pos, size) = (m.position(), m.size());
    let (w, h) = (size.width as f64, size.height as f64);
    let (x, y, cw, ch) = match edge {
        Edge::Top => (0.0, 0.0, w, h * SHARE),
        Edge::Bottom => (0.0, h * (1.0 - SHARE), w, h * SHARE),
        Edge::Left => (0.0, 0.0, w * SHARE, h),
        Edge::Right => (w * (1.0 - SHARE), 0.0, w * SHARE, h),
    };
    window.set_outer_position(PhysicalPosition::new(pos.x + x as i32, pos.y + y as i32));
    let _ = window.request_inner_size(PhysicalSize::new(cw as u32, ch as u32));
}

/// The window attributes of a quick terminal: no frame, always on top, not in the taskbar list
/// of ordinary windows' order.
pub fn attributes(attrs: winit::window::WindowAttributes) -> winit::window::WindowAttributes {
    attrs.with_decorations(false).with_window_level(WindowLevel::AlwaysOnTop).with_title("Terminus quick terminal")
}

/// Starts listening for the global hotkey; each press sends `UserEvent::QuickTerminal`. Returns why
/// it could not, for the log (the key cannot be registered, or the platform has no support yet).
pub fn listen(chord: &KeyChord, proxy: EventLoopProxy<UserEvent>) -> Result<(), String> {
    imp::listen(chord, proxy)
}

#[cfg(windows)]
mod imp {
    use windows::Win32::UI::Input::KeyboardAndMouse::{
        HOT_KEY_MODIFIERS, MOD_ALT, MOD_CONTROL, MOD_NOREPEAT, MOD_SHIFT, MOD_WIN, RegisterHotKey,
    };
    use windows::Win32::UI::WindowsAndMessaging::{GetMessageW, MSG, WM_HOTKEY};
    use winit::event_loop::EventLoopProxy;

    use crate::config::KeyChord;
    use crate::gui::UserEvent;

    /// The virtual key code of a chord's key, for the keys a hotkey can reasonably use.
    pub fn virtual_key(key: &str) -> Option<u32> {
        let mut chars = key.chars();
        if let (Some(c), None) = (chars.next(), chars.clone().next()) {
            return match c {
                'a'..='z' => Some(c.to_ascii_uppercase() as u32),
                '0'..='9' => Some(c as u32),
                '`' => Some(0xC0),
                '-' => Some(0xBD),
                '=' => Some(0xBB),
                '[' => Some(0xDB),
                ']' => Some(0xDD),
                '\\' => Some(0xDC),
                ';' => Some(0xBA),
                '\'' => Some(0xDE),
                ',' => Some(0xBC),
                '.' => Some(0xBE),
                '/' => Some(0xBF),
                _ => None,
            };
        }
        match key {
            "space" => Some(0x20),
            "enter" => Some(0x0D),
            "escape" => Some(0x1B),
            k if k.starts_with('f') => k[1..].parse::<u32>().ok().filter(|n| (1..=24).contains(n)).map(|n| 0x6F + n),
            _ => None,
        }
    }

    /// Registers the hotkey on a thread of its own and forwards each press.
    pub fn listen(chord: &KeyChord, proxy: EventLoopProxy<UserEvent>) -> Result<(), String> {
        let vk = virtual_key(&chord.key).ok_or_else(|| format!("'{}' cannot be a global hotkey", chord.key))?;
        let mut mods = MOD_NOREPEAT;
        for (on, m) in [
            (chord.mods.ctrl, MOD_CONTROL),
            (chord.mods.alt, MOD_ALT),
            (chord.mods.shift, MOD_SHIFT),
            (chord.mods.super_key, MOD_WIN),
        ] {
            if on {
                mods |= m;
            }
        }
        let (tx, rx) = std::sync::mpsc::channel();
        std::thread::Builder::new()
            .name("terminus quick terminal hotkey".into())
            .spawn(move || {
                // SAFETY: the hotkey belongs to this thread's message queue; GetMessageW reads it.
                let ok = unsafe { RegisterHotKey(None, 1, HOT_KEY_MODIFIERS(mods.0), vk) }.is_ok();
                let _ = tx.send(ok);
                if !ok {
                    return;
                }
                let mut msg = MSG::default();
                // SAFETY: a standard message loop over this thread's queue.
                while unsafe { GetMessageW(&mut msg, None, 0, 0) }.as_bool() {
                    if msg.message == WM_HOTKEY && proxy.send_event(UserEvent::QuickTerminal).is_err() {
                        return;
                    }
                }
            })
            .map_err(|e| e.to_string())?;
        match rx.recv() {
            Ok(true) => Ok(()),
            _ => Err("the hotkey is taken by another program".into()),
        }
    }
}

mod keymap;
#[cfg(target_os = "macos")]
mod macos;
#[cfg(target_os = "linux")]
mod wayland;
#[cfg(target_os = "linux")]
mod x11;

/// On Linux the hotkey is an X11 key grab, or the Wayland shortcuts portal, chosen the way winit
/// chooses its backend: `WAYLAND_DISPLAY` set means Wayland, otherwise `DISPLAY` means X11.
#[cfg(target_os = "linux")]
mod imp {
    use winit::event_loop::EventLoopProxy;

    use crate::config::KeyChord;
    use crate::gui::UserEvent;

    /// Registers the hotkey with whichever display server this session runs on.
    pub fn listen(chord: &KeyChord, proxy: EventLoopProxy<UserEvent>) -> Result<(), String> {
        let set = |name: &str| std::env::var_os(name).is_some_and(|v| !v.is_empty());
        if set("WAYLAND_DISPLAY") {
            super::wayland::listen(chord, proxy)
        } else if set("DISPLAY") {
            super::x11::listen(chord, proxy)
        } else {
            Err("neither WAYLAND_DISPLAY nor DISPLAY is set, so there is no display server to register a hotkey with"
                .into())
        }
    }
}

/// On macOS the hotkey is a Carbon hotkey on the main thread.
#[cfg(target_os = "macos")]
mod imp {
    use winit::event_loop::EventLoopProxy;

    use crate::config::KeyChord;
    use crate::gui::UserEvent;

    /// Registers the hotkey with Carbon.
    pub fn listen(chord: &KeyChord, proxy: EventLoopProxy<UserEvent>) -> Result<(), String> {
        super::macos::listen(chord, proxy)
    }
}

/// Other platforms have no global hotkey support.
#[cfg(not(any(windows, target_os = "linux", target_os = "macos")))]
mod imp {
    use winit::event_loop::EventLoopProxy;

    use crate::config::KeyChord;
    use crate::gui::UserEvent;

    /// There is no global hotkey on this platform.
    pub fn listen(_chord: &KeyChord, _proxy: EventLoopProxy<UserEvent>) -> Result<(), String> {
        Err("this platform has no global hotkey support".into())
    }
}
