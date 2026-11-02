//! The quick terminal hotkey on macOS: Carbon's `RegisterEventHotKey`, with a handler on the
//! application event target. Carbon is the only API that registers a system wide hotkey without
//! the Accessibility permission. The registration must happen on the main thread, and the key
//! presses arrive through the application's own event loop, which winit runs.

use std::ffi::c_void;
use std::sync::OnceLock;

use winit::event_loop::EventLoopProxy;

use super::keymap;
use crate::config::KeyChord;
use crate::gui::UserEvent;

type OsStatus = i32;
type EventTargetRef = *mut c_void;
type EventHandlerCallRef = *mut c_void;
type EventRef = *mut c_void;
type EventHandlerRef = *mut c_void;
type EventHotKeyRef = *mut c_void;
type EventHandlerProc = extern "C" fn(EventHandlerCallRef, EventRef, *mut c_void) -> OsStatus;

/// kEventClassKeyboard, the four character code 'keyb'.
const EVENT_CLASS_KEYBOARD: u32 = u32::from_be_bytes(*b"keyb");
/// kEventHotKeyPressed.
const EVENT_HOT_KEY_PRESSED: u32 = 5;
/// The signature of the hotkey id, the four character code 'trmn'.
const SIGNATURE: u32 = u32::from_be_bytes(*b"trmn");
/// noErr.
const NO_ERR: OsStatus = 0;
/// eventHotKeyExistsErr, the status when another program holds the same hotkey.
const HOT_KEY_EXISTS_ERR: OsStatus = -9878;

/// Carbon's EventHotKeyID.
#[repr(C)]
struct EventHotKeyId {
    signature: u32,
    id: u32,
}

/// Carbon's EventTypeSpec.
#[repr(C)]
struct EventTypeSpec {
    event_class: u32,
    event_kind: u32,
}

#[link(name = "Carbon", kind = "framework")]
unsafe extern "C" {
    fn GetApplicationEventTarget() -> EventTargetRef;
    fn InstallEventHandler(
        target: EventTargetRef, handler: EventHandlerProc, count: u32, types: *const EventTypeSpec,
        user_data: *mut c_void, out: *mut EventHandlerRef,
    ) -> OsStatus;
    fn RegisterEventHotKey(
        key_code: u32, modifiers: u32, id: EventHotKeyId, target: EventTargetRef, options: u32,
        out: *mut EventHotKeyRef,
    ) -> OsStatus;
}

/// The proxy the handler sends through. Carbon gives the handler no closure, so it is a static.
static PROXY: OnceLock<EventLoopProxy<UserEvent>> = OnceLock::new();

/// Called by Carbon on the main thread for each press of the registered hotkey.
extern "C" fn on_hot_key(_call: EventHandlerCallRef, _event: EventRef, _user: *mut c_void) -> OsStatus {
    if let Some(proxy) = PROXY.get() {
        let _ = proxy.send_event(UserEvent::QuickTerminal);
    }
    NO_ERR
}

/// Installs the handler for hotkey presses on the application event target.
fn install_handler() -> Result<(), String> {
    let spec = EventTypeSpec { event_class: EVENT_CLASS_KEYBOARD, event_kind: EVENT_HOT_KEY_PRESSED };
    let mut handler: EventHandlerRef = std::ptr::null_mut();
    // SAFETY: the target is the application's own, the spec outlives the call (Carbon copies it),
    // the handler is a plain function that lives for the whole program, and `handler` is a valid
    // out pointer.
    let status = unsafe {
        InstallEventHandler(GetApplicationEventTarget(), on_hot_key, 1, &spec, std::ptr::null_mut(), &mut handler)
    };
    if status == NO_ERR { Ok(()) } else { Err(format!("InstallEventHandler failed with status {status}")) }
}

/// Registers the key and modifiers as a system wide hotkey.
fn register_hot_key(key_code: u32, modifiers: u32) -> Result<(), String> {
    let id = EventHotKeyId { signature: SIGNATURE, id: 1 };
    let mut hot_key: EventHotKeyRef = std::ptr::null_mut();
    // SAFETY: the target is the application's own and `hot_key` is a valid out pointer. The
    // registration lives until the process exits, so the returned reference is never released.
    let status = unsafe { RegisterEventHotKey(key_code, modifiers, id, GetApplicationEventTarget(), 0, &mut hot_key) };
    match status {
        NO_ERR => Ok(()),
        HOT_KEY_EXISTS_ERR => Err("the hotkey is taken by another program".into()),
        other => Err(format!("RegisterEventHotKey failed with status {other}")),
    }
}

/// Registers the chord as a global hotkey. Must be called on the main thread, before the event
/// loop runs, which is where `Gui::new` calls it.
pub fn listen(chord: &KeyChord, proxy: EventLoopProxy<UserEvent>) -> Result<(), String> {
    let code = keymap::mac_key_code(&chord.key).ok_or_else(|| format!("'{}' cannot be a global hotkey", chord.key))?;
    PROXY.set(proxy).map_err(|_| "the global hotkey is already registered".to_string())?;
    install_handler()?;
    register_hot_key(code, keymap::mac_modifiers(chord.mods))
}
