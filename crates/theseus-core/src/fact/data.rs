//! What a request that left the machine carried (theseus-gyjyo R7b).

use serde_json::{json, Value};
use theseus_protocol::LedgerKind;

use super::Fact;

/// `request.redacted` (theseus-gyjyo R7b): a request that left the machine
/// carried values the vault resolved, and which. The row is the answer to
/// "what key did we send?": the secret by name, the request it rode in (the
/// loop and the call), the model it went to, and whether the exit
/// substituted it (`redact_outbound`) or sent it as written.
pub struct RequestRedacted<'a> {
    pub provider: &'a str,
    pub model: &'a str,
    /// The vault secrets the request carried, by name, sorted.
    pub names: Vec<String>,
    /// How many values were met, over every name.
    pub count: u32,
    /// Whether they were replaced by `[redacted:<name>]`.
    pub substituted: bool,
    /// What was sent: `stream` (a model call) or `count_tokens`.
    pub op: &'a str,
    /// The loop of the turn it was made for, when it is a turn's.
    pub loop_index: Option<u32>,
    /// The provider action's correlation id, when it is a turn's.
    pub correlation_id: Option<&'a str>,
}

impl Fact for RequestRedacted<'_> {
    const KIND: Option<LedgerKind> = Some(LedgerKind::RequestRedacted);

    fn row(&self) -> Value {
        json!({"provider": self.provider, "model": self.model, "names": self.names,
               "count": self.count, "substituted": self.substituted, "op": self.op,
               "loop": self.loop_index, "correlation_id": self.correlation_id})
    }
}
