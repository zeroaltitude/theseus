//! Ledger rows. Every state transition, loop, and Advancer decision is a
//! row. In M0 rows are JSON in the embedded store.

use serde::{Deserialize, Serialize};
use serde_json::Value;

/// Kinds renamed after rows were stored under the old name, as (now, before).
/// A query for either name reads both.
const RENAMED: &[(&str, &str)] = &[("action.declined", "action.denied")];

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

    /// Whether this row answers a query for `kind`; a renamed kind matches
    /// under either of its names.
    pub fn is_kind(&self, kind: &str) -> bool {
        self.kind == kind
            || RENAMED.iter().any(|&(now, before)| {
                (kind == now && self.kind == before) || (kind == before && self.kind == now)
            })
    }
}
