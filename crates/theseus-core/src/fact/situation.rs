//! Situations' fact (M6 step 35a): a request its situation does not admit,
//! or whose set does not close (`context.unadmitted`), so the turn fails
//! before any call, naming the piece.

use serde_json::{json, Value};
use theseus_protocol::{LedgerKind, NarrativePart::Context};

use super::{Fact, Say};
use crate::compiler::situation::{Situation, Unadmitted as Why};

/// A compiled request that was not sent (§2.11): its situation, and the
/// piece it did not admit or that did not close.
pub struct Unadmitted<'a> {
    pub situation: &'a Situation,
    pub why: &'a Why,
    pub compilation_id: &'a str,
    pub loop_index: u32,
}

impl Fact for Unadmitted<'_> {
    const KIND: Option<LedgerKind> = Some(LedgerKind::ContextUnadmitted);

    fn row(&self) -> Value {
        json!({"situation": self.situation, "why": self.why.why, "piece": self.why.piece,
               "detail": self.why.detail, "compilation_id": self.compilation_id,
               "loop": self.loop_index})
    }

    fn narrate(&self, say: &mut Say<'_>) {
        say.line(Context, format!("Context: {}", words(self.why)));
    }
}

/// The failure in words: what was not sent, and why.
pub fn words(w: &Why) -> String {
    let what = match w.why {
        "unclosed" => "does not close",
        _ => "admits a piece it may not",
    };
    format!(
        "the compiled request {what}: {}. Nothing was sent.",
        w.detail
    )
}
