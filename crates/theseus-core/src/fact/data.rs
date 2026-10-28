//! The data floor's fact (theseus-a1wvq.3, theseus-9ymz0): a request the
//! provider chokepoint refused to a model of restricted origin, and why.

use serde_json::{json, Value};
use theseus_protocol::LedgerKind;
use theseus_protocol::NarrativePart::Session;

use super::{Fact, Say};

/// `data_gate.blocked`: a request to a model of restricted origin was refused,
/// and nothing was sent.
pub struct DataGateBlocked<'a> {
    /// The provider the request was for.
    pub provider: &'a str,
    pub model: &'a str,
    /// `cn` or `unknown`.
    pub origin: &'a str,
    /// What kept it: a flag's word (`confidential`), `never`, `no_session`
    /// or `no_record`.
    pub why: &'a str,
}

impl Fact for DataGateBlocked<'_> {
    const KIND: Option<LedgerKind> = Some(LedgerKind::DataGateBlocked);

    fn row(&self) -> Value {
        json!({"provider": self.provider, "model": self.model, "origin": self.origin,
               "why": self.why})
    }

    fn narrate(&self, say: &mut Say<'_>) {
        say.line(
            Session,
            format!(
                "Data floor: a request to {} (origin {}) was refused ({}); nothing was sent.",
                self.model, self.origin, self.why
            ),
        );
    }
}
