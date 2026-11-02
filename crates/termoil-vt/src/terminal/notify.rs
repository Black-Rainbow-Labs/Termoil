//! OSC 99, kitty desktop notifications, as the published specification describes them: an
//! identifier `i`, chunks joined until `d` is not 0, payload types `title`, `body`, `close`,
//! `alive` and `?`, and Base64 payloads (`e=1`).

use super::osc::{decode_base64, terminator};
use super::{Event, Terminal};
use crate::parser::Terminator;

/// A notification whose chunks are still arriving.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(super) struct PendingNotification {
    id: String,
    title: String,
    body: String,
}

/// The metadata of one OSC 99 sequence that Termoil acts on.
struct Meta {
    id: String,
    done: bool,
    base64: bool,
    payload_type: String,
}

impl Terminal {
    /// OSC 99: handles one chunk of a kitty notification, a query, a close request or an alive
    /// poll. `rest` is `metadata;payload`.
    pub(super) fn osc_kitty_notification(&mut self, rest: &[u8], term: Terminator) {
        let text = String::from_utf8_lossy(rest).into_owned();
        let Some((meta, payload)) = text.split_once(';') else { return };
        let meta = parse_meta(meta);
        match meta.payload_type.as_str() {
            "?" => self.notification_query(&meta, term),
            "alive" => self.notification_alive(&meta, term),
            "close" => self.notification_pending.retain(|n| n.id != meta.id || meta.id.is_empty()),
            "title" | "body" => self.notification_chunk(&meta, payload),
            _ => self.notification_finish_if_done(&meta),
        }
    }

    /// Answers `p=?` with the metadata keys Termoil supports: the payload types it handles and
    /// that it shows notifications always. It has no action, sound, urgency or expiry support.
    fn notification_query(&mut self, meta: &Meta, term: Terminator) {
        let id = reply_id(&meta.id);
        let reply = format!("\x1b]99;i={id}:p=?;p=title,body,close,alive,?:o=always{}", terminator(term));
        self.reply(reply.as_bytes());
    }

    /// Answers `p=alive` with the identifiers of notifications Termoil is still showing, which is
    /// none, because the host owns a notification once it is sent.
    fn notification_alive(&mut self, meta: &Meta, term: Terminator) {
        let reply = format!("\x1b]99;i={}:p=alive;{}", reply_id(&meta.id), terminator(term));
        self.reply(reply.as_bytes());
    }

    /// Adds a title or body chunk to the pending notification with this identifier, then sends the
    /// notification when the chunk says it is done.
    fn notification_chunk(&mut self, meta: &Meta, payload: &str) {
        let text = if meta.base64 {
            decode_base64(payload.as_bytes()).map(|b| String::from_utf8_lossy(&b).into_owned()).unwrap_or_default()
        } else {
            payload.to_string()
        };
        let max = self.config.max_notification_bytes;
        let slot = self.pending_slot(&meta.id);
        let target = if meta.payload_type == "body" { &mut slot.body } else { &mut slot.title };
        append_bounded(target, &text, max);
        self.notification_finish_if_done(meta);
    }

    /// The pending notification with identifier `id`, created when missing. Creating one beyond
    /// `max_pending_notifications` drops the oldest.
    fn pending_slot(&mut self, id: &str) -> &mut PendingNotification {
        if let Some(i) = self.notification_pending.iter().position(|n| n.id == id) {
            return &mut self.notification_pending[i];
        }
        let limit = self.config.max_pending_notifications.max(1);
        while self.notification_pending.len() >= limit {
            self.notification_pending.remove(0);
        }
        self.notification_pending.push(PendingNotification { id: id.to_string(), ..Default::default() });
        let last = self.notification_pending.len() - 1;
        &mut self.notification_pending[last]
    }

    /// Sends the pending notification with this identifier when `d` is not 0. A notification with
    /// no title uses its body as the title; one with neither is dropped.
    fn notification_finish_if_done(&mut self, meta: &Meta) {
        if !meta.done {
            return;
        }
        let Some(i) = self.notification_pending.iter().position(|n| n.id == meta.id) else { return };
        let PendingNotification { mut title, mut body, .. } = self.notification_pending.remove(i);
        if title.is_empty() {
            std::mem::swap(&mut title, &mut body);
        }
        if !title.is_empty() {
            self.event(Event::Notification { title, body });
        }
    }
}

/// Reads the keys Termoil uses from `key=value:key=value`; unknown keys are ignored.
fn parse_meta(meta: &str) -> Meta {
    let mut out = Meta { id: String::new(), done: true, base64: false, payload_type: "title".to_string() };
    for kv in meta.split(':') {
        match kv.split_once('=') {
            Some(("i", v)) => out.id = v.chars().filter(|c| c.is_ascii_alphanumeric() || "_-+.".contains(*c)).collect(),
            Some(("d", v)) => out.done = v != "0",
            Some(("e", v)) => out.base64 = v == "1",
            Some(("p", v)) => out.payload_type = v.to_string(),
            _ => {}
        }
    }
    out
}

/// The identifier a reply names: the request's, or `0` when the request had none.
fn reply_id(id: &str) -> &str {
    if id.is_empty() { "0" } else { id }
}

/// Appends `text` to `target` up to `max` bytes in all, cutting at a character boundary.
fn append_bounded(target: &mut String, text: &str, max: usize) {
    let room = max.saturating_sub(target.len());
    let mut end = text.len().min(room);
    while !text.is_char_boundary(end) {
        end -= 1;
    }
    target.push_str(&text[..end]);
}
