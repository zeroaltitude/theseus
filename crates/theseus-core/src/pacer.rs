//! The one pacing rule for unprompted posts (the owner's decision of
//! 2026-10-13, built by the first row that needs it: the always-on channel,
//! theseus-j7cje). A post nobody asked for (a question's needs-you ping, and
//! later the digest, the morning view, a nudge, proactive voice) goes to the
//! owner's DM at most once in `[notify] unprompted_pace_secs` (30 minutes by
//! default; 0 turns the rule off). What is due while the window is closed
//! waits for it and goes out together in the next post, so a busy hour is one
//! buzz, not twelve. The window paces the buzz: a question's card is posted
//! when it is due whatever the window says, silent (`"silent": true`) while
//! it is closed, and a silent post does not take the window (theseus-27p6w).
//!
//! The rule is a clock, `Pacer`, and a list, `UNPROMPTED`, of the outbox post
//! kinds it covers. The outbox keeps the clock (`Outbox::pacer`): it learns
//! each covered post's time as the post is written and again as the index is
//! read at a start, so a restart does not forget the window. A row that
//! joins asks `open` before it posts, posts what it holds in one post, and
//! adds its kind here.

use std::collections::HashSet;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Mutex;

/// The outbox post kinds the pacer covers. A kind here is one nobody asked
/// for; a reply, a card, a settle and a reminder the owner set are not.
/// `initiative`: the morning note and the consolidation finding (theseus-slvl.9).
/// `digest`: the posted digest of what happened (theseus-ss36w).
pub const UNPROMPTED: &[&str] = &["needs_you", "initiative", "digest"];

/// When the last covered post was written.
#[derive(Debug, Default)]
pub struct Pacer {
    last_ms: AtomicU64,
}

impl Pacer {
    /// Whether a covered post may go now: none was written in the last
    /// `window_ms` (a window of 0 is no rule).
    pub fn open(&self, now_ms: u64, window_ms: u64) -> bool {
        let last = self.last_ms.load(Ordering::SeqCst);
        window_ms == 0 || last == 0 || now_ms >= last.saturating_add(window_ms)
    }

    /// A covered post was written at `at_ms`.
    pub fn took(&self, at_ms: u64) {
        self.last_ms.fetch_max(at_ms, Ordering::SeqCst);
    }

    /// When the last one was written; 0 when none was.
    pub fn last_ms(&self) -> u64 {
        self.last_ms.load(Ordering::SeqCst)
    }
}

/// The sessions the owner asked to ask again (theseus-27p6w): the next
/// question each puts is prompted, so neither the wait nor the window holds
/// its card. In memory: a restart forgets it, and the question waits as any.
#[derive(Debug, Default)]
pub struct Prompted(Mutex<HashSet<String>>);

impl Prompted {
    pub fn add(&self, session_id: &str) {
        self.0.lock().unwrap().insert(session_id.to_string());
    }

    pub fn has(&self, session_id: &str) -> bool {
        self.0.lock().unwrap().contains(session_id)
    }

    pub fn take(&self, session_id: &str) {
        self.0.lock().unwrap().remove(session_id);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn one_post_a_window_and_a_window_of_zero_is_no_rule() {
        let p = Pacer::default();
        let thirty = 30 * 60_000;
        assert!(p.open(1_000, thirty), "none yet");
        p.took(1_000_000);
        assert!(!p.open(1_000_000 + thirty - 1, thirty));
        assert!(p.open(1_000_000 + thirty, thirty));
        assert!(p.open(1_000_001, 0), "0 turns it off");
        p.took(500);
        assert_eq!(p.last_ms(), 1_000_000, "an older post moves nothing");
    }
}
