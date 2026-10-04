//! The language-server board (L2, theseus-n88g.8), as health's `lsp` block
//! shows it: each server the board started, by its server and workspace
//! root, and the last start that failed for each that is not up.

use serde::{Deserialize, Serialize};

/// One language server: up (`starting`, `loading`, or `ready`), or the last
/// start that `failed`.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct LspServerStatus {
    /// The server's name: a preset's (`rust-analyzer`), or the config's.
    pub server: String,
    /// The workspace root it serves.
    pub root: String,
    /// `starting` (before `initialize` answered), `loading` (answered, still
    /// indexing), `ready`, or `failed`.
    pub state: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub pid: Option<u32>,
    /// When it was started (or failed), in Unix ms.
    pub since_ms: u64,
    /// From its start to its readiness.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub ready_ms: Option<u64>,
    /// The resident memory of its process group, read when health is asked.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub memory_kib: Option<u64>,
    /// How long since a call last used it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub idle_secs: Option<u64>,
    /// Requests the tools sent it.
    #[serde(default)]
    pub requests: u64,
    /// The edit results that carried its diagnostics (L3).
    #[serde(default)]
    pub edit_blocks: u64,
    /// Why its last start failed, or why it ended unasked.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub why: Option<String>,
}
