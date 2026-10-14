//! The self-join's facts (theseus-pw1q.9): `self.review.verdict`,
//! `self.joined`, `self.installed` and `self.join.stopped`. Each carries its
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
    /// The reviewer's profile and model, `fable (claude-fable-5-1)`.
    pub reviewer: String,
    /// The reviewer's model alone: a refusal is asked again on another.
    pub model: String,
    /// `accept`, `reject`, or `refused` (no independent review could run).
    pub verdict: String,
    /// The planted revert's result: `failed` (proved), `passed` (unproved),
    /// `no-tests`, `base-red`, or `not-run`.
    pub planted_revert: String,
    pub reasons: Vec<String>,
    pub cost_usd: f64,
}

impl Fact for SelfReviewVerdict {
    const KIND: Option<LedgerKind> = Some(LedgerKind::SelfReviewVerdict);

    fn row(&self) -> Value {
        json!({"item": self.item, "branch": self.branch, "head": self.head,
               "reviewer": self.reviewer, "model": self.model, "verdict": self.verdict,
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

fn short(c: &str) -> &str {
    &c[..c.len().min(12)]
}
