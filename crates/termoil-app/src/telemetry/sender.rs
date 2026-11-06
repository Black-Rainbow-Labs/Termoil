//! The thread that decides when to send, posts, and keeps the spool.
//!
//! **Nothing on the window's thread waits for the network.** The `termoil telemetry` thread waits on its
//! job channel with a timeout of the time until the next request is due, builds the body itself from the
//! process wide tally and clock, and posts it with curl. Closing waits a bounded time, and only after the
//! closing bodies are on disk.
//!
//! **What could not be posted is kept.** A failed body is appended to `spool.jsonl`, and the next process
//! that starts sends it. A starting process claims the spool by renaming it to `sending-<pid>.jsonl`, and a
//! `sending-` file whose process is no longer running is claimed again.

use std::path::{Path, PathBuf};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use serde_json::{Value, json};

use super::{FIRST_AFTER, MOST_REQUESTS, Opening, Permission, payload};

/// How long one request may take.
const REQUEST_TIMEOUT: Duration = Duration::from_secs(10);
/// GA4 refuses an event whose `timestamp_micros` is older than this.
const OLDEST: u64 = 72 * 60 * 60 * 1_000_000;
/// The most lines a spool keeps; the oldest go first.
const SPOOL_LINES: usize = 200;
/// How soon to look again when a request was due but there was nothing to say.
const RECHECK: Duration = Duration::from_secs(60);

/// What the process asks the thread to do.
enum Job {
    /// Send what is waiting now and answer the status.
    Flush(mpsc::Sender<Value>),
    /// Use a new interval.
    Interval(Duration),
    /// Send the closing request and say when that is done.
    Close(mpsc::Sender<()>),
}

/// What the thread has done, for `telemetry-status`.
#[derive(Debug, Clone, Default)]
pub struct Status {
    pub requests: u32,
    pub posted: u32,
    pub failed: u32,
    pub last_problem: Option<String>,
    pub client_id: String,
    pub debug: bool,
}

/// Where the thread posts and spools.
#[derive(Debug, Clone)]
pub struct Target {
    /// The full address, with the measurement id and the API secret in its query.
    pub url: String,
    /// The telemetry folder.
    pub folder: PathBuf,
    /// Whether every body is also appended to `sent.jsonl`.
    pub keep_a_copy: bool,
}

impl Target {
    /// The real endpoint, or `TERMOIL_TELEMETRY_URL` when a test names one.
    ///
    /// @param folder - the telemetry folder
    /// @param keep_a_copy - whether to write every body to `sent.jsonl`
    pub fn for_the_binary(folder: PathBuf, keep_a_copy: bool) -> Target {
        let base = std::env::var("TERMOIL_TELEMETRY_URL").unwrap_or_else(|_| super::ENDPOINT.to_owned());
        let url = format!("{base}?measurement_id={}&api_secret={}", super::MEASUREMENT_ID, super::API_SECRET);
        Target { url, folder, keep_a_copy }
    }

    /// The spool file.
    fn spool(&self) -> PathBuf {
        self.folder.join("spool.jsonl")
    }
}

/// The process's handle on the thread.
struct Handle {
    jobs: mpsc::Sender<Job>,
    status: Arc<Mutex<Status>>,
}

/// The running thread, once started.
static RUNNING: Mutex<Option<Handle>> = Mutex::new(None);
/// Why the process may not send, when it may not.
static REFUSED: Mutex<Option<Permission>> = Mutex::new(None);

/// The thread's own state: when it last sent and what is still to say.
struct Schedule {
    context: payload::Context,
    opening: Option<Opening>,
    started: Instant,
    last_send: Instant,
    every: Duration,
    requests: u32,
}

/// Records that this process may not send, for `telemetry-status`.
///
/// @param why - the reason
pub(crate) fn refuse(why: Permission) {
    if let Ok(mut refused) = REFUSED.lock() {
        *refused = Some(why);
    }
}

