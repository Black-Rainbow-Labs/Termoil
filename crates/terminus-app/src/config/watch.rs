//! Live reload: watches the config file and reports each change, debounced, as a freshly parsed config.
//!
//! The file is polled for its modification time and length rather than watched through the operating
//! system's change notifications. The `notify` crate is CC0 licensed, which the dependency allow list
//! does not include, and one `metadata` call every quarter second on one file costs nothing measurable.
//! Polling also handles every way an editor saves (in place, by rename, by delete and create) without
//! special cases, and a file created after start.

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant, SystemTime};

use super::{Config, ConfigError, Warning};

/// The debounce the application uses: one second of quiet after the last change.
pub const DEFAULT_DEBOUNCE: Duration = Duration::from_secs(1);

/// How often the file is checked.
pub const POLL_INTERVAL: Duration = Duration::from_millis(250);

/// What the callback receives: the new config and its warnings, or the parse error so the application can keep
/// the previous config and show a banner.
pub type LoadResult = Result<(Config, Vec<Warning>), ConfigError>;

/// A running watch on a config file. Dropping it stops the polling thread.
pub struct Watcher {
    stop: Arc<AtomicBool>,
    worker: Option<JoinHandle<()>>,
}

/// What identifies a version of the file: its modification time and length, or None when it is missing.
type Stamp = Option<(SystemTime, u64)>;

/// Reads the file's stamp.
fn stamp(path: &Path) -> Stamp {
    let m = std::fs::metadata(path).ok()?;
    Some((m.modified().ok()?, m.len()))
}

/// The polling loop: when the stamp changes, wait until it has stayed the same for `debounce`, then load the
/// file and call `on_change`.
fn run_worker(
    path: PathBuf, debounce: Duration, poll: Duration, stop: Arc<AtomicBool>, on_change: impl Fn(LoadResult),
) {
    let mut seen = stamp(&path);
    let mut pending: Option<(Stamp, Instant)> = None;
    while !stop.load(Ordering::Acquire) {
        thread::sleep(poll);
        let now = stamp(&path);
        match pending {
            Some((s, since)) if s == now && since.elapsed() >= debounce => {
                pending = None;
                seen = now;
                on_change(Config::load(&path));
            }
            Some((s, _)) if s == now => {}
            _ if now != seen || pending.is_some() => pending = Some((now, Instant::now())),
            _ => {}
        }
    }
}

impl Watcher {
    /// Starts watching `path` (its folder is created when missing). After a change to the file and `debounce`
    /// of quiet, `on_change` is called on a background thread with the file parsed again.
    pub fn start(
        path: PathBuf, debounce: Duration, on_change: impl Fn(LoadResult) + Send + 'static,
    ) -> Result<Watcher, String> {
        let folder = path
            .parent()
            .map(PathBuf::from)
            .filter(|p| !p.as_os_str().is_empty())
            .unwrap_or_else(|| PathBuf::from("."));
        std::fs::create_dir_all(&folder).map_err(|e| format!("cannot create {}: {e}", folder.display()))?;
        let stop = Arc::new(AtomicBool::new(false));
        let flag = stop.clone();
        let poll = POLL_INTERVAL.min(debounce.max(Duration::from_millis(10)));
        let worker = thread::Builder::new()
            .name("terminus config watcher".into())
            .spawn(move || run_worker(path, debounce, poll, flag, on_change))
            .map_err(|e| format!("cannot start the config watcher: {e}"))?;
        Ok(Watcher { stop, worker: Some(worker) })
    }
}

impl Drop for Watcher {
    /// Stops the polling thread and waits for it.
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Release);
        if let Some(w) = self.worker.take() {
            let _ = w.join();
        }
    }
}
