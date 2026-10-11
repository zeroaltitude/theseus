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
