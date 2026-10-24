//! What the turn said first, and whether it kept its word (theseus-slvl.9).
//!
//! At the turn's end, off nothing: the reply's text is read once by the
//! small matcher of `initiative::promise` (a substring search over a few
//! hundred bytes), and a reply that promises a follow-up writes
//! `promise.kept` when its turn scheduled one (a wake, a watch or a task,
//! counted by `Pending::scheduled`) and `promise.unkept` when it did not.
//! A turn that took a wake or a task's report was Theseus speaking first:
//! its `initiative.spoke` row is what the owner's 👍 or 👎 names. The rows
//! ride the turn's next frame, so a plain turn writes nothing here and no
//! frame of its own.

use super::{fact, Turn, TurnRunner};
use theseus_protocol::TurnSubmitResult;

impl TurnRunner {
    pub(super) fn initiative_rows(&self, t: &mut Turn<'_>, result: &TurnSubmitResult) {
        if let Some(phrase) = crate::initiative::promise::promised(&result.output) {
            match t.tc.commitments.scheduled() {
                0 => t.record(&fact::initiative::PromiseUnkept { phrase }),
                scheduled => t.record(&fact::initiative::PromiseKept { phrase, scheduled }),
            }
        }
        let kind = match (t.wakes.is_empty(), t.woke_by.is_empty()) {
            (true, true) => return,
            (false, _) => "wake",
            (true, false) => "report",
        };
        let persona = self
            .personas()
            .persona_for(t.target.persona.as_deref(), None)
            .name;
        t.record(&fact::initiative::InitiativeSpoke {
            id: t.tc.turn_id,
            kind,
            persona: &persona,
            cost_usd: None,
            model: None,
            text: None,
            stop_reason: None,
            output_tokens: None,
        });
    }
}
