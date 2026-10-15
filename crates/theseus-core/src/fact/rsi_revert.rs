//! Auto-revert's facts (theseus-pw1q.10): `self.reverted` (a self-join undone
//! on its own after a red) and `self.vetoed` (the same, by the owner's word).
//! Each carries its `SELF_ROWS` fields beside `what`, `why`, `numbers` and
//! `undo`, so `self.log` shows it as written; a revert's own undo is the
//! revert of the revert, which the owner makes in git (a revert is a plain
//! commit on main).

use serde_json::{json, Value};
use theseus_protocol::LedgerKind;
use theseus_protocol::NarrativePart::Approval;

use super::{Fact, Say};

/// A join reverted on its own: the signal that turned red, and the revert.
pub struct SelfReverted {
    pub merge: String,
    pub revert: String,
    /// `gate`, `bench` or `health`: the signal that turned red.
    pub signal: String,
    /// The window the join was watched in, in hours.
    pub window: u64,
    pub branch: String,
    pub item: String,
    /// The evidence: what the signal read, the bisection's steps.
    pub numbers: Value,
}

impl Fact for SelfReverted {
    const KIND: Option<LedgerKind> = Some(LedgerKind::SelfReverted);

    fn row(&self) -> Value {
        json!({"merge": self.merge, "revert": self.revert, "signal": self.signal,
               "window": self.window, "branch": self.branch, "item": self.item,
               "what": format!("self-join {} reverted as {}", short(&self.merge),
                   short(&self.revert)),
               "why": format!("the {} signal turned red after it", self.signal),
               "numbers": self.numbers,
               "undo": format!("git revert -m 1 {}", self.revert)})
    }

    fn narrate(&self, say: &mut Say<'_>) {
        say.line(
            Approval,
            format!(
                "Self-join {} of {} reverted as {}: the {} signal turned red within {} h.",
                short(&self.merge),
                self.branch,
                short(&self.revert),
                self.signal,
                self.window
            ),
        );
    }
}

/// The owner's veto: a join reverted by the owner's word.
pub struct SelfVetoed {
    pub merge: String,
    pub revert: String,
    pub by: String,
    /// Where it came from, as a refusal names a place (`cli`, `web`, …).
    pub place: String,
    pub branch: String,
    pub item: String,
    /// Whether the backlog item was dropped by this veto.
    pub dropped: bool,
}

impl Fact for SelfVetoed {
    const KIND: Option<LedgerKind> = Some(LedgerKind::SelfVetoed);

    fn row(&self) -> Value {
        json!({"merge": self.merge, "revert": self.revert, "by": self.by,
               "place": self.place, "branch": self.branch, "item": self.item,
               "what": format!("self-join {} vetoed and reverted as {}", short(&self.merge),
                   short(&self.revert)),
               "why": format!("vetoed by {}", self.by),
               "numbers": {"item_dropped": self.dropped},
               "undo": format!("git revert -m 1 {}", self.revert)})
    }

    fn narrate(&self, say: &mut Say<'_>) {
        say.line(
            Approval,
            format!(
                "Self-join {} of {} vetoed by {} through {}; reverted as {}.",
                short(&self.merge),
                self.branch,
                self.by,
                self.place,
                short(&self.revert)
            ),
        );
    }
}

fn short(c: &str) -> &str {
    &c[..c.len().min(12)]
}
