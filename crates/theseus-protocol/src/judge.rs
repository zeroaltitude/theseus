//! Jev's judgments (M5, steps 23a, 23b and 24): what health says of the
//! judge, `judge.list` and `judge.get`, and a notified call's score. Every
//! judgment is a `judge.call` ledger row (keyed by its id, scoped
//! `judge:<pack id>`); they stream on `ledger.tail`. A notified call's score
//! follows its notice as `judge.scored` (step 24), a judgment's one
//! notification.

use serde::{Deserialize, Serialize};

/// Health's `judge` block.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct JudgeHealth {
    /// `[judge] enabled`. Off: nothing below is counted.
    pub enabled: bool,
    /// `[judge] max_mode`: the ceiling on every pack.
    pub max_mode: String,
    /// Each wired pack and the mode it runs in now (`loop.v1: shadow`).
    pub packs: Vec<String>,
    /// The circuit breaker: `closed`, `open (Ns left)`, `half_open`, or
    /// `idle` before the first judgment builds the client.
    pub breaker: String,
    /// The breakers of their own (32d), each `<name>: <state>` as above:
    /// `rerank: closed`. Their packs' failures move only them.
    #[serde(default)]
    pub breakers: Vec<String>,
    pub in_flight: u64,
    /// The local day the counts below are of.
    pub day: String,
    /// Judgments that reached Jev today, and those that failed there.
    pub calls_today: u64,
    pub failed_today: u64,
    /// Judgments skipped today without a call: the shadow budget's pause,
    /// shedding, the breaker, an unsettled key.
    pub skipped_today: u64,
    /// What shadow judgments spent today, and the day's limit, in dollars.
    pub spend_today_usd: f64,
    pub shadow_limit_usd: f64,
    /// The shadow budget's limit is reached: shadow is paused until midnight.
    pub paused: bool,
    /// Shadow judgments shed for want of an in-flight permit since the
    /// client was built (23b).
    #[serde(default)]
    pub shed: u64,
    /// The key's state (23b): `ready`, `resolving`, `failed: <why>`, or
    /// `not configured` (`[judge] key_secret` names no `[secrets]` entry).
    /// Never a value.
    #[serde(default)]
    pub key: String,
}

/// `judge.list` (M5 23b): the newest judgments, as their `judge.call` rows,
/// without their states (`judge.get` gives one with its state).
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct JudgeListParams {
    /// One pack's (`loop.v1`); none, every pack's.
    #[serde(default)]
    pub pack: Option<String>,
    /// One session's.
    #[serde(default)]
    pub session_id: Option<String>,
    /// Those recorded at or after this time (unix ms).
    #[serde(default)]
    #[cfg_attr(test, ts(type = "number | null"))]
    pub since: Option<u64>,
    /// How many, newest kept (default 50, at most 500).
    #[serde(default)]
    #[cfg_attr(test, ts(type = "number | null"))]
    pub limit: Option<u64>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct JudgeListResult {
    /// The scopes read, one a pack (`judge:loop`): the listing's scope.
    pub scopes: Vec<String>,
    /// The judgments that matched, before the limit cut them.
    #[cfg_attr(test, ts(type = "number"))]
    pub matched: u64,
    /// The newest `limit` of them, oldest first: each a `judge.call` row
    /// (its `data` the judgment whole: pack, mode, answers with bands,
    /// timing, cost, outcome, context, and `disagrees`).
    pub judgments: Vec<crate::LedgerEntry>,
}

/// `judge.get`: one judgment by its id.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct JudgeGetParams {
    /// `jdg_…`.
    pub id: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct JudgeGetResult {
    /// Its `judge.call` row.
    pub judgment: crate::LedgerEntry,
    /// The state Jev was sent, from its blob: named fields, as JSON. `None`
    /// when the blob is missing or no longer matches its digest
    /// (`state_missing` says which).
    #[cfg_attr(test, ts(type = "unknown"))]
    pub state: Option<serde_json::Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub state_missing: Option<String>,
}

/// `judge.scored` (M5 step 24, design §2.8b): a notified call's
/// `security.v1` judgment landed, after its notice. In shadow the score is
/// uncalibrated and acts on nothing; a notice line shows it as
/// `risk 12% (shadow)`. Live progress, best effort: the judgment's
/// `judge.call` row is the record.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[serde(default)]
pub struct JudgeScored {
    pub session_id: String,
    pub turn_id: String,
    /// The call its notice named.
    pub tool_use_id: String,
    pub correlation_id: String,
    pub tool: String,
    /// `security.v1`.
    pub pack: String,
    /// The judgment's id, its `judge.call` row's key.
    pub judgment: String,
    /// `shadow`.
    pub mode: String,
    /// `risky`'s probability, 0 to 1.
    pub risky: f64,
    /// The same, as a whole percent.
    pub percent: u8,
}

impl JudgeScored {
    /// What a notice line adds: `risk 12% (shadow)`.
    pub fn line(&self) -> String {
        format!("risk {}% ({})", self.percent, self.mode)
    }
}

/// A probability as a whole percent, 0 to 100.
pub fn percent(p: f64) -> u8 {
    (p.clamp(0.0, 1.0) * 100.0).round() as u8
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_score_reads_as_a_whole_percent_in_its_mode() {
        assert_eq!(
            (percent(0.124), percent(0.995), percent(-1.0), percent(2.0)),
            (12, 100, 0, 100)
        );
        let s = JudgeScored {
            percent: percent(0.12),
            mode: "shadow".into(),
            ..Default::default()
        };
        assert_eq!(s.line(), "risk 12% (shadow)");
    }
}
