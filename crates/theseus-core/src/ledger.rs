//! Ledger rows. Every state transition, hook site visit, loop, and Advancer
//! decision is a row. In M0 rows are JSON in the embedded store.

use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LedgerRow {
    pub at_unix_ms: u64,
    pub kind: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub session_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub turn_id: Option<String>,
    #[serde(default, skip_serializing_if = "Value::is_null")]
    pub data: Value,
}

impl LedgerRow {
    pub fn new(kind: &str, session_id: Option<&str>, turn_id: Option<&str>, data: Value) -> Self {
        Self {
            at_unix_ms: theseus_protocol::now_unix_ms(),
            kind: kind.into(),
            session_id: session_id.map(str::to_string),
            turn_id: turn_id.map(str::to_string),
            data,
        }
    }
}
