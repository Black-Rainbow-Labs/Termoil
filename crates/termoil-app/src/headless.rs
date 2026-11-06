//! The headless runtime: the same `App` and IPC server with no windows. `termoil-gui --headless`
//! runs it, which is how the E5 tests get "a running instance started for the test" on a machine
//! with or without a display, and how a script can drive sessions it never shows.

use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use crossbeam_channel::{Receiver, Sender, unbounded};
use termoil_protocol::ActionCall;

use crate::app::{App, Effect, MemoryClipboard, Outcome, WakerFactory};
use crate::config::Config;
use crate::ipc_server::{ServeError, bridge, start};
use crate::model::PaneId;

/// A message to the thread that owns the app.
enum Msg {
    /// A pane has output or events; the flag is cleared when handled.
    Wake(PaneId, Arc<AtomicBool>),
    /// An IPC request and where to send its outcome.
    Ipc(ActionCall, Sender<Outcome>),
    /// An update job's event: a notification, printed here, or a checked update to install.
    Update(crate::update::actions::Event),
}

/// Makes wakers that send one `Wake` per burst of output: the flag stays set until the app thread
/// has handled the wake, so a flood of output does not flood the channel.
pub fn coalescing_wakers<M: Send + 'static>(tx: Sender<M>, make: fn(PaneId, Arc<AtomicBool>) -> M) -> WakerFactory {
    Box::new(move |id| {
        let tx = tx.clone();
        let pending = Arc::new(AtomicBool::new(false));
        Arc::new(move || {
            if !pending.swap(true, Ordering::AcqRel) {
                let _ = tx.send(make(id, pending.clone()));
            }
        })
    })
}

/// Runs the headless instance on this thread until a `quit` action. Errors when another instance
/// already serves the socket. `first_window` opens a window at start (a separate instance does,
/// with its cwd and command); the primary headless instance starts with none.
pub fn run(
    config: Config, config_path: PathBuf, socket: &str, first_window: Option<(Option<PathBuf>, Option<String>)>,
) -> Result<(), String> {
    let (tx, rx) = unbounded::<Msg>();
    let wakers = coalescing_wakers(tx.clone(), Msg::Wake);
    let mut app = App::new(config, config_path, Box::new(MemoryClipboard::default()), wakers);
    let update_tx = tx.clone();
    app.updates.sink = Some(Arc::new(move |ev| {
        let _ = update_tx.send(Msg::Update(ev));
    }));
    let ipc_tx = tx.clone();
    let handler = bridge(move |call, reply| ipc_tx.send(Msg::Ipc(call, reply)).is_ok());
    match start(socket, handler) {
        Ok(()) => {}
        Err(ServeError::AlreadyRunning) => return Err(format!("another Termoil already serves {socket}")),
        Err(ServeError::Io(e)) => return Err(format!("could not serve {socket}: {e}")),
    }
    drop(tx);
    let env = crate::telemetry::environment();
    let permission = crate::telemetry::Permission::for_the_binary(true, false, env.as_deref());
    crate::telemetry::start(&app.config, permission, "headless");
    if let Some((cwd, command)) = first_window {
        crate::actions::new_window(&mut app, cwd, command.as_deref()).map_err(|e| e.to_string())?;
        let _ = app.take_effects();
    } else if !crate::app::instance_mode().separate {
        crate::restore::restore_on_start(&mut app);
        let _ = app.take_effects();
    }
    serve(&mut app, &rx);
    crate::restore::save_on_quit(&app, None);
    for id in app.panes.keys().copied().collect::<Vec<_>>() {
        app.close_pane(id);
    }
    Ok(())
}

/// Handles messages until a `quit`.
fn serve(app: &mut App, rx: &Receiver<Msg>) {
    while let Ok(msg) = rx.recv() {
        match msg {
            Msg::Wake(pane, flag) => {
                flag.store(false, Ordering::Release);
                crate::events::pump(app, pane, true);
            }
            Msg::Ipc(call, reply) => {
                crate::telemetry::note_action(crate::telemetry::Category::Ipc, &call.action);
                let _ = reply.send(app.run_action(&call));
            }
            Msg::Update(crate::update::actions::Event::Notify { title, body }) => eprintln!("termoil: {title}: {body}"),
            Msg::Update(crate::update::actions::Event::Restart(prepared, target)) => {
                match crate::update::actions::restart(app, &prepared, &target) {
                    Ok(()) => return,
                    Err(e) => eprintln!("termoil: could not install the update: {e}"),
                }
            }
        }
        if apply_effects(app) {
            return;
        }
    }
}

/// Applies the effects that mean something without a window. Returns true on `quit`.
fn apply_effects(app: &mut App) -> bool {
    let mut quit = false;
    for e in app.take_effects() {
        match e {
            Effect::SetClipboard(t) => app.clipboard.set(&t),
            Effect::Quit => quit = true,
            Effect::ConfigChanged => crate::telemetry::configure(&app.config),
            _ => {}
        }
    }
    quit
}
