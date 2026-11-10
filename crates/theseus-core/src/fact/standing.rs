//! The standing records' facts (theseus-a7vi): a row for each record written,
//! in the frame that writes it, and a line of the narrative. The rows hold
//! the id, kind, scope and size, never the text, which the record holds.

use serde_json::{json, Value};
use theseus_protocol::NarrativePart::Session;
use theseus_protocol::{LedgerKind, StandingRecord};

use super::{Fact, Say};

/// A record was added (`standing.added`), alone or replacing another.
pub struct StandingAdded<'a> {
    pub record: &'a StandingRecord,
    pub who: &'a str,
    pub via: &'a str,
}

impl Fact for StandingAdded<'_> {
    const KIND: Option<LedgerKind> = Some(LedgerKind::StandingAdded);

    fn row(&self) -> Value {
        let r = self.record;
        json!({"id": r.id, "kind": r.kind.name(), "scope": r.scope.words(), "bytes": r.text.len(),
               "until_ms": r.until_ms, "source": r.source, "replaces": r.ends,
               "who": self.who, "via": self.via})
    }

    fn narrate(&self, say: &mut Say<'_>) {
        let r = self.record;
        say.line(
            Session,
            format!(
                "Standing: {} added the {} {} for {}; every compile of that scope carries it from its next turn.",
                self.who,
                r.kind.name(),
                r.id,
                r.scope.words()
            ),
        );
    }
}

/// A record was revoked (`standing.revoked`): the revoke's own record names
/// the one it ends.
pub struct StandingRevoked<'a> {
    pub record: &'a StandingRecord,
    pub who: &'a str,
    pub via: &'a str,
}

impl Fact for StandingRevoked<'_> {
    const KIND: Option<LedgerKind> = Some(LedgerKind::StandingRevoked);

    fn row(&self) -> Value {
        let r = self.record;
        json!({"id": r.ends, "revoke": r.id, "who": self.who, "via": self.via})
    }

    fn narrate(&self, say: &mut Say<'_>) {
        say.line(
            Session,
            format!(
                "Standing: {} revoked {}; it leaves every prompt at its next turn.",
                self.who,
                self.record.ends.as_deref().unwrap_or("a record")
            ),
        );
    }
}

/// The owner said a live record's words again (`standing.repeated`,
/// theseus-slvl.4): the record, and the node of the message that restated it.
/// The capture row writes it; counting these per record is how a later row and
/// the exam know a preference was said more than once, with no change to the
/// record or the store's format. The row holds ids, never the words.
pub struct StandingRepeated<'a> {
    pub record: &'a str,
    pub node: &'a str,
}

impl Fact for StandingRepeated<'_> {
    const KIND: Option<LedgerKind> = Some(LedgerKind::StandingRepeated);

    fn row(&self) -> Value {
        json!({"record": self.record, "node": self.node})
    }
}

/// The loop's answer opened by acknowledging a standing record the owner did
/// not just send (`turn.standing_ack`, theseus-ectzg; `standing_ack`): the
/// paragraph was `stripped` from the answer, or only `flagged` and left. The
/// row keeps the paragraph, so nothing the model said is lost.
pub struct TurnStandingAck<'a> {
    pub loop_index: u32,
    pub stripped: bool,
    pub records: &'a [String],
    pub paragraph: &'a str,
}

impl Fact for TurnStandingAck<'_> {
    const KIND: Option<LedgerKind> = Some(LedgerKind::TurnStandingAck);

    fn row(&self) -> Value {
        json!({"loop": self.loop_index,
               "action": if self.stripped { "stripped" } else { "flagged" },
               "records": self.records, "paragraph": self.paragraph})
    }

    fn span(&self, trace: &mut crate::trace::Trace) {
        trace.mark(
            "standing_ack",
            "mark",
            json!({"loop": self.loop_index, "stripped": self.stripped, "records": self.records}),
        );
    }

    fn narrate(&self, say: &mut Say<'_>) {
        say.line(
            theseus_protocol::NarrativePart::Model,
            format!(
                "The answer opened by acknowledging the owner's standing record {} and {}.",
                self.records.join(", "),
                if self.stripped {
                    "that paragraph was taken out"
                } else {
                    "was left as it is"
                }
            ),
        );
    }
}
