//! `execution.list`: every execution (a client older than this module sends
//! no params and is answered as before), the newest `n` by birth with a
//! cursor to page back, or only the executions `ids` names (theseus-0jet).
//! The paged forms cost the page, not every execution in the store.

use serde::{Deserialize, Serialize};

use crate::ExecutionInfo;

/// `execution.list`'s params; absent, null or `{}` answers every execution.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct ExecutionListParams {
    /// Only these executions, in the order named, an unknown id left out: a
    /// client that the push told of a change refreshes just those.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub ids: Option<Vec<String>>,
    /// The newest `n` executions by birth, newest first (at most 1,000), in
    /// place of every execution.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub n: Option<usize>,
    /// With `n`: only executions born before this cursor, an answer's
    /// `older`, to page back.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub before: Option<u64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct ExecutionListResult {
    pub executions: Vec<ExecutionInfo>,
    /// With `n`: the `before` for the next page back while older executions
    /// remain; absent at the last page, and without `n`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub older: Option<u64>,
}
