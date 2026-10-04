//! Jev's judgments (M5, steps 23a and 23b): what health says of the judge,
//! and `judge.list` and `judge.get`. Every judgment is a `judge.call` ledger
//! row (keyed by its id, scoped `judge:<pack id>`); they stream on
//! `ledger.tail`, with no notification of their own.

use serde::{Deserialize, Serialize};

/// Health's `judge` block.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct JudgeHealth {
    /// `[judge] enabled`. Off: nothing below is counted.
    pub enabled: bool,
    /// `[judge] max_mode`: the ceiling on every pack.
    pub max_mode: String,
    /// Each wired pack and the mode it runs in now (`loop.v1: shadow`).
    pub packs: Vec<String>,
    /// The circuit breaker: `closed`, `open (Ns left)`, `half_open`, or
    /// `idle` before the first judgment builds the client.
    pub breaker: String,
    pub in_flight: u64,
    /// The local day the counts below are of.
    pub day: String,
    /// Judgments that reached Jev today, and those that failed there.
    pub calls_today: u64,
    pub failed_today: u64,
    /// Judgments skipped today without a call: the shadow budget's pause,
    /// shedding, the breaker, an unsettled key.
    pub skipped_today: u64,
    /// What shadow judgments spent today, and the day's limit, in dollars.
    pub spend_today_usd: f64,
    pub shadow_limit_usd: f64,
    /// The shadow budget's limit is reached: shadow is paused until midnight.
    pub paused: bool,
    /// Shadow judgments shed for want of an in-flight permit since the
    /// client was built (23b).
    #[serde(default)]
    pub shed: u64,
    /// The key's state (23b): `ready`, `resolving`, `failed: <why>`, or
    /// `not configured` (`[judge] key_secret` names no `[secrets]` entry).
    /// Never a value.
    #[serde(default)]
    pub key: String,
}

/// `judge.list` (M5 23b): the newest judgments, as their `judge.call` rows,
/// without their states (`judge.get` gives one with its state).
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct JudgeListParams {
    /// One pack's (`loop.v1`); none, every pack's.
    #[serde(default)]
    pub pack: Option<String>,
    /// One session's.
    #[serde(default)]
    pub session_id: Option<String>,
    /// Those recorded at or after this time (unix ms).
    #[serde(default)]
    #[cfg_attr(test, ts(type = "number | null"))]
    pub since: Option<u64>,
    /// How many, newest kept (default 50, at most 500).
    #[serde(default)]
    #[cfg_attr(test, ts(type = "number | null"))]
    pub limit: Option<u64>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct JudgeListResult {
    /// The scopes read, one a pack (`judge:loop`): the listing's scope.
    pub scopes: Vec<String>,
    /// The judgments that matched, before the limit cut them.
    #[cfg_attr(test, ts(type = "number"))]
    pub matched: u64,
    /// The newest `limit` of them, oldest first: each a `judge.call` row
    /// (its `data` the judgment whole: pack, mode, answers with bands,
    /// timing, cost, outcome, context, and `disagrees`).
    pub judgments: Vec<crate::LedgerEntry>,
}

/// `judge.get`: one judgment by its id.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct JudgeGetParams {
    /// `jdg_…`.
    pub id: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct JudgeGetResult {
    /// Its `judge.call` row.
    pub judgment: crate::LedgerEntry,
    /// The state Jev was sent, from its blob: named fields, as JSON. `None`
    /// when the blob is missing or no longer matches its digest
    /// (`state_missing` says which).
    #[cfg_attr(test, ts(type = "unknown"))]
    pub state: Option<serde_json::Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub state_missing: Option<String>,
}
