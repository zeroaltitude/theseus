//! Records: the unit the kernel appends. Each carries its kind, so old rows
//! are read forward, in place (§6).
//!
//! **The version rule** (P5b, theseus-qa0 F4a; one number since
//! theseus-ptx1): the store has one format number, `MANIFEST_FORMAT` in
//! `store.rs`. A step that adds a field to a stored record, or changes how
//! one encodes, bumps it, on the same commit as the reader for the layout it
//! replaces (serde defaults, or a reader such as `Execution::from_stored`)
//! and a sample of the old layout in theseus-core's `tests_layouts`. A build
//! refuses a store whose format is newer than its own, so an older binary
//! never reads or writes over a newer store.

use serde::{de::DeserializeOwned, Deserialize, Serialize};

pub type RecordKind = u16;

/// Well-known kinds. Stable numbers; never reuse.
pub mod kinds {
    use super::RecordKind;
    pub const SESSION: RecordKind = 1;
    pub const LEDGER: RecordKind = 2;
    pub const META: RecordKind = 3;
    pub const EXECUTION: RecordKind = 4;
    pub const ACTION: RecordKind = 5;
    pub const COMPLETION: RecordKind = 6;
    pub const NODE: RecordKind = 7;
    /// No longer written (theseus-hco); stores keep old `derived_from` rows,
    /// which nothing reads (the compilation carries `derived_from` itself).
    pub const EDGE: RecordKind = 8;
    pub const COMPILATION: RecordKind = 9;
    // 10 was `JUDGMENT` and 255 `CHECKPOINT`, reserved and never written.
    /// What must reach a channel (theseus-q4v): the kernel's outbox actions,
    /// kept apart from `ACTION` so no reader of an execution's work sees them.
    pub const OUTBOX: RecordKind = 11;

    pub fn name(k: RecordKind) -> &'static str {
        match k {
            SESSION => "session",
            LEDGER => "ledger",
            META => "meta",
            EXECUTION => "execution",
            ACTION => "action",
            COMPLETION => "completion",
            NODE => "node",
            EDGE => "edge",
            COMPILATION => "compilation",
            OUTBOX => "outbox",
            _ => "unknown",
        }
    }
}

/// The record header's schema field, frozen (theseus-ptx1). Builds before
/// one store format wrote each kind's own schema number there; this one
/// writes 0, and nothing reads it: the store's format and a record's own
/// bytes say what it holds.
pub const FROZEN_SCHEMA: u16 = 0;

/// A record as it will be appended. The store assigns position and time.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NewRecord {
    pub kind: RecordKind,
    /// Entity key for "latest state" lookups (a session id, a meta name).
    /// `None` for pure log rows (ledger).
    pub key: Option<String>,
    /// Scope for ordered per-owner scans (a session id): the per-session
    /// position table (§4.4b). `None` for records that belong to no session.
    pub scope: Option<String>,
    pub payload: Vec<u8>,
}

impl NewRecord {
    /// Attach a scope (a session id) so the record appears in that scope's
    /// ordered position table.
    pub fn scoped(mut self, scope: &str) -> Self {
        self.scope = Some(scope.to_string());
        self
    }

    /// A record of `kind`, its value as JSON.
    pub fn json<T: Serialize>(
        kind: RecordKind,
        key: Option<&str>,
        value: &T,
    ) -> anyhow::Result<Self> {
        Ok(Self::bytes(kind, key, serde_json::to_vec(value)?))
    }
    pub fn bytes(kind: RecordKind, key: Option<&str>, payload: Vec<u8>) -> Self {
        Self {
            kind,
            key: key.map(str::to_string),
            scope: None,
            payload,
        }
    }
}

/// A record as read back. `position` is unique and monotonic across the store.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Record {
    pub position: u64,
    pub kind: RecordKind,
    /// The header's schema field: an old record's kind's number when it was
    /// written, `FROZEN_SCHEMA` since. Nothing reads it.
    pub schema: u16,
    pub key: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub scope: Option<String>,
    pub at_unix_ms: u64,
    pub payload: Vec<u8>,
}

impl Record {
    pub fn decode<T: DeserializeOwned>(&self) -> anyhow::Result<T> {
        Ok(serde_json::from_slice(&self.payload)?)
    }
    pub fn payload_crc(&self) -> u32 {
        crc32fast::hash(&self.payload)
    }
}

pub fn now_unix_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}
