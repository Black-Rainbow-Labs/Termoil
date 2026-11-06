//! The update actions (`check-for-updates`, `list-updates`, `install-update`, `skip-update`) and the
//! automatic check at start. The network work runs as an [`Outcome::Later`] job, never on the thread
//! that owns the `App`. A job reaches the window layer through the [`Sink`] the runtime installs: to show
//! a notification, or to quit and hand over to the helper once a file has been downloaded and checked.

use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use serde_json::{Map, Value, json};
use termoil_protocol::ActionError;

use super::install::{self, HelperOptions, Prepared, Target};
use super::{Release, SHOWN, channel_name, current_version, fetch_channel, is_newer, parse_channel};
use crate::actions::{arg_str, req_str};
use crate::app::{App, Outcome};
use crate::config::{UpdateChannel, UpdateCheck};

/// What a job asks of the window layer.
pub enum Event {
    /// Show a desktop notification.
    Notify { title: String, body: String },
    /// Save the session, start the helper for this file and quit.
    Restart(Prepared, Target),
}

/// Where a job sends its events; set by the window and headless runtimes.
pub type Sink = Arc<dyn Fn(Event) + Send + Sync>;

/// The update state an `App` keeps: the sink and the newest version the last check found.
#[derive(Clone, Default)]
pub struct Updates {
    pub sink: Option<Sink>,
    pub last_found: Arc<Mutex<Option<String>>>,
}

impl Updates {
    /// Sends an event when a runtime is listening.
    fn send(&self, event: Event) {
        if let Some(s) = &self.sink {
            s(event);
        }
    }

    /// Shows a notification when a runtime is listening.
    fn notify(&self, title: &str, body: &str) {
        self.send(Event::Notify { title: title.into(), body: body.into() });
    }

    /// Records the newest version a check found.
    fn remember(&self, version: &str) {
        if let Ok(mut v) = self.last_found.lock() {
            *v = Some(version.to_string());
        }
    }
}

/// Runs one of the update actions.
pub fn dispatch(app: &mut App, name: &str, args: &Map<String, Value>) -> Outcome {
    let channel = match channel_arg(app, args) {
        Ok(c) => c,
        Err(e) => return Outcome::err(e),
    };
    let timeout = Duration::from_secs(app.config.updates.timeout_seconds.max(1));
    let updates = app.updates.clone();
    match name {
        "check-for-updates" => Outcome::Later(Box::new(move |announce| check(&updates, channel, timeout, announce))),
        "list-updates" => {
            let named = arg_str(args, "channel").is_some();
            Outcome::Later(Box::new(move |_| Ok(list(channel, named, timeout))))
        }
        "install-update" => {
            let request = InstallRequest {
                version: arg_str(args, "version").map(|v| v.trim().trim_start_matches('v').to_string()),
                channel: arg_str(args, "channel").map(|_| channel),
                restart: args.get("restart").is_none_or(|_| crate::actions::arg_bool(args, "restart")),
                timeout,
            };
            Outcome::Later(Box::new(move |announce| install_job(&updates, &request, announce)))
        }
        _ => Outcome::Done(skip(app, args)),
    }
}

/// The `channel` argument, or `updates.channel` when it is not given.
fn channel_arg(app: &App, args: &Map<String, Value>) -> Result<UpdateChannel, ActionError> {
    match arg_str(args, "channel") {
        None => Ok(app.config.updates.channel),
        Some(name) => parse_channel(name)
            .ok_or_else(|| ActionError::BadArgs(format!("--channel must be stable or nightly, not '{name}'"))),
    }
}

/// check-for-updates: asks the channel and answers what it found. A person who asked from the window is
/// told the answer either way.
fn check(updates: &Updates, channel: UpdateChannel, timeout: Duration, announce: bool) -> Result<Value, ActionError> {
    let list = match fetch_channel(channel, timeout) {
        Ok(l) => l,
        Err(e) => {
            if announce {
                updates.notify("Could not check for updates", &e);
            }
            return Err(ActionError::Failed(e));
        }
    };
    let newest = &list[0];
    updates.remember(&newest.version);
    let newer = is_newer(current_version(), &newest.version);
    if announce {
        let (title, body) = sentence(channel, newest, newer);
        updates.notify(&title, &body);
    }
    let mut answer = newest.to_json();
    if let Value::Object(m) = &mut answer {
        m.insert("current".into(), json!(current_version()));
        m.insert("channel".into(), json!(channel_name(channel)));
        m.insert("latest".into(), json!(newest.version));
        m.insert("newer".into(), json!(newer));
    }
    Ok(answer)
}

