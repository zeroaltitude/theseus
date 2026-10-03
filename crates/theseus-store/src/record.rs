//! Records: the unit the kernel appends. Kind and schema version travel with
//! every record so forward-only migrations can read old rows (§6).
//!
//! **The standing rule** (P5b, theseus-qa0 F4a): a change to what a kind's
//! records hold bumps that kind's number in [`kinds::SCHEMAS`], on the same
//! commit as the reader for the layout it replaces (serde defaults, or a
//! reader such as `Execution::from_stored`), and a test that reads the old
//! layout. The store records the newest schema written for each kind, and a
//! build that finds one newer than it knows refuses to open the store, so an
//! older binary never writes over a newer store.

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

    /// The schema this build writes for each kind, which is also the newest
    /// it reads. Schema 1 is every record written before F4a (theseus-qa0),
    /// read with serde's defaults. Schema 2 marks the kinds whose records
    /// gained fields since the store's format 2 (M2), fields an older binary
    /// would drop when it rewrote the record: the session's hold on external
    /// text (T1), an execution's wakes, report wakes, and stop (DD8, W1).
    /// Session schema 3 adds its run of failures (theseus-ljr), 4 the images
    /// its provider refused (theseus-0s4), 5 a search's query in its hold on
    /// external text (theseus-qiy), and 6 the 1-hour cache writes in its
    /// usage (theseus-ev1); serde's defaults read 2 to 5. Compilation schema
    /// 3 adds the manifest's cache layout (theseus-ev1), read from 2 as none.
    /// Node schema 3 adds a tool call's own class and its AWS call to its
    /// gate record's plan (`plan.class`, `plan.aws`, theseus-ppsd), read from
    /// 2 as neither (theseus-core's
    /// `a_tool_call_node_written_before_its_class_and_aws_reads`). Node
    /// schema 4 adds an L1 call's class to its gate record's decision
    /// (`decision.class`, theseus-7ve.1), read from 3 as none (theseus-core's
    /// `a_tool_call_node_written_before_its_l1_class_reads`). Action schema
    /// 3 adds how a cancel was verified (`verified_by`, `killed`,
    /// `survivors`, M4 18a), read from 2 as none of them (theseus-core's
    /// `an_action_written_before_its_cancels_verdict_reads`). Outbox schema 2
    /// is the same change: a post is an action, read from 1 with no verdict
    /// (the same test). Node schema 5 adds its label (M4 19a, theseus-7ve.3),
    /// read from 4 as none (`a_node_written_before_its_label_reads`);
    /// compilation schema 4 adds the manifest's audience, readers, integrity,
    /// and withheld nodes, and a
    /// context file's readers and withholding, read from 3 as none
    /// (`a_compilation_written_before_its_audience_reads`).
    /// Bump a kind here with the reader for the layout it replaces.
    pub const SCHEMAS: [(RecordKind, u16); 10] = [
        (SESSION, 6),
        (LEDGER, 1),
        (META, 1),
        (EXECUTION, 2),
        (ACTION, 3),
        (COMPLETION, 2),
        (NODE, 5),
        (EDGE, 1),
        (COMPILATION, 4),
        (OUTBOX, 2),
    ];

    /// The schema this build writes for `k`, and the newest it reads; 0 for
    /// a kind it does not know, whose records it can read at no schema.
    pub fn schema(k: RecordKind) -> u16 {
        SCHEMAS
            .iter()
            .find(|(kind, _)| *kind == k)
            .map_or(0, |(_, s)| *s)
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

    /// A record of `kind` at the schema this build writes for it.
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
            schema: kinds::schema(kind).max(1),
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
