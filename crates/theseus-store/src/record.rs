//! Records: the unit the kernel appends. Kind and schema version travel with
//! every record so forward-only migrations can read old rows (§6).

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
    pub const EDGE: RecordKind = 8;
    pub const COMPILATION: RecordKind = 9;
    pub const JUDGMENT: RecordKind = 10;
    pub const CHECKPOINT: RecordKind = 255;

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
            JUDGMENT => "judgment",
            CHECKPOINT => "checkpoint",
            _ => "unknown",
        }
    }
}

/// A record as it will be appended. The store assigns position and time.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NewRecord {
    pub kind: RecordKind,
    pub schema: u16,
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

    pub fn json<T: Serialize>(
        kind: RecordKind,
        key: Option<&str>,
        value: &T,
    ) -> anyhow::Result<Self> {
        Ok(Self {
            kind,
            schema: 1,
            key: key.map(str::to_string),
            scope: None,
            payload: serde_json::to_vec(value)?,
        })
    }
    pub fn bytes(kind: RecordKind, key: Option<&str>, payload: Vec<u8>) -> Self {
        Self {
            kind,
            schema: 1,
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
