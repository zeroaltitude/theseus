//! The ladder (M5 step 26a; design `m5-judgment.md` §2.7): each Jev pack
//! version's mode, held in the store as `pack.mode` rows scoped per pack id
//! (`pack:<id>`), and the operator's moves on it.
//!
//! - **The modes**: `off`, `shadow`, `canary` (with a share of sessions),
//!   `live`, and `rolled_back`, which acts as shadow. The latest row of a
//!   version is its mode; with none, the line the build wires it at. The
//!   config's `[judge] max_mode` and a pack's own `mode` lower it, never
//!   raise it.
//! - **`pack.list`**: every pack this build wires, its mode, share and why,
//!   its rollback rules, and its last rows. A read.
//! - **`pack.promote`** and **`pack.rollback`**: the owner's acts, from a
//!   private place (refused from a job's process and a shared place). A
//!   promotion short of the design's bar is the owner's to force: its row
//!   says `forced`, with the numbers. A `security.*` pack's promotion is
//!   an approval card instead, answered with `action.confirm`.

use serde::{Deserialize, Serialize};

/// `pack.list`'s answer.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct PackListResult {
    /// `[judge] enabled`: off, every pack is off whatever its rows say.
    pub enabled: bool,
    /// `[judge] max_mode`: the ceiling of every pack.
    pub max_mode: String,
    pub packs: Vec<PackInfo>,
}

/// One pack version on the ladder.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct PackInfo {
    /// `loop.v1`.
    pub pack: String,
    /// The ladder's mode: its latest row's, else `wired`'s.
    pub mode: String,
    /// A canary's share of sessions, 0 to 1.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub share: Option<f64>,
    /// What it does under the config's ceiling: `off`, `shadow`, `canary`,
    /// or `live`.
    pub acts: String,
    /// The line the build wires it at, its mode with no row.
    pub wired: String,
    /// Why it is in its mode, as health says it (`owner: decision of
    /// 2026-10-04`, `rolled back until 00:00 (notices_per_day)`).
    pub why: String,
    /// A day's brake lapses at this local midnight (unix ms).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional, type = "number"))]
    pub until_ms: Option<u64>,
    /// Its rollback rules: its file's, then those its adoption gives it.
    pub rules: Vec<String>,
    /// Its newest `pack.mode` rows, oldest first.
    pub rows: Vec<PackModeRow>,
    /// `compiled` (in the binary) or `learned` (25f: written by the
    /// learning loop, its text in the store).
    #[serde(default)]
    pub source: String,
    /// A learned version's parent, and the compiled-in version heading its
    /// lineage.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub parent: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub root: Option<String>,
    /// Whether it stands at its point now (a learned version placed, or a
    /// root no learned version displaces).
    #[serde(default)]
    pub standing: bool,
    /// The pack file's text, for the diff between versions.
    #[serde(default)]
    pub text: String,
}

/// One `pack.mode` row, as the ladder reads it.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct PackModeRow {
    /// Where the row landed, and when: the record's, not in the row's data.
    #[serde(default)]
    #[cfg_attr(test, ts(type = "number"))]
    pub position: u64,
    #[serde(default)]
    #[cfg_attr(test, ts(type = "number"))]
    pub at_unix_ms: u64,
    /// `loop.v1`.
    pub pack: String,
    /// The mode it gives the version (or asked for, when `declined`).
    pub mode: String,
    /// The mode before it.
    pub from: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub share: Option<f64>,
    /// `owner` or `system`.
    pub who: String,
    /// Who answered, as an approval names them, and through what.
    #[serde(default)]
    pub by: String,
    #[serde(default)]
    pub via: String,
    pub why: String,
    /// The learning report it cites (`rpt_<date>_<pack>`), and its holdout's
    /// bounds.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub report: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub holdout: Option<HoldoutBounds>,
    /// The owner moved it up short of the bar.
    #[serde(default)]
    pub forced: bool,
    /// What was short, in numbers (`work_state: labeled 37 of 200`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub numbers: Option<String>,
    /// A rollback's rule and its words.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub rule: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub words: Option<String>,
    /// A day's brake lapses then (unix ms, a local midnight).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional, type = "number"))]
    pub until_ms: Option<u64>,
    /// The approval card it answers (a security pack's promotion).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub question: Option<String>,
    /// The card was declined, or nobody answered it: the mode is unchanged.
    #[serde(default)]
    pub declined: bool,
}

/// A frozen holdout's window, `[start, end)`, unix ms.
#[derive(Debug, Clone, Copy, Default, PartialEq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct HoldoutBounds {
    #[cfg_attr(test, ts(type = "number"))]
    pub start_ms: u64,
    #[cfg_attr(test, ts(type = "number"))]
    pub end_ms: u64,
}

/// `pack.promote`: move a pack version up, to a canary share or live.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct PackPromoteParams {
    /// `loop.v1`.
    pub pack: String,
    /// `canary` or `live`.
    pub to: String,
    /// A canary's share, above 0 and at most 1.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub share: Option<f64>,
    /// The learning report it cites (`rpt_<date>_<pack>`); none, the latest.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub report: Option<String>,
}

/// `pack.promote`'s answer: the row it wrote, or the card it asked.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct PackPromoteResult {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub row: Option<PackModeRow>,
    /// A security pack's card: its question's id, answered with
    /// `action.confirm` (`theseus confirm <id> --approve`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub question: Option<String>,
    /// What happened, in words.
    pub said: String,
}

/// `pack.rollback`: the owner moves a pack version down to `rolled_back`.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct PackRollbackParams {
    pub pack: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub why: Option<String>,
    /// A reject (25f): the version goes `off`, not `rolled_back`.
    #[serde(default)]
    pub off: bool,
}
