//! `self.patterns` (theseus-pw1q.21.3): the pattern miner's read-only view.
//! The owner's, from any place; it runs the miner over the window on demand,
//! works while `[self] mode = "off"`, and writes nothing, not even the miner's
//! cursor. It carries shapes, counts and costs: never a value.

use serde::{Deserialize, Serialize};

/// `self.patterns`' params: the top `top` motifs (10 by default).
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct SelfPatternsParams {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub top: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub author: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub discord: Option<crate::DiscordOrigin>,
    /// The `THESEUS_SESSION` of the shell that sent it: a job's never counts.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub from_job: Option<String>,
}

/// One motif, as a row: its steps as shape texts, its arms as lines.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct SelfPatternRow {
    /// `sha256:…` of the skeleton spec.
    pub digest: String,
    /// The repository (`r:…`) or `bench:<suite>`.
    pub scope: String,
    pub bench_born: bool,
    pub places: Vec<String>,
    pub occurrences: u32,
    pub sessions: u32,
    pub days: u32,
    /// Each step's shape text, `?` before an optional one.
    pub steps: Vec<String>,
    /// Each branch arm, `merge → err:1+conflict → commit → push`.
    pub arms: Vec<String>,
    pub inputs: Vec<String>,
    pub tokens_per_occ: u64,
    pub usd_per_occ: f64,
    pub secs_per_occ: f64,
    pub decisions_per_occ: f64,
    pub saving_usd_per_day: f64,
    pub saving_secs_per_day: f64,
    /// The backlog's score for it, in milli-points.
    pub score: u64,
}

/// `self.patterns`' answer.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct SelfPatternsResult {
    pub window_days: u32,
    /// Runs and calls the window held.
    pub runs: u32,
    pub calls: u32,
    /// Motifs found, of which `rows` are the top.
    pub found: u32,
    pub rows: Vec<SelfPatternRow>,
}