/// Whether the thread was started.
pub fn is_running() -> bool {
    RUNNING.lock().map(|r| r.is_some()).unwrap_or(false)
}

/// Starts the thread. It first sends whatever an earlier process left in the spool.
///
/// @param target - where it posts and spools
/// @param context - the ids and facts every request carries
/// @param opening - what `app_open` says
/// @param every - how often a used process sends
pub(crate) fn start(target: Target, context: payload::Context, opening: Opening, every: Duration) {
    let Ok(mut running) = RUNNING.lock() else { return };
    if running.is_some() {
        return;
    }
    let (jobs, inbox) = mpsc::channel();
    let status = Arc::new(Mutex::new(Status {
        client_id: context.client_id.clone(),
        debug: context.debug,
        ..Status::default()
    }));
    let thread_status = status.clone();
    let now = Instant::now();
    let schedule = Schedule { context, opening: Some(opening), started: now, last_send: now, every, requests: 0 };
    let spawned = std::thread::Builder::new()
        .name("termoil telemetry".into())
        .spawn(move || run(target, schedule, inbox, thread_status));
    if spawned.is_ok() {
        *running = Some(Handle { jobs, status });
    }
}

/// Sends a job to the thread, when it runs.
fn send_job(job: Job) -> bool {
    RUNNING.lock().ok().and_then(|r| r.as_ref().map(|h| h.jobs.send(job).is_ok())).unwrap_or(false)
}

/// Changes how often a used process sends.
///
/// @param every - the new interval
pub(crate) fn set_interval(every: Duration) {
    send_job(Job::Interval(every));
}

/// Sends the closing request and waits at most `wait` for the thread to post it.
///
/// @param wait - the longest the process waits
pub(crate) fn close(wait: Duration) {
    let (done, finished) = mpsc::channel();
    if send_job(Job::Close(done)) {
        let _ = finished.recv_timeout(wait);
    }
}

/// What `telemetry-status` prints, after sending what is waiting when `flush` is true.
///
/// @param flush - whether to send now
pub fn status(flush: bool) -> Value {
    let refused = REFUSED.lock().ok().and_then(|r| *r);
    let reason = match (refused, super::is_on(), is_running()) {
        (Some(why), _, _) => Some(why.name()),
        (None, _, false) => Some("not started"),
        (None, false, true) => Some("setting"),
        (None, true, true) => None,
    };
    let mut report = json!({ "enabled": reason.is_none(), "offBecause": reason, "waiting": super::waiting() });
    if flush && reason.is_none() {
        let (reply, answer) = mpsc::channel();
        if send_job(Job::Flush(reply))
            && let Ok(after) = answer.recv_timeout(REQUEST_TIMEOUT * 5)
        {
            report["flushed"] = after;
        }
    }
    if let Some(s) = RUNNING.lock().ok().and_then(|r| r.as_ref().and_then(|h| h.status.lock().ok().map(|s| s.clone())))
    {
        report["clientId"] = json!(s.client_id);
        report["requests"] = json!(s.requests);
        report["posted"] = json!(s.posted);
        report["failed"] = json!(s.failed);
        report["lastProblem"] = json!(s.last_problem);
        report["debug"] = json!(s.debug);
    }
    report
}

/// The thread: leftover spools first, then requests as they fall due and jobs as they arrive.
fn run(target: Target, mut schedule: Schedule, inbox: Receiver<Job>, status: Arc<Mutex<Status>>) {
    for claimed in claim_the_leftovers(&target.folder, std::process::id()) {
        drain(&target, &claimed, &status);
    }
    let mut wait = FIRST_AFTER;
    loop {
        match inbox.recv_timeout(wait) {
            Ok(Job::Flush(reply)) => {
                send_now(&target, &mut schedule, &status);
                let _ = reply.send(json!({ "requests": schedule.requests }));
            }
            Ok(Job::Interval(every)) => schedule.every = every,
            Ok(Job::Close(done)) => {
                close_now(&target, &mut schedule, &status);
                let _ = done.send(());
                return;
            }
            Err(RecvTimeoutError::Timeout) => {
                if is_due(&mut schedule, Instant::now()) {
                    send_now(&target, &mut schedule, &status);
                }
            }
            Err(RecvTimeoutError::Disconnected) => return,
        }
        wait = until_next(&schedule, Instant::now());
    }
}

