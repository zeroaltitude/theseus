//! `hands.list` (AWS design §3.3, "Watching a hundred hands"; step 40 part
//! 2, theseus-mgw.11): each hands group, its hands' cells by state, and its
//! cost against its cap. The cockpit's grid and `theseus hands` read it.

use serde::{Deserialize, Serialize};

/// `hands.list`'s params: the newest groups first, at most `limit`
/// (default 20), only the open ones when `open` is set.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct HandsListParams {
    #[serde(default)]
    #[cfg_attr(test, ts(optional))]
    pub limit: Option<u32>,
    #[serde(default)]
    #[cfg_attr(test, ts(optional))]
    pub open: Option<bool>,
}

/// One group: its call, where its hands run, each hand's cell, and its
/// money. A cell is `waiting` (not launched yet), `running`, `stopping` (its
/// stop asked, not yet seen), `succeeded`, `failed`, `unknown`, `cancelled`
/// (stopped after a launch), or `not_launched` (cancelled before one).
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct HandsGroupInfo {
    /// The `aws.hands.run` call's correlation id.
    pub group: String,
    pub session_id: String,
    pub execution_id: String,
    pub account: String,
    pub region: String,
    /// `lambda` or `fargate`.
    pub backend: String,
    /// `all`, `first_success`, or `a quorum of n`.
    pub until: String,
    /// One per hand, in index order.
    pub cells: Vec<String>,
    pub succeeded: u32,
    pub failed: u32,
    pub running: u32,
    pub cancelled: u32,
    pub not_launched: u32,
    /// What its settled hands cost, and what its running ones hold.
    pub spent_micros: u64,
    pub reserved_micros: u64,
    /// `max_usd`, when the call named one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub cap_micros: Option<u64>,
    /// The worst case of all its hands: each one's TTL at its size's rate.
    pub worst_micros: u64,
    pub created_at_unix_ms: u64,
    /// How it ended (`met`, `not_met`, `cancelled`), once it has.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub settled: Option<String>,
    /// Its one line, as Discord shows it: "🖐️ 37/100 done, 2 failed, $1.84
    /// of $5".
    pub line: String,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct HandsListResult {
    pub groups: Vec<HandsGroupInfo>,
}
