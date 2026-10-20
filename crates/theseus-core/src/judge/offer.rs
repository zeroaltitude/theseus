//! The task offer's two halves (theseus-bh3t.5, theseus-ypnxt.5): `classify.v3`
//! finds the owner's message to be work for later (`inbound::offer_answer`), and
//! the reply carries a chip for it.
//!
//! - **Early.** The batch landed before the reply was written: `note_offer`
//!   kept the brief, and the reply's post takes it (`take_offer`).
//! - **Late.** The batch has not landed yet: the reply is written without the
//!   chip and `take_offer` leaves the turn waiting. When the batch lands within
//!   [`LATE_MS`] of the reply, `note_offer` writes an `offer` post to the reply's
//!   place (one frame, only then), which the binding turns into an edit of the
//!   reply. A later one is dropped: the moment has passed. The plain turn pays a
//!   lock and nothing else, and never waits for the judge.
//!
//! In memory, bounded; a restart forgets what has not landed.

use std::collections::VecDeque;
use std::time::Duration;

use serde_json::json;
use tokio::time::Instant;

use super::JudgeService;
use crate::node::Body;
use crate::store::Store;

/// How many turns' offers wait for their reply, or replies for their offer.
const KEPT: usize = 64;
/// How much of the message the offered brief keeps.
const BRIEF_CHARS: usize = 1_000;
/// How long after the reply an offer may still land.
pub const LATE: Duration = Duration::from_secs(120);

/// A reply written without its chip, waiting for the batch.
pub struct Waiting<'a> {
    pub turn: &'a str,
    pub session: &'a str,
    pub execution: &'a str,
    pub target: &'a str,
}

pub(super) struct Owed {
    pub(super) turn: String,
    pub(super) session: String,
    pub(super) execution: String,
    pub(super) target: String,
    pub(super) at: Instant,
}

/// The offers noted and the replies waiting for them.
#[derive(Default)]
pub struct Book {
    briefs: VecDeque<(String, String)>,
    owed: VecDeque<Owed>,
}

impl Book {
    /// The batch found `turn` offerable at `now`: the reply that waits for it,
    /// if it is within [`LATE`] of the reply's writing (an older one is
    /// dropped); else the brief is kept for the reply to come.
    pub(super) fn note(&mut self, turn: &str, brief: &str, now: Instant) -> Option<Owed> {
        match self.owed.iter().position(|o| o.turn == turn) {
            Some(at) => self
                .owed
                .remove(at)
                .filter(|o| now.saturating_duration_since(o.at) <= LATE),
            None => {
                if self.briefs.len() == KEPT {
                    self.briefs.pop_front();
                }
                self.briefs.push_back((turn.to_string(), brief.to_string()));
                None
            }
        }
    }

    /// The brief noted for `turn`, taken; else the reply, written at `now`
    /// without one, waits for it.
    pub(super) fn take(&mut self, w: &Waiting<'_>, now: Instant) -> Option<String> {
        if let Some(at) = self.briefs.iter().position(|(t, _)| t == w.turn) {
            return self.briefs.remove(at).map(|(_, brief)| brief);
        }
        if self.owed.len() == KEPT {
            self.owed.pop_front();
        }
        self.owed.push_back(Owed {
            turn: w.turn.to_string(),
            session: w.session.to_string(),
            execution: w.execution.to_string(),
            target: w.target.to_string(),
            at: now,
        });
        None
    }
}

impl JudgeService {
    /// The batch found `turn`'s message offerable. A reply already written
    /// without the chip gets it late; else the brief waits for the reply.
    pub(super) fn note_offer(&self, turn: &str, text: &str) {
        let brief: String = text.chars().take(BRIEF_CHARS).collect();
        let late = {
            let mut book = self.offers.lock().unwrap_or_else(|e| e.into_inner());
            book.note(turn, &brief, Instant::now())
        };
        if let Some(o) = late {
            self.offer_late(o, brief);
        }
    }

    /// The brief of `turn`'s offer, when the batch had landed by the time the
    /// reply was written. When it had not, the turn waits for it: the batch's
    /// landing writes the late offer (it never waits).
    pub fn take_offer(&self, w: Waiting<'_>) -> Option<String> {
        let mut book = self.offers.lock().unwrap_or_else(|e| e.into_inner());
        book.take(&w, Instant::now())
    }

    /// The reply went out without its chip: post the offer to its place, off
    /// the batch's task, unless the turn made a task itself meanwhile.
    fn offer_late(&self, o: Owed, brief: String) {
        let Some(core) = self.core() else { return };
        tokio::task::spawn_blocking(move || {
            if made_a_task(&core.store, &o.session, &o.turn) != Some(false) {
                return;
            }
            let body = json!({"kind": "offer", "turn_id": o.turn, "brief": brief});
            if let Err(e) = core.outbox.post(&o.session, &o.execution, &o.target, body) {
                tracing::warn!(error = %format!("{e:#}"), "the late task offer was not posted");
            }
        });
    }
}

/// Whether the turn called `task_create` itself, which makes the offer moot;
/// `None` when the transcript cannot be read.
pub fn made_a_task(store: &Store, session: &str, turn: &str) -> Option<bool> {
    let nodes = store.transcript(session).ok()?;
    Some(nodes.iter().any(|(_, n)| {
        n.turn_id.as_deref() == Some(turn)
            && matches!(&n.body, Body::AssistantMessage { blocks, .. }
                if crate::provider::tool_uses_in(blocks)
                    .iter()
                    .any(|u| matches!(u.name.as_str(), "task_create" | "task.create")))
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn waiting(turn: &str) -> Waiting<'_> {
        Waiting {
            turn,
            session: "ses_a",
            execution: "exe_a",
            target: "discord:dm:42",
        }
    }

    /// An offer that lands within two minutes of its reply is owed to the
    /// reply; one that lands later is dropped, and is not kept for a reply
    /// that has gone (theseus-ypnxt.5).
    #[test]
    fn a_batch_within_two_minutes_of_the_reply_is_late_and_a_slower_one_is_dropped() {
        let t0 = Instant::now();
        let mut book = Book::default();
        assert_eq!(book.take(&waiting("turn_1"), t0), None);
        assert!(book
            .note("turn_1", "later", t0 + Duration::from_secs(90))
            .is_some());
        assert_eq!(book.take(&waiting("turn_2"), t0), None);
        assert!(book
            .note("turn_2", "later", t0 + Duration::from_secs(180))
            .is_none());
        assert!(book.briefs.is_empty(), "a dropped offer is kept for no one");
        assert!(book.owed.is_empty());
    }

    /// A batch that landed first is taken by the reply, once.
    #[test]
    fn a_batch_before_the_reply_is_taken_by_it_once() {
        let t0 = Instant::now();
        let mut book = Book::default();
        assert!(book.note("turn_1", "later", t0).is_none());
        assert_eq!(book.take(&waiting("turn_1"), t0).as_deref(), Some("later"));
        assert_eq!(book.take(&waiting("turn_1"), t0), None);
    }

    #[test]
    fn what_waits_is_bounded() {
        let t0 = Instant::now();
        let mut book = Book::default();
        for i in 0..(KEPT + 5) {
            book.note(&format!("a{i}"), "x", t0);
            book.take(&waiting(&format!("b{i}")), t0);
        }
        assert_eq!((book.briefs.len(), book.owed.len()), (KEPT, KEPT));
    }
}
