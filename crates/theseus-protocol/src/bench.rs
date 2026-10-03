//! The gates' bench history (theseus-1hk), as `bench.history` serves it to
//! the cockpit's speed wall: one row per bench run on this machine, read from
//! the CSV the gate appends (`$THESEUS_BENCH_HISTORY`, or
//! `~/.cache/theseus/bench-history.csv`). A read of a file outside the store:
//! nothing in it is written or decided by the daemon.

use serde::{Deserialize, Serialize};

/// `bench.history`'s params.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct BenchHistoryParams {
    /// The newest this many runs (default 500, at most 5,000).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub last: Option<usize>,
}

/// The history as read now.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct BenchHistoryResult {
    /// The file it reads: absent when neither the variable nor `HOME` names
    /// one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub path: Option<String>,
    /// False when there is no file: no gate on this machine has recorded a
    /// bench yet.
    pub exists: bool,
    /// The runs, oldest first.
    pub runs: Vec<BenchRun>,
    /// Runs in the file, before `last` cut them.
    pub total: u64,
    /// Lines left out, each with its number and why: a torn last line (a
    /// write cut short), or one that does not parse.
    pub skipped: Vec<String>,
}

/// One bench run: the gate's lifecycle bench, or `bench turn`, `idle`, or
/// `size`, each filling its own columns.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct BenchRun {
    /// Local time with its offset, as the bench wrote it
    /// (`2026-10-01T10:20:11-07:00`).
    pub time: String,
    /// The gate's label: the branch and `git describe`.
    pub label: String,
    /// The 1-minute load average as the run ended.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub load1: Option<f64>,
    pub passed: bool,
    /// The phases the run measured, in the file's column order.
    pub phases: Vec<BenchPhase>,
}

/// One phase of a run: `cold`, `shutdown`, `kill`, `swap`, … in ms, or one of
/// the other benches' columns in its own unit (`frames_plain` in frames,
/// `rss_start` in MB).
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct BenchPhase {
    pub phase: String,
    pub p50: f64,
    pub p95: f64,
    /// What the gate judged it against (the budget plus the margin); absent
    /// for a phase with no budget.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub limit: Option<f64>,
}
