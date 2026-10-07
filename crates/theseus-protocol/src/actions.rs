//! `action.list`: the actions, each a tool or provider call with its
//! correlation id (spec §3.16). The newest `n`, an execution's, or every one
//! not settled, however old (theseus-hnof.3), which the cockpit's Ship and its
//! watch read so that a job running for hours never drops out of their list.

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::CancelVerdict;

/// One action (a tool or provider call with a correlation id, spec §3.16).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct ActionInfo {
    pub correlation_id: String,
    pub execution_id: String,
    pub session_id: String,
    pub tool: String,
    pub state: String,
    pub retry_class: String,
    pub planned_at_ms: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub authorized_at_ms: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub dispatched_at_ms: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub settled_at_ms: Option<u64>,
    pub deadline_at_ms: u64,
    /// What the action's budget reservation holds, in US dollars.
    #[serde(default)]
    pub reserved_usd: f64,
    pub confirmed: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub cancel: Option<String>,
    /// The cancel's verdict (M4 18a): how it knows the call stopped.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub verdict: Option<CancelVerdict>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub external_op_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub result_ref: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub resolution: Option<String>,
    pub completions_seen: u32,
    /// An L1 job's egress (M4 18c), as its completion's `detail.egress`
    /// keeps it: its list, the hosts it reached, and the refusals.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional, type = "unknown"))]
    pub egress: Option<Value>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct ActionListParams {
    /// Only this execution's actions.
    #[serde(default)]
    pub execution_id: Option<String>,
    /// Newest `n` (default 200; with `unsettled`, every one). At most 2,000.
    #[serde(default)]
    pub n: Option<usize>,
    /// Only the actions not settled, however old (theseus-hnof.3): planned,
    /// authorized or dispatched (a question waiting, a job running), or of
    /// an unknown outcome, which a late result can still settle. Read from
    /// the kernel's open actions, never from every action, so a job that has
    /// run for hours stays in the list however many calls came after it.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub unsettled: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct ActionListResult {
    /// Newest first.
    pub actions: Vec<ActionInfo>,
    /// Every action the record holds, whatever the filter.
    pub total: u64,
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The filter is absent from the bytes when off, so an older peer's
    /// params keep their bytes, and params without it decode with it off.
    #[test]
    fn unsettled_is_off_unless_asked_and_absent_when_off() {
        let old: ActionListParams =
            serde_json::from_str(r#"{"execution_id":null,"n":500}"#).unwrap();
        assert!(!old.unsettled);
        assert_eq!(old.n, Some(500));
        assert_eq!(
            serde_json::to_string(&old).unwrap(),
            r#"{"execution_id":null,"n":500}"#
        );
        let asked: ActionListParams = serde_json::from_str(r#"{"unsettled":true}"#).unwrap();
        assert!(asked.unsettled);
        assert_eq!((asked.execution_id, asked.n), (None, None));
        let sent = ActionListParams {
            unsettled: true,
            ..Default::default()
        };
        assert_eq!(
            serde_json::to_string(&sent).unwrap(),
            r#"{"execution_id":null,"n":null,"unsettled":true}"#
        );
    }
}
