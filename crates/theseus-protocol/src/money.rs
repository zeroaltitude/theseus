//! The owner's two words on money the kernel holds or has stopped (issue 6; theseus-x3m9, theseus-cv3v):
//! reopening an execution that ended at its dollar limit (`execution.reopen`), and settling or
//! forgiving a call whose spend is held as unknown (`action.settle`). Each is the owner's act, from
//! their own shell, and none changes a default limit.

use serde::{Deserialize, Serialize};

/// `execution.reopen`'s params: the execution, and the dollar limit it gets, which must be above
/// what it has spent.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct ExecutionReopenParams {
    pub execution_id: String,
    /// The new limit, in dollars.
    pub limit_usd: f64,
}

/// `execution.reopen`'s answer.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct ExecutionReopenResult {
    pub execution_id: String,
    pub spent_usd: f64,
    pub was_limit_usd: f64,
    pub limit_usd: f64,
}

/// `action.settle`'s params: a call held as unknown spend, and what it cost (`usd`), or none to forgive
/// it (count it as nothing, said in its row).
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct ActionSettleParams {
    pub correlation_id: String,
    /// What it cost, from the provider's bill; none forgives it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub usd: Option<f64>,
}

/// `action.settle`'s answer.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct ActionSettleResult {
    pub correlation_id: String,
    pub execution_id: String,
    /// What the call now counts as, in dollars (0 when forgiven).
    pub usd: f64,
    /// What it was held at.
    pub held_usd: f64,
    pub forgiven: bool,
}
