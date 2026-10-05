//! `session.history`: a session's nodes, the newest `n`, or a page either
//! way from a position (theseus-xo0m, theseus-kym3), as `ledger.tail` pages
//! the ledger.

use serde::{Deserialize, Serialize};

use crate::{ConfirmRequest, NodeInfo, SessionInfo};

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct SessionHistoryParams {
    pub session_id: String,
    /// Newest `n` nodes (default all). With `after` or `before`, a page's
    /// size, 200 by default.
    #[serde(default)]
    pub n: Option<usize>,
    /// Only nodes after this WAL position, the first `n` of them, oldest
    /// first: a walk forward passes 0, then each answer's `next`; a poll
    /// passes the last position it has.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub after: Option<u64>,
    /// Only nodes before this WAL position, the newest `n` of them: a page
    /// back, and a walk back passes each answer's `older`. Positions never
    /// move, so nodes written meanwhile neither repeat a node nor skip one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub before: Option<u64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct SessionHistoryResult {
    pub session: SessionInfo,
    pub nodes: Vec<NodeInfo>,
    /// Actions waiting for the operator's confirmation in this session.
    #[serde(default)]
    pub pending_confirms: Vec<ConfirmRequest>,
    /// With `after`: the `after` for the next page while more nodes may
    /// follow (this page's last); absent at the session's end, and
    /// without `after`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub next: Option<u64>,
    /// With `before` alone: the `before` for the next page back while older
    /// nodes remain (this page's first); absent once none do, and without
    /// `before`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub older: Option<u64>,
}
