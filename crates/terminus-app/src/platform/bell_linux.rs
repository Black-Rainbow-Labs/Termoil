//! The audible bell on Linux. On X11 it is the core protocol Bell request; on Wayland it is the
//! `xdg_system_bell_v1` protocol when the compositor offers it, and silence when it does not (no
//! notification popup is used as a substitute). `ring` is called on the UI thread, so it only
//! queues a request: the connection is made once, on a thread of its own, and that thread rings.

use std::sync::OnceLock;
use std::sync::mpsc::{self, Sender};

use wayland_client::globals::{GlobalList, GlobalListContents, registry_queue_init};
use wayland_client::protocol::wl_registry::WlRegistry;
use wayland_client::{Connection, Dispatch, QueueHandle};
use wayland_protocols::xdg::system_bell::v1::client::xdg_system_bell_v1::XdgSystemBellV1;
use x11rb::connection::Connection as _;
use x11rb::protocol::xproto::ConnectionExt;
use x11rb::rust_connection::RustConnection;

/// The display server a session runs on, chosen the way winit chooses its backend.
#[derive(Debug, PartialEq, Eq)]
enum Backend {
    Wayland,
    X11,
    None,
}

/// Wayland when `WAYLAND_DISPLAY` is set, otherwise X11 when `DISPLAY` is set, otherwise none.
fn choose_backend(wayland_display_set: bool, display_set: bool) -> Backend {
    if wayland_display_set {
        Backend::Wayland
    } else if display_set {
        Backend::X11
    } else {
        Backend::None
    }
}

/// What the worker thread rings: an X11 connection or a Wayland bell object.
enum Bell {
    X11(Box<RustConnection>),
    Wayland(Connection, XdgSystemBellV1),
}

/// The state the Wayland event queue dispatches into; the bell needs no events.
struct WaylandState;

impl Dispatch<WlRegistry, GlobalListContents> for WaylandState {
    /// Registry events are not needed after the initial list of globals.
    fn event(
        _: &mut Self, _: &WlRegistry, _: <WlRegistry as wayland_client::Proxy>::Event, _: &GlobalListContents,
        _: &Connection, _: &QueueHandle<Self>,
    ) {
    }
}

impl Dispatch<XdgSystemBellV1, ()> for WaylandState {
    /// The bell interface has no events.
    fn event(
        _: &mut Self, _: &XdgSystemBellV1, _: <XdgSystemBellV1 as wayland_client::Proxy>::Event, _: &(),
        _: &Connection, _: &QueueHandle<Self>,
    ) {
    }
}

/// Connects to the compositor and binds `xdg_system_bell_v1`, or None when it is not offered.
fn connect_wayland() -> Option<Bell> {
    let conn = Connection::connect_to_env().ok()?;
    let (globals, queue): (GlobalList, _) = registry_queue_init::<WaylandState>(&conn).ok()?;
    let bell: XdgSystemBellV1 = globals.bind(&queue.handle(), 1..=1, ()).ok()?;
    Some(Bell::Wayland(conn, bell))
}

/// Connects to the X server.
fn connect_x11() -> Option<Bell> {
    x11rb::connect(None).ok().map(|(conn, _)| Bell::X11(Box::new(conn)))
}

/// Connects to whichever display server this session runs on.
fn connect() -> Option<Bell> {
    let set = |name: &str| std::env::var_os(name).is_some_and(|v| !v.is_empty());
    match choose_backend(set("WAYLAND_DISPLAY"), set("DISPLAY")) {
        Backend::Wayland => connect_wayland(),
        Backend::X11 => connect_x11(),
        Backend::None => None,
    }
}

/// Rings once at the default volume. Errors are ignored: a bell that cannot sound is not worth a log line each time.
fn ring_once(bell: &Bell) {
    match bell {
        Bell::X11(conn) => {
            let _ = conn.bell(0).and_then(|_| conn.flush());
        }
        Bell::Wayland(conn, bell) => {
            bell.ring(None);
            let _ = conn.flush();
        }
    }
}

/// The worker: connects once, then rings for each request, folding a burst of requests into one.
fn worker(requests: mpsc::Receiver<()>) {
    let bell = connect();
    while requests.recv().is_ok() {
        while requests.try_recv().is_ok() {}
        if let Some(bell) = &bell {
            ring_once(bell);
        }
    }
}

/// Starts the worker thread and returns the channel that feeds it.
fn start_worker() -> Sender<()> {
    let (tx, rx) = mpsc::channel();
    let _ = std::thread::Builder::new().name("terminus bell".into()).spawn(move || worker(rx));
    tx
}

/// Asks the worker thread to ring the bell. Returns at once.
pub fn ring() {
    static WORKER: OnceLock<Sender<()>> = OnceLock::new();
    let _ = WORKER.get_or_init(start_worker).send(());
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wayland_wins_over_x11() {
        assert_eq!(choose_backend(true, true), Backend::Wayland);
        assert_eq!(choose_backend(false, true), Backend::X11);
        assert_eq!(choose_backend(false, false), Backend::None);
    }

    #[test]
    fn ringing_never_panics_whatever_the_display() {
        ring();
        ring();
    }
}
