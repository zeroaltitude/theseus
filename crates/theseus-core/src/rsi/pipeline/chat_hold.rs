//! The hold on a finding filed from chat (issue 57): its first build does not start for
//! [`CHAT_HOLD`] after its filing and its note are written, so the note, with its Undo, reaches
//! the owner before work starts. It is not a promise that he has read it.
//!
//! The hold lives in this process's memory, kept by the filing itself: marked before the frame
//! is staged (held, with no end yet), its end set from the monotonic clock once the frame that
//! holds the filing and its note has been written, and dropped when that write fails. A
//! chat-filed item this process holds no entry for (filed before a restart) is held for a full
//! minute from the start of this process: nothing durable says when its note was written, so the
//! hold is taken again, whole. No store format changes. Every start asks it: the pipeline's ready
//! queue (`queues.rs`) and the starter they share (`backlog/start.rs`).

use std::collections::HashMap;
use std::sync::Mutex;
use std::time::{Duration, Instant};

use theseus_protocol::tasks::TaskRecord;

/// How long a finding filed from chat waits after its note is written. The tender ticks at
/// least every two minutes, so its first build starts at the first tick after.
pub(crate) const CHAT_HOLD: Duration = Duration::from_secs(60);

/// The holds of this process: when it started, and each filing's end (none while its frame is
/// being written).
pub(crate) struct ChatHolds {
    started: Mutex<Instant>,
    until: Mutex<HashMap<String, Option<Instant>>>,
}

impl Default for ChatHolds {
    fn default() -> Self {
        Self {
            started: Mutex::new(Instant::now()),
            until: Mutex::default(),
        }
    }
}

/// Whether `t` was filed from chat: `self.file`'s evidence, `filed:<the owner>`.
pub(crate) fn from_chat(t: &TaskRecord) -> bool {
    let chat = format!("filed:{}", crate::rsi::backlog::tool::OWNER);
    t.evidence.iter().any(|e| e.identity == chat)
}

impl ChatHolds {
    fn map(&self) -> std::sync::MutexGuard<'_, HashMap<String, Option<Instant>>> {
        self.until
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }

    /// `item`'s frame is about to be written: held, with no end yet.
    pub(crate) fn writing(&self, item: &str) {
        self.map().insert(item.to_string(), None);
    }

    /// `item`'s filing and note are written: held for [`CHAT_HOLD`] from now.
    pub(crate) fn written(&self, item: &str) {
        self.map()
            .insert(item.to_string(), Some(Instant::now() + CHAT_HOLD));
    }

    /// `item`'s frame was not written: there is nothing to hold.
    pub(crate) fn failed(&self, item: &str) {
        self.map().remove(item);
    }

    /// Whether `t`'s first build must wait now.
    pub(crate) fn held(&self, t: &TaskRecord) -> bool {
        if !from_chat(t) {
            return false;
        }
        let now = Instant::now();
        match self.map().get(&t.id) {
            Some(None) => true,
            Some(Some(end)) => now < *end,
            None => {
                let started = *self
                    .started
                    .lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner);
                now < started + CHAT_HOLD
            }
        }
    }

    /// How much of `item`'s hold is left, when this process holds an end for it.
    #[cfg(test)]
    pub(crate) fn left(&self, item: &str) -> Option<Duration> {
        let end = (*self.map().get(item)?)?;
        Some(end.saturating_duration_since(Instant::now()))
    }

    /// A test's clock: every hold, and the process's start, `by` earlier.
    #[cfg(test)]
    pub(crate) fn age(&self, by: Duration) {
        let mut s = self
            .started
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        *s = s.checked_sub(by).unwrap_or(*s);
        for end in self.map().values_mut().flatten() {
            *end = end.checked_sub(by).unwrap_or(*end);
        }
    }

    /// A test's restart: this process's memory is gone, and it starts now.
    #[cfg(test)]
    pub(crate) fn restart(&self) {
        self.map().clear();
        *self
            .started
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner) = Instant::now();
    }
}
