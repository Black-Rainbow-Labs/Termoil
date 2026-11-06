//! GA4 Measurement Protocol bodies. Pure, so every limit is a test with no network behind it.
//!
//! The limits are GA4's own: 25 events a request, an event name of 40 characters, 25 parameters an event,
//! a parameter name of 40 characters and a string value of 100. A body that breaks one is answered with a
//! 2xx and silently dropped, so the only place to catch it is here.

use std::collections::BTreeMap;

use serde_json::{Map, Value, json};

use super::{Category, Opening, Peaks};

/// The most events one request may carry.
pub const MOST_EVENTS: usize = 25;
/// The most requests one send may become. What does not fit waits for the next one.
pub const MOST_REQUESTS_AT_ONCE: usize = 4;

/// The ids and facts every request carries.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Context {
    pub client_id: String,
    /// The process's session: the Unix time in seconds it started, as GA4's own tag makes it.
    pub session_id: u64,
    pub version: String,
    pub debug: bool,
}

impl Context {
    /// The context for this process.
    ///
    /// @param client_id - this installation's id
    /// @param debug - whether to ask GA4 to show the events in DebugView
    pub fn for_this_process(client_id: String, debug: bool) -> Context {
        let session_id = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|since| since.as_secs())
            .unwrap_or_default();
        Context { client_id, session_id, version: termoil_protocol::VERSION.to_owned(), debug }
    }
}

/// The time now in microseconds since the Unix epoch, which is what `timestamp_micros` is.
pub fn now_micros() -> u64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|s| s.as_micros() as u64).unwrap_or_default()
}

/// The operating system's name as GA4's device reports spell it.
pub fn operating_system() -> &'static str {
    match std::env::consts::OS {
        "windows" => "Windows",
        "macos" => "Macintosh",
        "linux" => "Linux",
        _ => "Other",
    }
}

/// One event, its name cut to 40 characters, its parameter names to 40 and every string value to 100,
/// with room left for `session_id` and `engagement_time_msec`.
///
/// @param name - the event's name
/// @param params - its parameters, a JSON object
pub fn event(name: &str, params: Value) -> Value {
    let mut cut = Map::new();
    if let Value::Object(params) = params {
        for (key, value) in params.into_iter().take(MOST_EVENTS - 3) {
            let value = match value {
                Value::String(text) => Value::String(text.chars().take(100).collect()),
                other => other,
            };
            cut.insert(key.chars().take(40).collect(), value);
        }
    }
    json!({ "name": name.chars().take(40).collect::<String>(), "params": Value::Object(cut) })
}

/// The `app_open` event.
///
/// @param opening - what the process says about itself
pub fn app_open(opening: &Opening) -> Value {
    event(
        "app_open",
        json!({
            "launch": opening.launch,
            "renderer": opening.renderer,
            "theme": opening.theme,
            "channel": opening.channel,
            "first_run": u8::from(opening.first_run),
        }),
    )
}

/// The `app_heartbeat` event, carrying the active time since the last request.
///
/// @param engaged_ms - active milliseconds since the last request
pub fn heartbeat(engaged_ms: u64) -> Value {
    let mut beat = event("app_heartbeat", json!({}));
    beat["params"]["engagement_time_msec"] = json!(engaged_ms.max(1));
    beat
}

/// The `app_close` event.
///
/// @param totals - open, focused and active seconds
/// @param peaks - the largest layout seen
pub fn app_close(totals: (u64, u64, u64), peaks: Peaks) -> Value {
    event(
        "app_close",
        json!({
            "open_seconds": totals.0,
            "focused_seconds": totals.1,
            "active_seconds": totals.2,
            "most_windows": peaks.windows,
            "most_tabs": peaks.tabs,
            "most_panes": peaks.panes,
        }),
    )
}

/// One `feature` event per counted name, most used first, and what did not fit.
///
/// @param tally - every name counted, by category
/// @param room - how many events there is room for
pub fn feature_rows(
    tally: BTreeMap<(Category, String), u32>, room: usize,
) -> (Vec<Value>, BTreeMap<(Category, String), u32>) {
    let mut rows: Vec<((Category, String), u32)> = tally.into_iter().collect();
    rows.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(&b.0)));
    let room = room.min(MOST_EVENTS * MOST_REQUESTS_AT_ONCE);
    let left: BTreeMap<(Category, String), u32> = rows.split_off(room.min(rows.len())).into_iter().collect();
    let events = rows
        .into_iter()
        .map(|((category, name), uses)| {
            event("feature", json!({ "category": category.name(), "feature": name, "uses": uses }))
        })
        .collect();
    (events, left)
}

/// The request bodies for a list of events: 25 a body, at most [`MOST_REQUESTS_AT_ONCE`] bodies, each
/// event given the session and an engagement time, each body the client id, user properties and device.
///
/// @param context - the ids and facts about this process
/// @param events - the events, in order
/// @param timestamp - when they happened, in microseconds since the epoch
pub fn bodies(context: &Context, events: Vec<Value>, timestamp: u64) -> Vec<String> {
    let events: Vec<Value> = events
        .into_iter()
        .map(|mut one| {
            let params = &mut one["params"];
            params["session_id"] = json!(context.session_id.to_string());
            if params.get("engagement_time_msec").is_none() {
                params["engagement_time_msec"] = json!(1);
            }
            if context.debug {
                params["debug_mode"] = json!(1);
            }
            one
        })
        .collect();
    events.chunks(MOST_EVENTS).take(MOST_REQUESTS_AT_ONCE).map(|chunk| body(context, chunk, timestamp)).collect()
}

/// One request body.
fn body(context: &Context, events: &[Value], timestamp: u64) -> String {
    json!({
        "client_id": context.client_id,
        "timestamp_micros": timestamp,
        "user_properties": {
            "app_version": { "value": context.version },
            "os": { "value": std::env::consts::OS },
            "arch": { "value": std::env::consts::ARCH },
        },
        "device": { "category": "desktop", "operating_system": operating_system() },
        "events": events,
    })
    .to_string()
}

/// The `timestamp_micros` a spooled body was made at, which decides whether GA4 will still take it.
///
/// @param body - a request body
pub fn timestamp_of(body: &str) -> Option<u64> {
    serde_json::from_str::<Value>(body).ok()?.get("timestamp_micros")?.as_u64()
}
