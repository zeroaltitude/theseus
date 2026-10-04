//! The memory pass's facts (M6 step 31a, design §2.6, §2.8): a node's
//! labels (`memory.labeled`) and the gate's decision with the neighbours it
//! saw (`memory.gated`), each keyed by the node and scoped
//! `memory:<session>`, so a session's next pass reads what is done with one
//! scan of its own; and a recalled item's attribution and outcome
//! (`memory.used`), scoped `recall:<session>` with the session's recalls.
//! Rows only: the pass runs after the turn, and a surface reads them.

use serde::Serialize;
use serde_json::{json, Value};
use theseus_protocol::LedgerKind;

use super::Fact;
use crate::memory_pass::attribution::{Outcome, Use};
use crate::memory_pass::labels::Labels;

/// The scope of a session's labels and gate decisions.
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

/// One neighbour the gate saw.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Seen {
    pub node_id: String,
    pub session_id: String,
    pub kind: String,
    pub cosine: f64,
}

/// The gate's decision for a node, and the neighbours it saw.
pub struct MemoryGated<'a> {
    pub node_id: &'a str,
    /// `store`, `same_entity`, `supersedes`, or `unavailable`.
    pub decision: &'a str,
    /// The edge's other end.
    pub to: Option<&'a str>,
    pub correction: bool,
    pub neighbours: &'a [Seen],
    /// The science and its parameters (`baseline@…`), whose thresholds
    /// decided.
    pub science: &'a str,
    pub merge_cosine: f32,
    pub supersede_cosine: f32,
    /// Why the gate did not run.
    pub why: Option<&'a str>,
}

impl Fact for MemoryGated<'_> {
    const KIND: Option<LedgerKind> = Some(LedgerKind::MemoryGated);

    fn row(&self) -> Value {
        json!({
            "node_id": self.node_id, "decision": self.decision, "to": self.to,
            "correction": self.correction, "neighbours": self.neighbours,
            "science": self.science, "merge_cosine": self.merge_cosine,
            "supersede_cosine": self.supersede_cosine, "why": self.why,
        })
    }
}

/// A recalled item's attribution: whether the turn used it, why, and, for
/// one used, how it turned out.
pub struct MemoryUsed<'a> {
    pub recall_id: &'a str,
    pub arm: &'a str,
    /// The recalled node, and its session.
    pub node_id: &'a str,
    pub source_session: &'a str,
    pub used: &'a Use,
    /// `None` for an item not used: exposure is no review.
    pub outcome: Option<Outcome>,
    pub entities_unavailable: Option<&'a str>,
}

impl Fact for MemoryUsed<'_> {
    const KIND: Option<LedgerKind> = Some(LedgerKind::MemoryUsed);

    fn row(&self) -> Value {
        json!({
            "recall_id": self.recall_id, "arm": self.arm, "node_id": self.node_id,
            "source_session": self.source_session, "used": self.used.used, "by": self.used.by,
            "outcome": self.outcome, "entities_unavailable": self.entities_unavailable,
        })
    }
}
