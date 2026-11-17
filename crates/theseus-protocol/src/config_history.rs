//! `config.history` (issue 22, undo): the last changes the owner approved to
//! Theseus's own settings, as words. A read. The daemon builds the words from
//! its ledger and the copies it keeps; a surface only shows them.

use serde::{Deserialize, Serialize};

/// `config.history`'s params: how many changes, newest first (at most the
/// ten the daemon keeps copies of).
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct ConfigHistoryParams {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub limit: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub author: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub discord: Option<crate::DiscordOrigin>,
}

/// `config.history`'s answer: how many changes, and the words every surface
/// shows.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct ConfigHistoryResult {
    pub changes: u32,
    pub words: String,
}
