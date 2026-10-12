//! A node marked wrong, and the correction notes that follow it
//! (theseus-2ced). `fact.retracted` is scoped `retract` and holds the whole
//! table, so a second retraction of the node answers from it; `fact.corrected`
//! is a note written into a session at its next turn, in that turn's
//! session scope.

use serde_json::{json, Value};
use theseus_protocol::{LedgerKind, NarrativePart::Context};

use super::{Fact, Say};

/// A retraction: the node, why, who, and everything the walk found.
pub struct Retracted<'a> {
    pub node_id: &'a str,
    pub session_id: &'a str,
    pub gist: &'a str,
    pub why: &'a str,
    pub who: &'a str,
    pub via: &'a str,
    pub at_ms: u64,
    pub exposures: &'a [crate::retract::Found],
    pub partial: bool,
    /// Whether the card to the owner was staged.
    pub card: bool,
    /// The node's own session reads private.
    pub private: bool,
    /// The nodes labeled with it: the node `wrong`, its syntheses `stale`,
    /// its published copies `wrong`.
    pub labeled: &'a [(String, &'static str)],
}

impl Fact for Retracted<'_> {
    const KIND: Option<LedgerKind> = Some(LedgerKind::FactRetracted);

    fn row(&self) -> Value {
        json!({
            "node_id": self.node_id, "session_id": self.session_id, "gist": self.gist,
            "why": self.why, "by": self.who, "via": self.via, "at_ms": self.at_ms,
            "exposures": self.exposures, "partial": self.partial, "card": self.card, "private": self.private,
            "labeled": self.labeled.iter().map(|(n, l)| json!({"node_id": n, "label": l}))
                .collect::<Vec<_>>(),
        })
    }

    fn narrate(&self, say: &mut Say<'_>) {
        say.line(
            Context,
            format!(
                "{} retracted {} through {}: {}. It reached {} exposure(s){}.",
                self.who,
                self.node_id,
                self.via,
                self.why,
                self.exposures.len(),
                if self.partial { " (and more)" } else { "" },
            ),
        );
    }
}

/// A correction note, written into a session at its next turn.
pub struct Corrected<'a> {
    pub node_id: &'a str,
    pub session_id: &'a str,
    pub note_id: &'a str,
    pub text: &'a str,
}

impl Fact for Corrected<'_> {
    const KIND: Option<LedgerKind> = Some(LedgerKind::FactCorrected);

    fn row(&self) -> Value {
        json!({"node_id": self.node_id, "note_id": self.note_id, "text": self.text})
    }

    fn narrate(&self, say: &mut Say<'_>) {
        say.line(
            Context,
            format!(
                "A correction about {} was written into {}.",
                self.node_id, self.session_id
            ),
        );
    }
}
