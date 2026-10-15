//! Self-narrow's facts (theseus-pw1q.15): a narrowing proposal, the owner's
//! answer to it, and the week's watch after an apply. Every `self.*` row
//! carries `what`, `why`, `numbers` and `undo` beside its own fields
//! (`theseus_protocol::rsi::SELF_ROWS`).

use serde_json::{json, Value};
use theseus_protocol::LedgerKind;
use theseus_protocol::NarrativePart::Approval;

use super::{Fact, Say};

/// A narrower policy proposed to the owner.
pub struct NarrowProposed {
    pub role: String,
    pub proposal: String,
    pub actions_before: u64,
    pub actions_after: u64,
    pub evidence_days: u32,
    /// The analyzer's suggestions and warnings, in words.
    pub analyzer_findings: Vec<String>,
    pub events: u64,
}

impl Fact for NarrowProposed {
    const KIND: Option<LedgerKind> = Some(LedgerKind::SelfNarrowProposed);

    fn row(&self) -> Value {
        json!({"role": self.role, "proposal": self.proposal,
               "actions_before": self.actions_before, "actions_after": self.actions_after,
               "evidence_days": self.evidence_days, "analyzer_findings": self.analyzer_findings,
               "what": format!("narrower policy proposed for {}: about {} actions down to {}",
                   self.role, self.actions_before, self.actions_after),
               "why": format!("{} CloudTrail events over {} days show the role calling only these",
                   self.events, self.evidence_days),
               "numbers": {"before": self.actions_before, "after": self.actions_after,
                           "events": self.events},
               "undo": format!("theseus self narrow decline {}", self.proposal)})
    }

    fn narrate(&self, say: &mut Say<'_>) {
        say.line(
            Approval,
            format!(
                "A narrower policy for {} is on the owner's card: about {} actions down to {}.",
                self.role, self.actions_before, self.actions_after
            ),
        );
    }
}

/// The owner applied a proposal.
pub struct NarrowApplied {
    pub role: String,
    pub proposal: String,
    pub stack: String,
    pub change_set: String,
    pub by: String,
}

impl Fact for NarrowApplied {
    const KIND: Option<LedgerKind> = Some(LedgerKind::SelfNarrowApplied);

    fn row(&self) -> Value {
        json!({"role": self.role, "proposal": self.proposal, "stack": self.stack,
               "change_set": self.change_set, "by": self.by,
               "what": format!("{}'s narrower policy applied through stack {}", self.role, self.stack),
               "why": "the owner said yes to the proposal", "numbers": null,
               "undo": format!("theseus self narrow revert {}", self.proposal)})
    }

    fn narrate(&self, say: &mut Say<'_>) {
        say.line(
            Approval,
            format!(
                "{}'s narrower policy applied through {}. A week's watch for AccessDenied begins.",
                self.role, self.stack
            ),
        );
    }
}

/// The owner said not now.
pub struct NarrowDeclined {
    pub role: String,
    pub proposal: String,
    pub by: String,
}

impl Fact for NarrowDeclined {
    const KIND: Option<LedgerKind> = Some(LedgerKind::SelfNarrowDeclined);

    fn row(&self) -> Value {
        json!({"role": self.role, "proposal": self.proposal, "by": self.by,
               "what": format!("narrowing {} declined for now", self.role),
               "why": "the owner said not now", "numbers": null, "undo": null})
    }

    fn narrate(&self, say: &mut Say<'_>) {
        say.line(
            Approval,
            format!("Narrowing {} declined for now.", self.role),
        );
    }
}

/// A narrowed role was denied a call.
pub struct NarrowRed {
    pub role: String,
    pub proposal: String,
    /// `service:Action` of the first denied call.
    pub denied: String,
    pub count: u64,
}

impl Fact for NarrowRed {
    const KIND: Option<LedgerKind> = Some(LedgerKind::SelfNarrowRed);

    fn row(&self) -> Value {
        json!({"role": self.role, "proposal": self.proposal, "denied": self.denied,
               "count": self.count,
               "what": format!("{} was denied {} after it was narrowed", self.role, self.denied),
               "why": "an AccessDenied from the narrowed role in the week after the apply",
               "numbers": {"denied": self.count},
               "undo": format!("theseus self narrow revert {}", self.proposal)})
    }

    fn narrate(&self, say: &mut Say<'_>) {
        say.line(
            Approval,
            format!(
                "{} was denied {} after it was narrowed: the owner's card offers the revert.",
                self.role, self.denied
            ),
        );
    }
}

/// The owner reverted a narrowing.
pub struct NarrowReverted {
    pub role: String,
    pub proposal: String,
    pub stack: String,
    pub change_set: String,
    pub by: String,
}

impl Fact for NarrowReverted {
    const KIND: Option<LedgerKind> = Some(LedgerKind::SelfNarrowReverted);

    fn row(&self) -> Value {
        json!({"role": self.role, "proposal": self.proposal, "stack": self.stack,
               "change_set": self.change_set, "by": self.by,
               "what": format!("{}'s narrowing reverted through stack {}", self.role, self.stack),
               "why": "the owner reverted it", "numbers": null, "undo": null})
    }

    fn narrate(&self, say: &mut Say<'_>) {
        say.line(
            Approval,
            format!(
                "{}'s policy is back as it was before the narrowing.",
                self.role
            ),
        );
    }
}
