//! Extensions' surfaces (M7 43a, 43b): `extend.list` (`theseus extend
//! list`, Discord's `/extensions`), `extension.revoke` (`theseus extend
//! revoke`), and health's counts. Each name carries `Extend` or
//! `Extension`, since the web apps' types share one namespace.

use serde::{Deserialize, Serialize};

/// One proposal, as its manifest has it now.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct ExtendInfo {
    pub name: String,
    /// The frozen tree's SHA-256.
    pub digest: String,
    /// `proposed` (its question waits), `failed` (it did not come up in L1,
    /// so nothing was asked), `acked` (and loaded, 43b), `declined`,
    /// `replaced` (a later version of its name was acked), or `revoked`.
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

/// A loaded extension (43b): what the ack loaded, and its server now.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct ExtendLoadedInfo {
    pub name: String,
    /// Its server on the board: `ext-<name>`.
    pub server: String,
    pub digest: String,
    pub description: String,
    pub command: Vec<String>,
    /// The frozen copy that runs.
    pub frozen: String,
    /// Its tools, `mcp:ext-<name>/<tool>`, as the ack loaded them.
    pub tools: Vec<String>,
    /// The hosts it may reach; empty: no network.
    #[serde(default)]
    pub network: Vec<String>,
    pub acked_by: String,
    pub acked_via: String,
    pub acked_at_ms: u64,
    /// The session that proposed it.
    pub session_id: String,
    /// The digest this one replaced, when it was a new version.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub replaced: Option<String>,
    /// Its server's state on the board (`ready`, `starting`, `failed`, …).
    pub state: String,
    pub calls: u64,
    pub errors: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub last_error: Option<String>,
}

/// `extend.list`: every proposal, newest first, and the loaded ones.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct ExtendListResult {
    pub extensions: Vec<ExtendInfo>,
    /// The loaded extensions, by name (43b).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub loaded: Vec<ExtendLoadedInfo>,
}

/// `extension.revoke` (43b): the operator's, from a private place.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct ExtensionRevokeParams {
    pub name: String,
    /// Who presses, as a client names them (Discord's button).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub author: Option<String>,
    /// Where the press came from, which only the Discord binding names.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub discord: Option<crate::DiscordOrigin>,
}

/// What a revoke did: the server stopped, its tools dropped from the next
/// turn; the frozen copy stays on disk.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct ExtensionRevokeResult {
    pub name: String,
    pub digest: String,
    pub tools: Vec<String>,
    /// The frozen copy, kept.
    pub frozen: String,
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
    /// Loaded now (43b).
    #[serde(default, skip_serializing_if = "is_zero")]
    pub loaded: u64,
}

fn is_zero(n: &u64) -> bool {
    *n == 0
}
