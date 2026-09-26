//! Sessions (spec §3.2a, §4.4b). A session is a compiler scope with exactly one
//! execution, and that execution has the turn lock (the kernel holds it, M2).
//! In M0–M2 the compilation is the user's prompt and nothing else.

use serde::{Deserialize, Serialize};
use theseus_protocol::{SessionInfo, SessionKind, Usage};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionRecord {
    pub session_id: String,
    pub kind: SessionKind,
    pub label: Option<String>,
    pub created_at_unix_ms: u64,
    pub turns: u64,
    pub last_turn_id: Option<String>,
    #[serde(default)]
    pub usage: Usage,
    /// The session's one kernel execution (§3.2a). Sessions written before M2
    /// have none; the turn runner opens one on their next turn.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub execution_id: Option<String>,
}

impl SessionRecord {
    pub fn new(kind: SessionKind, label: Option<String>) -> Self {
        Self {
            session_id: crate::new_id("ses"),
            kind,
            label,
            created_at_unix_ms: theseus_protocol::now_unix_ms(),
            turns: 0,
            last_turn_id: None,
            usage: Usage::default(),
            execution_id: None,
        }
    }
    pub fn info(&self) -> SessionInfo {
        SessionInfo {
            session_id: self.session_id.clone(),
            kind: self.kind,
            label: self.label.clone(),
            created_at_unix_ms: self.created_at_unix_ms,
            turns: self.turns,
            usage: self.usage.clone(),
            execution_id: self.execution_id.clone(),
            execution_state: None,
        }
    }
}
