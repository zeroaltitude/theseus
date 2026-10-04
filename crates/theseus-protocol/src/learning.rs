//! The learning ledger (M5 step 25c; design §2.9, spec §3.10): labels on
//! Jev's judgments, and the report per pack, version and question that says
//! how right they were.
//!
//! - **A label** is a `judge.label` row, never an edit: keyed `lbl_<id>`,
//!   scoped as its judgment (`judge:<pack id>`), naming the judgment, the
//!   question (or none: all of them), the label, its source (`operator`,
//!   `system`, `audit`), who and through what, a weight, and a note. What a
//!   label holds depends on the question's kind:
//!   - a **Noul**: `true` or `false`, the statement's truth;
//!   - a **Choice**: the right option's id (`"progressing"`), or
//!     `{"not": "<option>"}` when only a wrong one is known (a false
//!     completion says `{"not": "complete"}`);
//!   - a **Score**: the right level, 0-based.
//!
//!   On every kind, `"right"` and `"wrong"` say the answer's own lean was
//!   right or wrong; on the whole judgment (no question) they say it of every
//!   answer, and `"noise"` and `"useful"` are recorded for the ladder's rules
//!   (26a) and count toward no question.
//! - **The report** is written nightly by the core's learning tender and on
//!   demand (`learning.report` without a date), as one `judge.report` row
//!   per pack version and as `<state dir>/learning/<date>.json`, a file the
//!   rows rebuild. With a date, `learning.report` reads the stored one.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

/// `judge.label`: an operator's label on one judgment (acting: the owner,
/// from a private place).
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct JudgeLabelParams {
    /// `jdg_…`.
    pub judgment: String,
    /// The question it labels (`work_state`); none labels the whole
    /// judgment.
    #[serde(default)]
    pub question: Option<String>,
    /// The label: as the module says, by the question's kind.
    #[cfg_attr(test, ts(type = "unknown"))]
    pub label: serde_json::Value,
    #[serde(default)]
    pub note: Option<String>,
    /// Set by the Discord binding alone (a notice's right / wrong / noise
    /// press, step 24's notices): the channel and user the press came from,
    /// which the core judges the label by, as it judges an answer.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub discord: Option<crate::DiscordOrigin>,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct JudgeLabelResult {
    /// `lbl_…`, the row's key.
    pub id: String,
    pub judgment: String,
    /// `loop.v1`.
    pub pack: String,
    pub question: Option<String>,
    #[cfg_attr(test, ts(type = "unknown"))]
    pub label: serde_json::Value,
    /// `operator`.
    pub source: String,
    pub weight: f64,
}

/// `learning.report`: with `date`, the report stored for that local day;
/// without, the report run now (as the tender runs it), written and
/// returned. `pack` keeps one pack's (`loop.v1`, or `loop` for every
/// version).
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct LearningReportParams {
    #[serde(default)]
    pub pack: Option<String>,
    /// `2026-10-04`, a local day.
    #[serde(default)]
    pub date: Option<String>,
}

/// One report: every pack version's, as of its run.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct LearningReport {
    /// The local day it is of.
    pub date: String,
    #[cfg_attr(test, ts(type = "number"))]
    pub at_unix_ms: u64,
    /// `nightly`, `missed` (a night the daemon was down, run once after the
    /// next start), or `on_demand`.
    pub trigger: String,
    /// The labels the report read, by source, and the system labels this
    /// run derived and wrote (a second run writes none again).
    pub labels: LabelCounts,
    pub packs: Vec<PackReport>,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct LabelCounts {
    pub operator: u32,
    pub system: u32,
    pub audit: u32,
    pub system_written: u32,
}

/// One pack version's report: every judgment of it up to the run, and its
/// frozen holdout.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct PackReport {
    /// `loop.v1`.
    pub pack: String,
    pub calls: u32,
    pub answered: u32,
    pub failed: u32,
    pub skipped: u32,
    /// Judgments with at least one label that counts toward a question.
    pub labeled: u32,
    /// What Jev is set beside (the pack's `baseline`).
    pub baseline: String,
    /// Answered judgments that agree with the baseline, as a share: one
    /// agrees unless its deciding questions reach the act verdict (23b's
    /// `disagrees`, the rule behind `theseus.judge.disagreements`). None
    /// with none answered.
    pub agreement: Option<f64>,
    pub disagreements: u32,
    pub cost_usd: f64,
    /// Latency per workload class (`task`, `reply`, `tools`, `inbound`, …):
    /// each call that reached Jev, whole (`timing.total_ms`).
    pub latency: Vec<LatencyRow>,
    pub questions: Vec<QuestionReport>,
    pub holdout: Holdout,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct LatencyRow {
    pub class: String,
    pub n: u32,
    #[cfg_attr(test, ts(type = "number | null"))]
    pub p50_ms: Option<u64>,
    #[cfg_attr(test, ts(type = "number | null"))]
    pub p95_ms: Option<u64>,
    #[cfg_attr(test, ts(type = "number | null"))]
    pub p99_ms: Option<u64>,
}

/// One question's numbers, over a set of judgments.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct QuestionReport {
    pub question: String,
    /// `choice`, `noul`, or `score`.
    pub kind: String,
    pub decides: bool,
    pub answered: u32,
    pub labeled: u32,
    /// Answers by band (`act`, `confirm`, `escalate`), each with its share.
    pub bands: Vec<BandShare>,
    /// A Choice's precision and recall, per class.
    pub classes: Vec<ClassReport>,
    /// A Noul's p against its label; a Choice's or Score's confidence
    /// against whether its top answer was right. None with nothing labeled.
    pub calibration: Option<Calibration>,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct BandShare {
    pub band: String,
    pub n: u32,
    pub share: f64,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct ClassReport {
    pub class: String,
    /// Labeled judgments whose top answer was this class, and whose label is.
    pub predicted: u32,
    pub actual: u32,
    pub precision: Option<f64>,
    pub recall: Option<f64>,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct Calibration {
    pub n: u32,
    pub brier: f64,
    pub ece: f64,
    /// The reliability table: ten equal bins, empty ones included.
    pub bins: Vec<ReliabilityBin>,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct ReliabilityBin {
    pub lo: f64,
    pub hi: f64,
    pub n: u32,
    pub mean_p: f64,
    pub frequency: f64,
}

/// The holdout: a closed window of judgment times, `[start, end)`, the
/// latest 14 days before the report's local midnight, frozen with the
/// judgments inside it and their labels as of the run. Later judgments
/// belong to neither side.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct Holdout {
    #[cfg_attr(test, ts(type = "number"))]
    pub start_ms: u64,
    #[cfg_attr(test, ts(type = "number"))]
    pub end_ms: u64,
    /// The answered judgments inside the window, by id.
    pub judgments: Vec<String>,
    /// The labels on them that count, by id.
    pub labels: Vec<String>,
    /// Answered judgments before the window (what a candidate may be
    /// written from), and after it.
    pub train: u32,
    pub later: u32,
    pub labeled_per_question: BTreeMap<String, u32>,
    /// Labeled judgments whose top answer is a class that acts
    /// (`loop.v1`'s `progressing`).
    pub labeled_per_acting_class: BTreeMap<String, u32>,
    /// The minimum (200 labeled per deciding question, 30 per acting
    /// class) is met.
    pub sufficient: bool,
    /// What is short, when it is not: `insufficient: work_state: labeled 3
    /// of 200`.
    pub insufficient: Option<String>,
    /// The holdout's own numbers, per question.
    pub questions: Vec<QuestionReport>,
}
