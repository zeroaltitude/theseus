//! The self-join's facts (theseus-pw1q.9): `self.review.verdict`,
//! `self.joined`, `self.installed`, `self.join.stopped` and `self.join.reopened`. Each carries its
//! `SELF_ROWS` fields beside every `self.*` row's `what`, `why`, `numbers`
//! and `undo`, so `self.log` shows it as written.

use serde_json::{json, Value};
use theseus_protocol::LedgerKind;
use theseus_protocol::NarrativePart::Approval;

use super::{Fact, Say};

/// An independent review's verdict on a branch at its head.
pub struct SelfReviewVerdict {
    pub item: String,
    pub branch: String,
    pub head: String,
    /// The profile and model that answered, `fable (claude-fable-5-2)`; for a refusal before any
    /// turn, the one the review would have run on.
    pub reviewer: String,
    /// The model that answered, from the turn's own result; for a refusal before any turn, the one
    /// the review would have run on.
    pub model: String,
    /// The model the review was asked to run on: a refusal is asked again on another. Differs from
    /// `model` when the turn ended up elsewhere. A row from before this field has none.
    pub asked: String,
    /// `accept`, `reject`, or `refused` (no independent review could run).
    pub verdict: String,
    /// The planted revert's result: `failed` (a test failed without the fix: proved),
    /// `build-broke` (only the build failed without it: proved by a test that no longer compiles,
    /// which the reviewer is told to check), `passed` (unproved), `no-tests`, `base-red`,
    /// `not-found`, `unbuilt`, or `not-run`.
    pub planted_revert: String,
    pub reasons: Vec<String>,
    pub cost_usd: f64,
}

impl Fact for SelfReviewVerdict {
    const KIND: Option<LedgerKind> = Some(LedgerKind::SelfReviewVerdict);

    fn row(&self) -> Value {
        json!({"item": self.item, "branch": self.branch, "head": self.head,
               "reviewer": self.reviewer, "model": self.model, "asked": self.asked,
               "verdict": self.verdict,
               "planted_revert": self.planted_revert, "reasons": self.reasons,
               "what": format!("self branch {} reviewed: {} by {}", self.branch,
                   self.verdict, self.reviewer),
               "why": self.reasons.first(),
               "numbers": {"cost_usd": self.cost_usd, "reasons": self.reasons.len()},
               "undo": null})
    }

    fn narrate(&self, say: &mut Say<'_>) {
        say.line(
            Approval,
            format!(
                "Self review of {}: {} by {} (planted revert: {}).",
                self.branch, self.verdict, self.reviewer, self.planted_revert
            ),
        );
    }
}

/// A self branch joined main.
pub struct SelfJoined {
    pub item: String,
    pub branch: String,
    pub head: String,
    pub merge: String,
    /// The gate's verdict on the merge (`green`) and the blob of main's gate.
    pub gate: String,
    pub cost_usd: f64,
    pub why: String,
    /// The tries (main moved), the gate's seconds, the diff's size.
    pub numbers: Value,
}

impl Fact for SelfJoined {
    const KIND: Option<LedgerKind> = Some(LedgerKind::SelfJoined);

    fn row(&self) -> Value {
        json!({"item": self.item, "branch": self.branch, "head": self.head,
               "merge": self.merge, "gate": self.gate, "cost_usd": self.cost_usd,
               "what": format!("self branch {} joined main as {}", self.branch,
                   short(&self.merge)),
               "why": self.why, "numbers": self.numbers,
               "undo": format!("theseus self revert {}", self.merge)})
    }

    fn narrate(&self, say: &mut Say<'_>) {
        say.line(
            Approval,
            format!(
                "Self-join: {} joined main as {} (gate {}).",
                self.branch,
                short(&self.merge),
                self.gate
            ),
        );
    }
}

/// A joined build installed and running.
pub struct SelfInstalled {
    pub merge: String,
    pub binaries: Vec<String>,
    pub version: String,
    pub branch: String,
}

impl Fact for SelfInstalled {
    const KIND: Option<LedgerKind> = Some(LedgerKind::SelfInstalled);

    fn row(&self) -> Value {
        json!({"merge": self.merge, "binaries": self.binaries, "version": self.version,
               "branch": self.branch,
               "what": format!("installed {} ({}), from self branch {}", short(&self.merge),
                   self.version, self.branch),
               "why": null, "numbers": {"binaries": self.binaries.len()},
               "undo": format!("theseus self revert {}", self.merge)})
    }
}

/// A self-join stopped before a phase.
pub struct SelfJoinStopped {
    pub branch: String,
    pub head: String,
    /// The phase it stopped before: `review`, `planted`, `keel`, `lock`,
    /// `merge`, `gate`, `sign`, `push`, `install`.
    pub phase: String,
    pub why: String,
    /// A keel stop's classes, a red gate's log.
    pub numbers: Value,
}

impl Fact for SelfJoinStopped {
    const KIND: Option<LedgerKind> = Some(LedgerKind::SelfJoinStopped);

    fn row(&self) -> Value {
        json!({"branch": self.branch, "head": self.head, "phase": self.phase,
               "what": format!("self-join of {} stopped before the {}", self.branch, self.phase),
               "why": self.why, "numbers": self.numbers, "undo": null})
    }

    fn narrate(&self, say: &mut Say<'_>) {
        say.line(
            Approval,
            format!(
                "Self-join of {} stopped before the {}: {}.",
                self.branch, self.phase, self.why
            ),
        );
    }
}

/// The owner's `self retry` on a change a join holds: the head is held no longer, so the next
/// pass runs the join on it again from its first check.
pub struct SelfJoinReopened {
    pub item: String,
    pub branch: String,
    pub head: String,
    /// Who said it: the owner's name for the act.
    pub by: String,
}

impl Fact for SelfJoinReopened {
    const KIND: Option<LedgerKind> = Some(LedgerKind::SelfJoinReopened);

    fn row(&self) -> Value {
        json!({"item": self.item, "branch": self.branch, "head": self.head, "by": self.by,
               "what": format!("self branch {} is to be checked by its join again", self.branch),
               "why": "the owner asked for it with `self retry`; nothing is remembered from the \
                       first run",
               "numbers": null, "undo": null})
    }

    fn narrate(&self, say: &mut Say<'_>) {
        say.line(
            Approval,
            format!(
                "Self-join of {} will run again from its first check; asked for by {}.",
                self.branch, self.by
            ),
        );
    }
}

fn short(c: &str) -> &str {
    &c[..c.len().min(12)]
}
