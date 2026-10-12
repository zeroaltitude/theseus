//! A binding's outbox as health reports it (theseus-q4v), apart from `lib.rs`,
//! whose length the shape budget caps (`scripts/long-files.txt`).

use serde::{Deserialize, Serialize};

/// A binding's outbox (theseus-q4v): what waits to reach its channels.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct OutboxStatus {
    /// Posts written and not yet delivered.
    pub pending: u64,
    /// Posts delivered, in this store's life.
    pub sent: u64,
    /// Posts the channel refused for good (a deleted channel, lost access).
    pub failed: u64,
    /// When the oldest pending post was written (unix ms); 0 with none.
    #[serde(default)]
    pub oldest_pending_ms: u64,
    /// The last delivery error, and when (unix ms).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub last_error: Option<String>,
    #[serde(default)]
    pub last_error_ms: u64,
}
