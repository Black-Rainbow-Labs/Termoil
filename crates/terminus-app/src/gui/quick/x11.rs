//! The quick terminal hotkey on X11: a passive key grab on the root window, held by a thread with
//! its own X connection. Each key press the server delivers for the grab sends the user event.

use winit::event_loop::EventLoopProxy;
use x11rb::connection::Connection;
use x11rb::protocol::Event;
use x11rb::protocol::xproto::{ConnectionExt, GrabMode, ModMask};
use x11rb::rust_connection::RustConnection;

use super::keymap;
use crate::config::KeyChord;
use crate::gui::UserEvent;

/// X11 modifier bit Lock, which is Caps Lock.
const LOCK_MASK: u16 = 1 << 1;
/// X11 modifier bit Mod2, which is Num Lock on every common keyboard layout.
const NUM_LOCK_MASK: u16 = 1 << 4;

/// What a grab needs: the keycode and the modifier mask.
struct Grab {
    keycode: u8,
    mods: u16,
}

/// Looks up the keycode of the chord's key in the server's keyboard mapping.
fn resolve_grab(conn: &RustConnection, chord: &KeyChord) -> Result<Grab, String> {
    let keysym = keymap::x11_keysym(&chord.key).ok_or_else(|| format!("'{}' cannot be a global hotkey", chord.key))?;
    let setup = conn.setup();
    let (min, max) = (setup.min_keycode, setup.max_keycode);
    let reply =
        conn.get_keyboard_mapping(min, max - min + 1).map_err(|e| e.to_string())?.reply().map_err(|e| e.to_string())?;
    let per = reply.keysyms_per_keycode as usize;
    let keycode = keymap::keycode_for_keysym(keysym, min, per, &reply.keysyms)
        .ok_or_else(|| format!("the keyboard has no key for '{}'", chord.key))?;
    Ok(Grab { keycode, mods: keymap::x11_modifiers(chord.mods) })
}

/// Grabs the key on the root window with and without Caps Lock and Num Lock held, so the hotkey
/// works whatever the lock keys are doing. Fails when another client already holds the key.
fn grab_all_lock_states(conn: &RustConnection, root: u32, grab: &Grab) -> Result<(), String> {
    for extra in [0, LOCK_MASK, NUM_LOCK_MASK, LOCK_MASK | NUM_LOCK_MASK] {
        conn.grab_key(false, root, ModMask::from(grab.mods | extra), grab.keycode, GrabMode::ASYNC, GrabMode::ASYNC)
            .map_err(|e| e.to_string())?
            .check()
            .map_err(|e| format!("the hotkey is taken by another program ({e})"))?;
    }
    conn.flush().map_err(|e| e.to_string())
}

/// Forwards each key press of the grab to the event loop until the loop or the connection ends.
fn forward_presses(conn: &RustConnection, keycode: u8, proxy: &EventLoopProxy<UserEvent>) {
    while let Ok(event) = conn.wait_for_event() {
        if let Event::KeyPress(press) = event
            && press.detail == keycode
            && proxy.send_event(UserEvent::QuickTerminal).is_err()
        {
            return;
        }
    }
}

/// Grabs the chord on a thread of its own and forwards each press. Returns an error string when
/// the X server cannot be reached, the chord has no key on this keyboard, or the key is taken.
pub fn listen(chord: &KeyChord, proxy: EventLoopProxy<UserEvent>) -> Result<(), String> {
    let (conn, screen) = x11rb::connect(None).map_err(|e| format!("cannot reach the X server: {e}"))?;
    let root = conn.setup().roots[screen].root;
    let grab = resolve_grab(&conn, chord)?;
    grab_all_lock_states(&conn, root, &grab)?;
    std::thread::Builder::new()
        .name("terminus quick terminal hotkey".into())
        .spawn(move || forward_presses(&conn, grab.keycode, &proxy))
        .map(|_| ())
        .map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Against a real X server (skipped without `DISPLAY`): the chord resolves to a keycode, the
    /// grab succeeds, and a second connection asking for the same chord is told it is taken.
    #[test]
    fn grabbing_a_chord_twice_reports_that_it_is_taken() {
        let Ok((first, screen)) = x11rb::connect(None) else { return };
        let root = first.setup().roots[screen].root;
        let chord = crate::keymap::parse_chord("ctrl+alt+shift+f9").unwrap();
        let grab = resolve_grab(&first, &chord).unwrap();
        grab_all_lock_states(&first, root, &grab).unwrap();
        let (second, _) = x11rb::connect(None).unwrap();
        let error = grab_all_lock_states(&second, root, &grab).unwrap_err();
        assert!(error.contains("taken by another program"), "{error}");
    }
}
