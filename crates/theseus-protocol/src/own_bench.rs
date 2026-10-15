//! Your own benchmark (own-benchmark, theseus-pw1q.12): `turn.replay`, a past
//! turn's first request rebuilt and sent to another profile, and
//! `route.report`, what the owner's own turns say about which model they
//! need. Types only; the pitch is "tells you which model you actually need,
//! measured on your own past work" (killer-features §4.5).

use serde::{Deserialize, Serialize};

/// `turn.replay`'s params. Refused when the turn's first request cannot be
/// rebuilt byte for byte; it names which part.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct TurnReplayParams {
    /// The turn's session (a turn id does not name it).
    pub session: String,
    pub turn: String,
    /// The profile the request is sent to.
    pub profile: String,
    /// The effort it runs at (`low` to `max`); the profile's own when absent.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub effort: Option<String>,
    /// `first_loop`, the only scope: the turn's first request, no tool run.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub scope: Option<String>,
    /// Judge the two replies with `compare.v1` (default true).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub compare: Option<bool>,
}

/// One reply, as the comparison reads it: its text and the calls it asked
/// for (names only; a replay runs none).
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct ReplayReply {
    pub model: String,
    pub text: String,
    pub tool_calls: Vec<String>,
    #[cfg_attr(test, ts(type = "number"))]
    pub input_tokens: u64,
    #[cfg_attr(test, ts(type = "number"))]
    pub output_tokens: u64,
    pub cost_usd: f64,
}

/// `compare.v1`'s word on a replay.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct ReplayVerdict {
    /// `same`, `weaker`, `different`, `better`, or `other`.
    pub verdict: String,
    /// Jev's probability of the verdict.
    pub p: f64,
    /// `same` and `better` together: what the report's bar reads.
    pub same_or_better: f64,
    pub judgment: String,
    /// `live` or `shadow`: the pack's mode when it answered.
    pub mode: String,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct TurnReplayResult {
    pub session: String,
    pub turn: String,
    /// The profile and model that answered.
    pub profile: String,
    pub effort: Option<String>,
    pub scope: String,
    /// The request's digest, which matched the original's.
    pub request_digest: String,
    pub original: ReplayReply,
    pub replay: ReplayReply,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub verdict: Option<ReplayVerdict>,
    /// Why there is no verdict, when there is none.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub unjudged: Option<String>,
    /// The replay's cost and the judgment's.
    pub cost_usd: f64,
    #[cfg_attr(test, ts(type = "number"))]
    pub elapsed_ms: u64,
}

/// `route.report`'s params: the window is a week with `week`, else a day.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct RouteReportParams {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub week: Option<bool>,
}

/// One (model, effort) cell of the owner's work.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct RouteCell {
    pub model: String,
    /// The effort the turns ran at; empty: the profile's own.
    pub effort: String,
    #[cfg_attr(test, ts(type = "number"))]
    pub turns: u64,
    pub spend_usd: f64,
    /// The profile one rung down the routing table, when there is one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub rung_down: Option<String>,
    /// Turns replayed there, and judged `same` or `better` at the bar.
    #[cfg_attr(test, ts(type = "number"))]
    pub replayed: u64,
    #[cfg_attr(test, ts(type = "number"))]
    pub matched: u64,
    /// Turns the owner checked (a label on their routing, or a correction).
    #[cfg_attr(test, ts(type = "number"))]
    pub checked: u64,
    /// The replays' first requests against the originals': the share of the
    /// cost a rung down would keep (0 to 1), when any replay is judged.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub cost_ratio: Option<f64>,
    /// The lower bound of the match rate at the stated confidence.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub match_floor: Option<f64>,
    /// What moving the cell's turns one rung down would have saved at that
    /// floor: spend, times the floor, times the saving.
    pub movable_usd: f64,
    /// What the replays and their judgments cost.
    pub replay_usd: f64,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct RouteReportResult {
    #[cfg_attr(test, ts(type = "number"))]
    pub window_days: u64,
    /// The bar a judged replay must reach to count as matched.
    pub bar: f64,
    /// The confidence the floor is stated at (0.9: a one-sided bound).
    pub confidence: f64,
    pub cells: Vec<RouteCell>,
    pub total_spend_usd: f64,
    pub total_movable_usd: f64,
    /// The replays' cost today against `[route] replay_day_usd`.
    pub replay_today_usd: f64,
    pub replay_day_usd: f64,
    /// Why the sampler is off or stopped, when it is.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub sampler: Option<String>,
}
