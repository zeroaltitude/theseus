//! Proposed extensions' surfaces (M7 43a): `extend.list` (`theseus extend
//! list`), and health's count of proposals. Each name carries `Extend`,
//! since the web apps' types share one namespace.

use serde::{Deserialize, Serialize};

/// One proposal, as its manifest has it now.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct ExtendInfo {
    pub name: String,
    /// The frozen tree's SHA-256.
    pub digest: String,
    /// `proposed` (its question waits), `failed` (it did not come up in L1,
    /// so nothing was asked), `acked`, or `declined`.
    pub state: String,
    pub description: String,
    pub command: Vec<String>,
    /// The workspace directory it was frozen from, and the frozen copy.
    pub source: String,
    pub frozen: String,
    /// The tools it listed in L1.
    pub tools: Vec<String>,
    pub passed: u64,
    pub tests: u64,
    /// The hosts it asked to reach; empty: no network.
    #[serde(default)]
    pub network: Vec<String>,
    /// Why it did not come up in L1.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub error: Option<String>,
    pub session_id: String,
    /// The question that asks the operator (`theseus confirm <it>`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub question: Option<String>,
    pub proposed_at_ms: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub answered_by: Option<String>,
}

/// `extend.list`: every proposal, newest first.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct ExtendListResult {
    pub extensions: Vec<ExtendInfo>,
}

/// Health's count of proposals, by state. Absent when there are none.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct ExtendHealth {
    pub proposals: u64,
    /// Waiting for the operator's ack.
    pub waiting: u64,
    pub acked: u64,
    pub declined: u64,
    pub failed: u64,
}
