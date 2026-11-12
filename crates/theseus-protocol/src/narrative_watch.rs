//! `narrative.watch`'s params (theseus-nuqut): the sessions a watch is for.

use serde::{Deserialize, Serialize};

/// `narrative.watch`'s params. Every field is optional, and no params at all
/// (null) is the old call: every session's lines.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct NarrativeWatchParams {
    /// Watch only these sessions: the returned tail is their own last lines
    /// (a quiet session's are not crowded out by a noisy one's), and only
    /// their lines arrive after. Empty or absent is every session.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub session_ids: Vec<String>,
}
