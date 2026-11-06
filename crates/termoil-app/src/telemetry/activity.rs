//! Open, focused and active time, from events.
//!
//! Termoil draws only when something changes, so there is no frame to count time on. The window layer
//! reports input and focus instead, and the time between two reports is credited when the second one
//! arrives (or when the sender asks).
//!
//! - *Open* is the time since the process started.
//! - *Focused* is time a Termoil window held the operating system's focus. A stretch with no event at all
//!   is credited at most [`LONGEST_QUIET`], so a machine that slept with a focused window does not count
//!   the night.
//! - *Active* is focused time within [`ACTIVE_FOR`] of the last input, which is GA4's idea of engagement.

use std::time::{Duration, Instant};

/// How long after the last input a focused window still counts as being used.
pub const ACTIVE_FOR: Duration = Duration::from_secs(60);
/// The most focused time one quiet stretch may add.
pub const LONGEST_QUIET: Duration = Duration::from_secs(30 * 60);

/// The clock. Every method takes the time now, so tests pass made up instants.
#[derive(Debug, Clone)]
pub struct Activity {
    started: Instant,
    /// Whether a window has the focus.
    focused: bool,
    /// The last input while focused.
    last_input: Option<Instant>,
    /// Time is credited up to here.
    counted_to: Instant,
    focused_time: Duration,
    active_time: Duration,
    /// Active time since the last request, which is that request's `engagement_time_msec`.
    active_since_send: Duration,
}

impl Activity {
    /// A clock starting now, with no window focused yet.
    ///
    /// @param now - the time the process started
    pub fn new(now: Instant) -> Activity {
        Activity {
            started: now,
            focused: false,
            last_input: None,
            counted_to: now,
            focused_time: Duration::ZERO,
            active_time: Duration::ZERO,
            active_since_send: Duration::ZERO,
        }
    }

    /// Credits the time from the last event to `now`.
    ///
    /// @param now - the time now
    pub fn settle(&mut self, now: Instant) {
        let from = self.counted_to;
        if now <= from {
            return;
        }
        if self.focused {
            self.focused_time += (now - from).min(LONGEST_QUIET);
            if let Some(input) = self.last_input {
                let end = now.min(input + ACTIVE_FOR);
                if end > from {
                    self.active_time += end - from;
                    self.active_since_send += end - from;
                }
            }
        }
        self.counted_to = now;
    }

    /// Records input in a window.
    ///
    /// @param now - when it happened
    pub fn input(&mut self, now: Instant) {
        self.settle(now);
        if self.focused {
            self.last_input = Some(now);
        }
    }

    /// Records a window gaining or losing the focus.
    ///
    /// @param focused - whether a window has the focus now
    /// @param now - when it changed
    pub fn focus(&mut self, focused: bool, now: Instant) {
        self.settle(now);
        self.focused = focused;
        if !focused {
            self.last_input = None;
        }
    }

    /// Active time since the last request, which this resets.
    ///
    /// @param now - the time now
    pub fn take_engagement(&mut self, now: Instant) -> Duration {
        self.settle(now);
        std::mem::take(&mut self.active_since_send)
    }

    /// Whether there was active time since the last request.
    ///
    /// @param now - the time now
    pub fn engaged_since_send(&mut self, now: Instant) -> bool {
        self.settle(now);
        self.active_since_send >= Duration::from_secs(1)
    }

    /// Drops the engagement not yet sent, when the setting is turned off.
    ///
    /// @param now - the time now
    pub fn forget_engagement(&mut self, now: Instant) {
        self.settle(now);
        self.active_since_send = Duration::ZERO;
    }

    /// Open, focused and active time so far.
    ///
    /// @param now - the time now
    pub fn totals(&mut self, now: Instant) -> (Duration, Duration, Duration) {
        self.settle(now);
        (now.saturating_duration_since(self.started), self.focused_time, self.active_time)
    }
}
