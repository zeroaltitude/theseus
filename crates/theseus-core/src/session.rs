//! Sessions (spec §3.2a, §4.4b). A session is a compiler scope with exactly one
//! turn lock. In M0 the compilation is the user's prompt and nothing else.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use serde::{Deserialize, Serialize};
use theseus_protocol::{SessionInfo, SessionKind};
use tokio::sync::Mutex as AsyncMutex;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionRecord {
    pub session_id: String,
    pub kind: SessionKind,
    pub label: Option<String>,
    pub created_at_unix_ms: u64,
    pub turns: u64,
    pub last_turn_id: Option<String>,
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
        }
    }
    pub fn info(&self) -> SessionInfo {
        SessionInfo {
            session_id: self.session_id.clone(),
            kind: self.kind,
            label: self.label.clone(),
            created_at_unix_ms: self.created_at_unix_ms,
            turns: self.turns,
        }
    }
}

/// One lock per session: exactly one turn advances at a time (the "GIL").
#[derive(Clone, Default)]
pub struct TurnLocks {
    locks: Arc<Mutex<HashMap<String, Arc<AsyncMutex<()>>>>>,
}

impl TurnLocks {
    pub fn for_session(&self, session_id: &str) -> Arc<AsyncMutex<()>> {
        let mut g = self.locks.lock().unwrap();
        g.entry(session_id.to_string())
            .or_insert_with(|| Arc::new(AsyncMutex::new(())))
            .clone()
    }
}
