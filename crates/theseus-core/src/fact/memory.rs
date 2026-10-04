//! The memory pass's facts (M6 step 31a, design §2.6, §2.8): a node's
//! labels (`memory.labeled`), keyed by the node and scoped
//! `memory:<session>`, so a session's next pass reads what is done with one
//! scan of its own. Rows only: the pass runs after the turn, and a surface
//! reads them.

use serde_json::{json, Value};
use theseus_protocol::LedgerKind;

use super::Fact;
use crate::memory_pass::labels::Labels;

/// The scope of a session's labels.
pub fn scope(session_id: &str) -> String {
    format!("memory:{session_id}")
}

/// A node's labels, the deterministic baseline's.
pub struct MemoryLabeled<'a> {
    pub node_id: &'a str,
    pub position: u64,
    /// The node's kind as the index names it (`user_message`).
    pub body: &'a str,
    pub labels: &'a Labels,
    /// `None`: the tender named its entities; else why it could not.
    pub entities_unavailable: Option<&'a str>,
}

impl Fact for MemoryLabeled<'_> {
    const KIND: Option<LedgerKind> = Some(LedgerKind::MemoryLabeled);

    fn row(&self) -> Value {
        let mut v = serde_json::to_value(self.labels).unwrap_or(Value::Null);
        v["node_id"] = json!(self.node_id);
        v["position"] = json!(self.position);
        v["body"] = json!(self.body);
        v["by"] = json!("rules");
        if let Some(why) = self.entities_unavailable {
            v["entities_unavailable"] = json!(why);
        }
        v
    }
}