/// The notification for a check's answer.
fn sentence(channel: UpdateChannel, newest: &Release, newer: bool) -> (String, String) {
    if newer {
        (
            format!("Termoil {} is available", newest.version),
            format!(
                "You have {}. Run Install Update from the command palette, or termoil +install-update.",
                current_version()
            ),
        )
    } else {
        (
            "Termoil is up to date".into(),
            format!("{} is the newest version on the {} channel.", current_version(), channel_name(channel)),
        )
    }
}

/// list-updates: the newest versions on the named channel, or on both.
fn list(channel: UpdateChannel, named: bool, timeout: Duration) -> Value {
    let channels = if named { vec![channel] } else { vec![UpdateChannel::Stable, UpdateChannel::Nightly] };
    let mut out = Map::new();
    out.insert("current".into(), json!(current_version()));
    for ch in channels {
        let entry = match fetch_channel(ch, timeout) {
            Ok(list) => json!(list.iter().take(SHOWN).map(|r| {
                json!({ "version": r.version, "date": r.date, "notes": r.notes, "running": r.version == current_version() })
            }).collect::<Vec<_>>()),
            Err(e) => json!({ "error": e }),
        };
        out.insert(channel_name(ch).into(), entry);
    }
    Value::Object(out)
}

/// What install-update was asked for.
struct InstallRequest {
    version: Option<String>,
    channel: Option<UpdateChannel>,
    restart: bool,
    timeout: Duration,
}

/// install-update: finds the version, downloads and checks it, and then either answers the file or hands
/// over to the helper. Failures are told to a person who asked from the window.
fn install_job(updates: &Updates, req: &InstallRequest, announce: bool) -> Result<Value, ActionError> {
    let result = install_steps(updates, req, announce);
    if announce && let Err(e) = &result {
        updates.notify("Termoil could not install the update", &e.to_string());
    }
    result
}

/// The steps of install-update, without the notification. `announce` also opens the download page when
/// this Termoil cannot be replaced in place.
fn install_steps(updates: &Updates, req: &InstallRequest, announce: bool) -> Result<Value, ActionError> {
    let release = find_release(req)?;
    let target = match req.restart.then(install::target) {
        None => None,
        Some(Ok(t)) => Some(t),
        Some(Err(why)) => {
            if announce {
                crate::platform::open(super::RELEASES_PAGE);
            }
            return Err(not_in_place(&release, why));
        }
    };
    let kind = target.as_ref().map(Target::kind).unwrap_or(super::platform_kind());
    let prepared = install::download_checked(&release, kind, req.timeout).map_err(ActionError::Failed)?;
    let mut answer =
        json!({ "version": prepared.version, "file": prepared.file.to_string_lossy(), "restart": req.restart });
    let Some(target) = target else { return Ok(answer) };
    if updates.sink.is_some() {
        updates.send(Event::Restart(prepared, target));
        answer["restarting"] = json!(true);
    } else {
        let log = install::start_helper(&prepared, &target, &HelperOptions::default()).map_err(ActionError::Failed)?;
        answer["installing"] = json!(true);
        answer["log"] = json!(log.to_string_lossy());
    }
    Ok(answer)
}

/// The failure for a Termoil that cannot be replaced in place, naming the download.
fn not_in_place(release: &Release, why: String) -> ActionError {
    let url = release.download().map(|d| d.url.clone()).unwrap_or_else(|| super::RELEASES_PAGE.to_string());
    ActionError::Failed(format!("{why}. Termoil {} is at {url}", release.version))
}

