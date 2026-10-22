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
    /// `first_loop` (the default): the turn's first request, no tool run; or
    /// `whole_turn`: the turn run again with its tools in L1 on a scratch
    /// copy of its directory, its writes discarded, the owner-facing tools
    /// stubbed (theseus-pw1q.20).
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
    /// What a `whole_turn` replay did beyond the reply: its loops, its tool
    /// calls, the paths it changed, and the deterministic comparison.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub whole: Option<WholeReplay>,
}

/// A program run that tests something: its words and how it ended.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct TestRun {
    pub argv: Vec<String>,
    /// Its exit status; none when it did not start or was ended by a signal.
    pub exit: Option<i32>,
}

/// One tool call of a whole-turn replay: what ran (`ran`), what returned a
/// recorded stub (`stub`: a tool that would reach the owner or the world),
/// and what the replay refused (`refused`).
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct WholeCall {
    pub tool: String,
    /// `ran`, `stub`, `refused` (a policy's answer: the call was not allowed
    /// here) or `failed` (a program that could not run: its start, its
    /// deadline).
    pub how: String,
    /// A program's exit status.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub exit: Option<i32>,
    /// Why a program did not run, in the words that were sent back to the model.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub why: Option<String>,
}

/// The deterministic half of a whole-turn comparison, where the turn has
/// one: whether the replay's test runs, changed paths and exit codes equal
/// the original's. `tests` is none when neither ran a test.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct WholeSame {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub tests: Option<bool>,
    pub files: bool,
    pub exits: bool,
}

/// What a `whole_turn` replay did (theseus-pw1q.20).
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct WholeReplay {
    /// Model calls made, and why the replay ended when a cap ended it
    /// (`loops`, `seconds`, `usd`).
    pub loops: u32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub capped: Option<String>,
    pub calls: Vec<WholeCall>,
    /// Paths (relative to the directory) the replay changed, and the
    /// original turn's.
    pub changed: Vec<String>,
    pub original_changed: Vec<String>,
    pub tests: Vec<TestRun>,
    pub original_tests: Vec<TestRun>,
    pub same: WholeSame,
    /// What the scratch tree could not be, or could not do: unrecorded
    /// programs in the original, paths left out of the copy.
    pub caveats: Vec<String>,
    /// Files and bytes in the scratch copy.
    #[cfg_attr(test, ts(type = "number"))]
    pub scratch_files: u64,
    #[cfg_attr(test, ts(type = "number"))]
    pub scratch_bytes: u64,
    /// The machine label this replay wrote for routing, if it wrote one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub label: Option<String>,
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
    /// The origin of the cell's turns' model (`us`, `eu`, `cn`, `unknown`),
    /// `unrecorded` for turns from before routing recorded it, `mixed` if
    /// they differ; empty for a cell no turn is in (theseus-a1wvq.11).
    #[serde(default)]
    pub origin: String,
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
    /// Whole turns replayed there (tools and all, theseus-pw1q.20), judged
    /// the same or better, and the ones whose tests ran the same.
    #[cfg_attr(test, ts(type = "number"))]
    pub whole_replayed: u64,
    #[cfg_attr(test, ts(type = "number"))]
    pub whole_matched: u64,
    #[cfg_attr(test, ts(type = "number"))]
    pub tests_same: u64,
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

/// The turns of one model origin and Jev's data verdict (theseus-a1wvq.11).
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct DataCell {
    /// `us`, `eu`, `cn`, `unknown`, or `unrecorded` for a row from before routing recorded it.
    pub origin: String,
    /// `open`, `private`, `unclear`, or `none` without a data answer.
    pub data: String,
    #[cfg_attr(test, ts(type = "number"))]
    pub turns: u64,
    pub spend_usd: f64,
}

/// Turns the data rule kept off a restricted-origin model, by why.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct GateCount {
    pub why: String,
    #[cfg_attr(test, ts(type = "number"))]
    pub turns: u64,
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
    /// Turns and spend per origin and data verdict, and the gate's skips per
    /// reason; the restricted origins' spend is what moved to them.
    #[serde(default)]
    pub data: Vec<DataCell>,
    #[serde(default)]
    pub gates: Vec<GateCount>,
    pub total_spend_usd: f64,
    pub total_movable_usd: f64,
    /// The replays' cost today against `[route_bench] replay_day_usd`.
    pub replay_today_usd: f64,
    pub replay_day_usd: f64,
    /// Machine labels the whole-turn replays fed to routing's learning loop
    /// (train split only), in the window.
    #[cfg_attr(test, ts(type = "number"))]
    pub labels_fed: u64,
    /// Why the sampler is off or stopped, when it is.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub sampler: Option<String>,
}