/// How long to wait before looking again.
fn until_next(schedule: &Schedule, now: Instant) -> Duration {
    let at = match schedule.opening {
        Some(_) => schedule.started + FIRST_AFTER,
        None => schedule.last_send + schedule.every,
    };
    at.checked_duration_since(now).filter(|d| !d.is_zero()).unwrap_or(RECHECK)
}

/// Whether a request is due: the first one ten seconds in, then one every interval while something was
/// counted or somebody was active.
fn is_due(schedule: &mut Schedule, now: Instant) -> bool {
    if !super::is_on() || schedule.requests >= MOST_REQUESTS {
        return false;
    }
    if schedule.opening.is_some() {
        return now.duration_since(schedule.started) >= FIRST_AFTER;
    }
    let engaged = super::with_activity(|a| a.engaged_since_send(now)).unwrap_or(false);
    now.duration_since(schedule.last_send) >= schedule.every && (engaged || super::waiting() > 0)
}

/// The events since the last request: `app_open` and `app_crash` when they have not gone, then the
/// heartbeat.
fn events_since_the_last(schedule: &mut Schedule, now: Instant) -> Vec<Value> {
    let mut events = Vec::new();
    if let Some(opening) = schedule.opening.take() {
        events.push(payload::app_open(&opening));
        if opening.crashed {
            events.push(payload::event("app_crash", json!({ "uses": 1 })));
        }
    }
    let engaged = super::with_activity(|a| a.take_engagement(now)).unwrap_or_default();
    events.push(payload::heartbeat(engaged.as_millis() as u64));
    events
}

/// Sends what is waiting, when telemetry is on.
fn send_now(target: &Target, schedule: &mut Schedule, status: &Mutex<Status>) {
    if !super::is_on() {
        return;
    }
    let now = Instant::now();
    schedule.last_send = now;
    let mut events = events_since_the_last(schedule, now);
    let (rows, left) = payload::feature_rows(super::take_tally(), payload::MOST_EVENTS - events.len());
    events.extend(rows);
    super::put_back(left);
    for body in payload::bodies(&schedule.context, events, payload::now_micros()) {
        if schedule.requests >= MOST_REQUESTS {
            break;
        }
        schedule.requests += 1;
        set_requests(status, schedule.requests);
        if !post_and_count(target, &body, status) {
            append(&target.spool(), &[body]);
        }
    }
}

/// The closing request: `app_close` and everything counted, written to the spool first and then posted.
fn close_now(target: &Target, schedule: &mut Schedule, status: &Mutex<Status>) {
    if !super::is_on() {
        return;
    }
    let now = Instant::now();
    let mut events = events_since_the_last(schedule, now);
    let (open, focused, active) = super::with_activity(|a| a.totals(now)).unwrap_or_default();
    let totals = (open.as_secs(), focused.as_secs(), active.as_secs());
    events.push(payload::app_close(totals, super::peaks()));
    let (rows, _) = payload::feature_rows(super::take_tally(), payload::MOST_EVENTS * 2);
    events.extend(rows);
    let bodies = payload::bodies(&schedule.context, events, payload::now_micros());
    schedule.requests += bodies.len() as u32;
    set_requests(status, schedule.requests);
    append(&target.spool(), &bodies);
    let claimed = target.folder.join(format!("sending-{}.jsonl", std::process::id()));
    if std::fs::rename(target.spool(), &claimed).is_ok() {
        drain(target, &claimed, status);
    }
}

/// Records how many requests the process has made.
fn set_requests(status: &Mutex<Status>, requests: u32) {
    if let Ok(mut s) = status.lock() {
        s.requests = requests;
    }
}

