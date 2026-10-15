//! `prefix.report` (theseus-rr15): the prefix library's picks, what they saved
//! and the false fits, for `theseus prefix report`.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[serde(deny_unknown_fields)]
pub struct PrefixReportParams {
    /// The newest picks read, at most (2,000).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub limit: Option<u32>,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct PrefixReport {
    /// Whether `[prefix] enabled` is on.
    pub enabled: bool,
    /// The picks read, newest last, and how many there were in all.
    pub picks: u64,
    pub reuses: u64,
    pub coarser: u64,
    pub mints: u64,
    /// Reuses (and coarser) over picks; mints over picks.
    pub reuse_rate: f64,
    pub mint_rate: f64,
    /// Picks that reused a header and whose turn's first call read the cache.
    pub cache_hits: u64,
    /// The header tokens those calls read instead of writing, and the money.
    pub writes_saved_tokens: u64,
    pub writes_saved_usd: f64,
    /// Reuses below `act`, and those that went wrong after.
    pub below_act: u64,
    pub false_fits: Vec<FalseFit>,
    /// The library in memory: its headers, its size against its cap, and the
    /// headers warm on some model now.
    pub headers: u64,
    pub bytes: u64,
    pub max_bytes: u64,
    pub warm: u64,
}

/// A reuse below `act` after which the turn went wrong: a labelled false fit.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct FalseFit {
    pub session_id: String,
    pub turn_id: String,
    pub at_unix_ms: u64,
    pub choice: String,
    pub fit: f64,
    pub header: String,
    /// What went wrong: `correction` (the owner corrected the route or a
    /// fact) or `re_ask` (the owner asked again).
    pub why: String,
}
