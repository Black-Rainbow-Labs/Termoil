//! The quick terminal hotkey on Wayland. No compositor lets a client grab a key, so this asks the
//! XDG desktop portal (`org.freedesktop.portal.GlobalShortcuts`) to bind the chord and listens for
//! its `Activated` signal. The portal is optional: GNOME and KDE ship it, several other compositors
//! do not, and then `listen` returns an error that names the alternative.

use std::collections::HashMap;

use winit::event_loop::EventLoopProxy;
use zbus::blocking::{Connection, Proxy};
use zbus::zvariant::{DynamicType, ObjectPath, OwnedObjectPath, OwnedValue, Value};

use super::keymap;
use crate::config::KeyChord;
use crate::gui::UserEvent;

const DESTINATION: &str = "org.freedesktop.portal.Desktop";
const PORTAL_PATH: &str = "/org/freedesktop/portal/desktop";
const SHORTCUTS: &str = "org.freedesktop.portal.GlobalShortcuts";
const REQUEST: &str = "org.freedesktop.portal.Request";
/// The id the portal reports back in `Activated` for the one shortcut Termoil binds.
const SHORTCUT_ID: &str = "quick-terminal";

/// The error returned when the desktop has no global shortcuts portal, with what to do instead.
pub fn no_portal_message(reason: &str) -> String {
    format!(
        "this desktop has no global shortcuts portal ({reason}), and a Wayland compositor does not let a program \
         grab a key itself; bind `termoil +quick-terminal` to a key in the desktop's own keyboard settings to get \
         the same result"
    )
}

/// Connects to the session bus and confirms the GlobalShortcuts portal answers on it.
fn connect_to_portal() -> Result<(Connection, Proxy<'static>), String> {
    let conn = Connection::session().map_err(|e| no_portal_message(&format!("no session bus: {e}")))?;
    let proxy =
        Proxy::new(&conn, DESTINATION, PORTAL_PATH, SHORTCUTS).map_err(|e| no_portal_message(&e.to_string()))?;
    proxy.get_property::<u32>("version").map_err(|e| no_portal_message(&e.to_string()))?;
    Ok((conn, proxy))
}

/// The object path the portal answers a request on: it is derived from the caller's bus name and
/// the handle token the caller chose.
fn request_path(conn: &Connection, token: &str) -> Result<OwnedObjectPath, String> {
    let name = conn.unique_name().ok_or("the session bus gave no unique name")?;
    let sender = name.as_str().trim_start_matches(':').replace('.', "_");
    let path = format!("/org/freedesktop/portal/desktop/request/{sender}/{token}");
    ObjectPath::try_from(path).map(Into::into).map_err(|e| e.to_string())
}

/// Calls a portal method that answers through a Request object and waits for its `Response`
/// signal. The subscription is made before the call so the answer cannot be missed.
fn portal_request<B: serde::Serialize + DynamicType>(
    conn: &Connection, portal: &Proxy<'_>, method: &str, token: &str, body: &B,
) -> Result<HashMap<String, OwnedValue>, String> {
    let path = request_path(conn, token)?;
    let request = Proxy::new(conn, DESTINATION, path, REQUEST).map_err(|e| e.to_string())?;
    let mut responses = request.receive_signal("Response").map_err(|e| e.to_string())?;
    portal.call_method(method, body).map_err(|e| format!("{method}: {e}"))?;
    let message = responses.next().ok_or("the portal closed without answering")?;
    let (code, results): (u32, HashMap<String, OwnedValue>) =
        message.body().deserialize().map_err(|e| e.to_string())?;
    match code {
        0 => Ok(results),
        1 => Err(format!("{method} was cancelled")),
        _ => Err(format!("{method} was refused by the desktop")),
    }
}

/// Opens a portal session and returns its handle.
fn create_session(conn: &Connection, portal: &Proxy<'_>) -> Result<OwnedObjectPath, String> {
    let options: HashMap<&str, Value<'_>> =
        HashMap::from([("handle_token", "termoil_create".into()), ("session_handle_token", "termoil_session".into())]);
    let results = portal_request(conn, portal, "CreateSession", "termoil_create", &(options,))?;
    let handle = results.get("session_handle").ok_or("the portal returned no session handle")?;
    let text = String::try_from(handle.try_clone().map_err(|e| e.to_string())?).map_err(|e| e.to_string())?;
    ObjectPath::try_from(text).map(Into::into).map_err(|e| e.to_string())
}

/// Asks the portal to bind the chord to the quick terminal shortcut. The desktop may show a dialog
/// and may pick another trigger; the user's choice stands.
fn bind_shortcut(
    conn: &Connection, portal: &Proxy<'_>, session: &OwnedObjectPath, trigger: &str,
) -> Result<(), String> {
    let details: HashMap<&str, Value<'_>> = HashMap::from([
        ("description", "Show or hide the Termoil quick terminal".into()),
        ("preferred_trigger", trigger.into()),
    ]);
    let options: HashMap<&str, Value<'_>> = HashMap::from([("handle_token", "termoil_bind".into())]);
    let shortcuts = vec![(SHORTCUT_ID, details)];
    portal_request(conn, portal, "BindShortcuts", "termoil_bind", &(session, shortcuts, "", options)).map(|_| ())
}

/// Binds the shortcut and then sends the user event for each `Activated` signal of it.
fn run(conn: &Connection, portal: &Proxy<'_>, trigger: &str, proxy: &EventLoopProxy<UserEvent>) -> Result<(), String> {
    let session = create_session(conn, portal)?;
    let mut activations = portal.receive_signal("Activated").map_err(|e| e.to_string())?;
    bind_shortcut(conn, portal, &session, trigger)?;
    for message in &mut activations {
        let body: Result<(OwnedObjectPath, String, u64, HashMap<String, OwnedValue>), _> = message.body().deserialize();
        if matches!(body, Ok((_, id, _, _)) if id == SHORTCUT_ID) && proxy.send_event(UserEvent::QuickTerminal).is_err()
        {
            break;
        }
    }
    Ok(())
}

/// Checks the portal is present, then binds the chord and listens on a thread of its own. The
/// binding itself can take a while when the desktop asks the user, so its failures are logged.
pub fn listen(chord: &KeyChord, proxy: EventLoopProxy<UserEvent>) -> Result<(), String> {
    let trigger = keymap::portal_trigger(chord).ok_or_else(|| format!("'{}' cannot be a global hotkey", chord.key))?;
    let (conn, portal) = connect_to_portal()?;
    std::thread::Builder::new()
        .name("termoil quick terminal hotkey".into())
        .spawn(move || {
            if let Err(e) = run(&conn, &portal, &trigger, &proxy) {
                eprintln!("termoil: the quick terminal hotkey portal failed: {e}");
            }
        })
        .map(|_| ())
        .map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_missing_portal_message_names_the_alternative() {
        let text = no_portal_message("no such interface");
        assert!(text.contains("no global shortcuts portal"));
        assert!(text.contains("no such interface"));
        assert!(text.contains("termoil +quick-terminal"));
    }
}
