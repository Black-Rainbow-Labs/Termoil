//! Desktop notifications on Linux: the freedesktop notification service
//! (`org.freedesktop.Notifications.Notify` on the session bus), called through zbus.

use std::collections::HashMap;

use zbus::blocking::{Connection, Proxy};
use zbus::zvariant::Value;

const DESTINATION: &str = "org.freedesktop.Notifications";
const PATH: &str = "/org/freedesktop/Notifications";
const APP_NAME: &str = "Terminus";
/// The icon name the .deb and the AppImage install under hicolor.
const ICON: &str = "terminus";
/// A timeout of -1 lets the notification daemon choose its own.
const DEFAULT_TIMEOUT: i32 = -1;

/// Sends one notification and returns an error string when there is no session bus or no
/// notification daemon on it.
pub fn show(title: &str, body: &str) -> Result<(), String> {
    let conn = Connection::session().map_err(|e| format!("no session bus: {e}"))?;
    let proxy = Proxy::new(&conn, DESTINATION, PATH, DESTINATION).map_err(|e| e.to_string())?;
    let actions: Vec<&str> = Vec::new();
    let hints: HashMap<&str, Value<'_>> = HashMap::new();
    let args = (APP_NAME, 0u32, ICON, title, body, actions, hints, DEFAULT_TIMEOUT);
    proxy.call_method("Notify", &args).map(|_| ()).map_err(|e| format!("no notification daemon answered: {e}"))
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;
    use std::sync::{Arc, Mutex};

    use zbus::zvariant::OwnedValue;

    use super::*;

    /// A stand in notification daemon that records what `Notify` was called with.
    struct FakeDaemon(Arc<Mutex<Vec<String>>>);

    #[zbus::interface(name = "org.freedesktop.Notifications")]
    impl FakeDaemon {
        /// Records the app name, icon, summary and body, and returns notification id 7.
        #[allow(clippy::too_many_arguments)]
        fn notify(
            &self, app: String, _replaces: u32, icon: String, summary: String, body: String, _actions: Vec<String>,
            _hints: HashMap<String, OwnedValue>, _timeout: i32,
        ) -> u32 {
            self.0.lock().unwrap().extend([app, icon, summary, body]);
            7
        }
    }

    /// True when some program already owns the notification service name on the session bus.
    fn daemon_present() -> bool {
        zbus::blocking::Connection::session()
            .ok()
            .and_then(|c| zbus::blocking::fdo::DBusProxy::new(&c).ok())
            .and_then(|p| p.name_has_owner(DESTINATION.try_into().unwrap()).ok())
            .unwrap_or(true)
    }

    /// One test, in order, because both parts share the session bus: with no daemon `show` returns
    /// an error, and with a stand in daemon it delivers the app name, icon, title and body.
    #[test]
    fn show_reports_a_missing_daemon_and_then_reaches_one() {
        if std::env::var_os("DBUS_SESSION_BUS_ADDRESS").is_none() || daemon_present() {
            return;
        }
        assert!(show("a", "b").is_err());
        let seen = Arc::new(Mutex::new(Vec::new()));
        let built = zbus::blocking::connection::Builder::session()
            .and_then(|b| b.name(DESTINATION))
            .and_then(|b| b.serve_at(PATH, FakeDaemon(seen.clone())))
            .and_then(|b| b.build());
        let _server = built.expect("the stand in daemon starts");
        show("Build finished", "all 12 tests passed").unwrap();
        assert_eq!(*seen.lock().unwrap(), ["Terminus", "terminus", "Build finished", "all 12 tests passed"]);
    }
}
