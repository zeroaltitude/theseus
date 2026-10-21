//! The data floor's facts (theseus-a1wvq.3): a request the provider chokepoint
//! refused to a model of restricted origin, for want of a pass.

use serde_json::{json, Value};
use theseus_protocol::LedgerKind;
use theseus_protocol::NarrativePart::Session;

use super::{Fact, Say};

/// `data_gate.blocked`: a request to a model of restricted origin carried no
/// pass for its data, and nothing was sent.
pub struct DataGateBlocked<'a> {
    /// The provider the request was for.
    pub provider: &'a str,
    pub model: &'a str,
    /// `cn` or `unknown`.
    pub origin: &'a str,
}

impl Fact for DataGateBlocked<'_> {
    const KIND: Option<LedgerKind> = Some(LedgerKind::DataGateBlocked);

    fn row(&self) -> Value {
        json!({"provider": self.provider, "model": self.model, "origin": self.origin})
    }

    fn narrate(&self, say: &mut Say<'_>) {
        say.line(
            Session,
            format!(
                "Data floor: a request to {} (origin {}) had no pass for its data; nothing was sent.",
                self.model, self.origin
            ),
        );
    }
}
