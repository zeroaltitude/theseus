//! Routing (M5 25e): which interaction mode `route.v1` judged a person's
//! message to need, and why the turn ran where it ran. A turn's result
//! carries it (`TurnSubmitResult.route`), so a client's status line can say
//! the mode beside the model.

use serde::{Deserialize, Serialize};

/// How routing placed one turn.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct TurnRoute {
    /// The mode `route.v1` answered (`trivial`, `chat`, `sophisticated`,
    /// `deep_coding`, `routine_coding`, `other`), when a verdict was read.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub mode: Option<String>,
    /// `verdict`, `detour`, `capped`, `fallback`, `cache_hold`, `unsure`,
    /// `late`, `no_verdict`, `pinned`, or `shadow`.
    pub reason: String,
    /// The profile the session ran on before routing.
    pub from: String,
}