/// The release to install: the named version on the named channel, or on either channel when none is
/// named; the newest on the channel when no version is named.
fn find_release(req: &InstallRequest) -> Result<Release, ActionError> {
    let channels = match req.channel {
        Some(c) => vec![c],
        None if req.version.is_some() => vec![UpdateChannel::Stable, UpdateChannel::Nightly],
        None => vec![UpdateChannel::Stable],
    };
    let mut problems = Vec::new();
    for ch in &channels {
        match fetch_channel(*ch, req.timeout) {
            Ok(list) => match &req.version {
                None => return Ok(list[0].clone()),
                Some(v) => {
                    if let Some(r) = list.into_iter().find(|r| &r.version == v) {
                        return Ok(r);
                    }
                }
            },
            Err(e) => problems.push(e),
        }
    }
    if !problems.is_empty() && req.version.is_none() {
        return Err(ActionError::Failed(problems.join("; ")));
    }
    let names: Vec<&str> = channels.iter().map(|c| channel_name(*c)).collect();
    Err(ActionError::NotFound(format!(
        "Termoil {} is not on the {} channel",
        req.version.as_deref().unwrap_or("?"),
        names.join(" or ")
    )))
}

/// skip-update: writes `updates.skip`, the named version or the one the last check found.
fn skip(app: &mut App, args: &Map<String, Value>) -> Result<Value, ActionError> {
    let version = match arg_str(args, "version") {
        Some(_) => req_str(args, "version")?.trim().trim_start_matches('v').to_string(),
        None => app
            .updates
            .last_found
            .lock()
            .ok()
            .and_then(|v| v.clone())
            .ok_or_else(|| ActionError::BadArgs("no check has found a version yet; give --version".into()))?,
    };
    if super::parse_version(&version).is_none() {
        return Err(ActionError::BadArgs(format!("'{version}' is not a version")));
    }
    let setting = crate::settings::all()
        .into_iter()
        .find(|s| s.path() == "updates.skip")
        .ok_or_else(|| ActionError::Failed("updates.skip is not in the schema".into()))?;
    crate::settings::save(app, &setting, &version).map_err(ActionError::Failed)?;
    Ok(json!({ "skip": version }))
}

/// Starts the automatic check when the first window opens: `daily` asks once a day across processes,
/// `start` every time, `off` never. A newer version that is not `updates.skip` is announced.
pub fn start_automatic(app: &App) {
    let cfg = &app.config.updates;
    if cfg.check == UpdateCheck::Off {
        return;
    }
    let folder = crate::paths::data_dir();
    if cfg.check == UpdateCheck::Daily {
        let Some(folder) = &folder else { return };
        let now = super::now_seconds();
        if !super::is_due(super::last_checked(folder), now) {
            return;
        }
        super::note_checked(folder, now);
    }
    let (channel, skip, updates) = (cfg.channel, cfg.skip.trim().to_string(), app.updates.clone());
    let timeout = Duration::from_secs(cfg.timeout_seconds.max(1));
    let _ = std::thread::Builder::new().name("termoil update check".into()).spawn(move || {
        let Ok(list) = fetch_channel(channel, timeout) else { return };
        let newest = &list[0];
        updates.remember(&newest.version);
        if is_newer(current_version(), &newest.version) && newest.version != skip {
            let (title, body) = sentence(channel, newest, true);
            updates.notify(&title, &body);
        }
    });
}

/// The session file an update writes before it quits, in the data folder.
pub fn update_session_path() -> Option<PathBuf> {
    crate::paths::data_dir().map(|d| d.join("update-session.json"))
}

/// Carries out a `Restart` event on the thread that owns the app: saves the session, starts the helper
/// waiting for this process, and answers whether the runtime should now quit.
pub fn restart(app: &App, prepared: &Prepared, target: &Target) -> Result<(), String> {
    let session = update_session_path().ok_or("there is no data folder to save the session in")?;
    let saved = crate::restore::save_session(app, Some(session.clone()), None).is_ok() && !app.model.windows.is_empty();
    let opts = HelperOptions { wait_pid: Some(std::process::id()), session: saved.then_some(session), launch: true };
    install::start_helper(prepared, target, &opts).map(|_| ())
}