/// Posts one body and records how it went. True when GA4 took it.
fn post_and_count(target: &Target, body: &str, status: &Mutex<Status>) -> bool {
    let answer = crate::update::fetch::post_json(&target.url, body, REQUEST_TIMEOUT);
    if target.keep_a_copy {
        append(&target.folder.join("sent.jsonl"), &[body.to_owned()]);
    }
    if let Ok(mut s) = status.lock() {
        match &answer {
            Ok(()) => s.posted += 1,
            Err(problem) => {
                s.failed += 1;
                s.last_problem = Some(problem.clone());
            }
        }
    }
    answer.is_ok()
}

/// Renames the spool and every `sending-` file whose process has gone to a name this process owns, and
/// answers the new names.
///
/// @param folder - the telemetry folder
/// @param pid - this process's id
pub fn claim_the_leftovers(folder: &Path, pid: u32) -> Vec<PathBuf> {
    let Ok(entries) = std::fs::read_dir(folder) else { return Vec::new() };
    let mut claimed = Vec::new();
    for (n, entry) in entries.flatten().enumerate() {
        let name = entry.file_name().to_string_lossy().into_owned();
        let owner = name.strip_prefix("sending-").and_then(|r| r.split(['-', '.']).next()).and_then(|p| p.parse().ok());
        let abandoned = owner.is_some_and(|other: u32| other != pid && !crate::update::apply::is_running(other));
        if name != "spool.jsonl" && !abandoned {
            continue;
        }
        let to = folder.join(format!("sending-{pid}-{n}.jsonl"));
        if std::fs::rename(entry.path(), &to).is_ok() {
            claimed.push(to);
        }
    }
    claimed
}

/// Posts every line of a claimed file GA4 will still take; what fails goes back to the spool. The claimed
/// file is removed.
fn drain(target: &Target, file: &Path, status: &Mutex<Status>) {
    let lines = read_lines(file);
    let _ = std::fs::remove_file(file);
    let failed: Vec<String> =
        still_wanted(lines, payload::now_micros()).into_iter().filter(|l| !post_and_count(target, l, status)).collect();
    if !failed.is_empty() {
        append(&target.spool(), &failed);
        cut_the_spool(&target.spool());
    }
}

/// The lines GA4 will still take: younger than 72 hours, the newest [`SPOOL_LINES`] of them.
///
/// @param lines - every line of a spool
/// @param now - the time now, in microseconds
pub fn still_wanted(lines: Vec<String>, now: u64) -> Vec<String> {
    let young: Vec<String> = lines
        .into_iter()
        .filter(|line| payload::timestamp_of(line).is_some_and(|at| now.saturating_sub(at) < OLDEST))
        .collect();
    let skip = young.len().saturating_sub(SPOOL_LINES);
    young.into_iter().skip(skip).collect()
}

/// Keeps the spool to its newest [`SPOOL_LINES`] lines.
fn cut_the_spool(file: &Path) {
    let lines = read_lines(file);
    if lines.len() > SPOOL_LINES {
        let keep: String = lines[lines.len() - SPOOL_LINES..].iter().map(|l| format!("{l}\n")).collect();
        let _ = std::fs::write(file, keep);
    }
}

/// The non-empty lines of a file.
fn read_lines(file: &Path) -> Vec<String> {
    std::fs::read_to_string(file)
        .map(|text| text.lines().filter(|line| !line.trim().is_empty()).map(str::to_owned).collect())
        .unwrap_or_default()
}

/// Appends lines to a file, making its folder when it is missing.
fn append(file: &Path, lines: &[String]) {
    use std::io::Write;
    if lines.is_empty() {
        return;
    }
    if let Some(folder) = file.parent() {
        let _ = std::fs::create_dir_all(folder);
    }
    if let Ok(mut out) = std::fs::OpenOptions::new().create(true).append(true).open(file) {
        for line in lines {
            let _ = writeln!(out, "{line}");
        }
    }
}
