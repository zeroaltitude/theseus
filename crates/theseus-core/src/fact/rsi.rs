//! The kill switch's facts (theseus-pw1q.2): `self.halted` and
//! `self.resumed`, each written in the frame that moves the switch, with the
//! switch's META record (`crate::rsi::switch`). Every `self.*` row carries
//! `what`, `why`, `numbers` and `undo` beside its own fields
//! (`theseus_protocol::rsi::SELF_ROWS`), so `self.log` shows it as written.

use serde_json::{json, Value};
use theseus_protocol::LedgerKind;
use theseus_protocol::NarrativePart::Approval;

use super::{Fact, Say};

/// The switch thrown: every self step stops before its next phase.
pub struct SelfHalted<'a> {
    pub by: &'a str,
    /// The place it came from, as a refusal names one (`cli`, `web`,
    /// `discord:dm`, `discord:<channel>`).
    pub place: &'a str,
    pub why: Option<&'a str>,
}

impl Fact for SelfHalted<'_> {
    const KIND: Option<LedgerKind> = Some(LedgerKind::SelfHalted);

    fn row(&self) -> Value {
        json!({"by": self.by, "place": self.place,
               "what": format!("self-improvement halted by {}", self.by),
               "why": self.why, "numbers": null,
               "undo": "theseus self resume (the owner, from a private place)"})
    }

    fn narrate(&self, say: &mut Say<'_>) {
        let why = self.why.map(|w| format!(": {w}")).unwrap_or_default();
        say.line(
            Approval,
            format!(
                "Self-improvement halted by {} through {}{why}. Nothing self-directed runs \
                 until the owner resumes it.",
                self.by, self.place
            ),
        );
    }
}

/// The owner released the switch, from a private place.
pub struct SelfResumed<'a> {
    pub by: &'a str,
    pub place: &'a str,
}

impl Fact for SelfResumed<'_> {
    const KIND: Option<LedgerKind> = Some(LedgerKind::SelfResumed);

    fn row(&self) -> Value {
        json!({"by": self.by, "place": self.place,
               "what": format!("self-improvement resumed by {}", self.by),
               "why": null, "numbers": null, "undo": "theseus self halt [why]"})
    }

    fn narrate(&self, say: &mut Say<'_>) {
        say.line(
            Approval,
            format!(
                "Self-improvement resumed by {} through {}: self steps may run while [self] \
                 mode is \"act\".",
                self.by, self.place
            ),
        );
    }
}
