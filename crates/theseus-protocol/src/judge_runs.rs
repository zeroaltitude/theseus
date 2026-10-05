//! The owner's runs over the learning ledger (M5 step 25d; design §2.9):
//! replay, audit, and backfill. Each spends money, and backfill sends the
//! owner's history to Jev, so each is the owner's alone, from a private
//! place, and the CLI refuses each inside a job.
//!
//! - **`judge.replay`** asks a candidate pack version (one the build embeds
//!   but does not wire, or a pack file's text) the incumbent's questions over
//!   a set of its recorded judgments: a report's frozen holdout, its train
//!   split, only the labeled ones the incumbent got wrong, or ids. Each state
//!   goes as it was sent when the candidate's builder, its version, and its
//!   cap equal the judgment's, else rebuilt from its inputs, or left out with
//!   the reason. A candidate that changes only thresholds makes no call: the
//!   stored answers are re-banded. Its calls are `judge.call` rows scoped
//!   `judge.replay:<pack id>`, which the nightly report never reads, and the
//!   run is one `judge.replay` row (`rpl_…`). Both sides' numbers are the
//!   learning report's, on the same judgments and labels.
//! - **`judge.audit`** has a model profile answer a pack's questions over a
//!   seeded sample of answered judgments that have no audit label yet: each
//!   answer a `judge.label` row, `source: audit`, weight 0.5. The run stops
//!   before it would pass `[judge] audit_limit_usd`; a `judge.audit` row
//!   holds it.
//! - **`judge.backfill`** rebuilds a pack's judged points from the recorded
//!   history since a date and judges them in shadow (`purpose: backfill`),
//!   one judgment per event (a second run writes nothing). It runs only
//!   under the owner's recorded consent, `[judge] backfill_consent = true`;
//!   a `judge.backfill` row records the run and the config's digest.

use serde::{Deserialize, Serialize};

use crate::learning::PackReport;

/// `judge.replay`'s params. Exactly one of `candidate` and `pack_text`.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct JudgeReplayParams {
    /// An embedded version the build does not wire, by name (`loop.v2`).
    #[serde(default)]
    pub candidate: Option<String>,
    /// A pack file's text (`--pack-file`): the daemon parses it with the
    /// loader's every rule, and opens no path.
    #[serde(default)]
    pub pack_text: Option<String>,
    /// The report whose holdout (or train split) is the set:
    /// `rpt_<date>_<pack>`.
    #[serde(default)]
    pub report: Option<String>,
    /// `holdout` (the default with a report) or `train`.
    #[serde(default)]
    pub split: Option<String>,
    /// Only the labeled judgments the incumbent got wrong.
    #[serde(default)]
    pub errors: bool,
    /// The set by id (`jdg_…`), instead of a report's.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub judgments: Vec<String>,
}

/// A judgment the replay could not ask, and why.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct ReplayLeftOut {
    pub judgment: String,
    pub reason: String,
}

/// One judgment, replayed: how its state went, and the questions the
/// candidate fixed (the incumbent wrong, the candidate right, by the label)
/// and broke (the reverse).
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct ReplayJudgment {
    /// The incumbent's judgment it re-asks.
    pub judgment: String,
    /// The candidate's judgment (`jdg_…`), when one was called.
    #[serde(default)]
    pub replayed: Option<String>,
    /// `stored` (sent as it was), `rebuilt` (from its inputs), or
    /// `rebanded` (a thresholds-only candidate: no call).
    pub state: String,
    pub fixed: Vec<String>,
    pub broken: Vec<String>,
}

/// A Choice class whose precision or recall fell with the candidate.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct ReplayClassFell {
    pub question: String,
    pub class: String,
    pub precision_fell: bool,
    pub recall_fell: bool,
}

/// One side of the planted-injection set (a security candidate's).
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct ReplayEvalSide {
    pub pack: String,
    /// Expectations met and missed, over every judged case.
    pub met: u32,
    pub missed: u32,
    /// Cases whose call failed or was skipped.
    pub unanswered: u32,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct ReplayEval {
    pub set: String,
    pub cases: u32,
    pub incumbent: ReplayEvalSide,
    pub candidate: ReplayEvalSide,
}

/// `judge.replay`'s result: the candidate beside the incumbent on the same
/// judgments, every number the learning report's.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct JudgeReplayResult {
    /// `rpl_…`: the `judge.replay` row's key.
    pub id: String,
    /// `loop.v2`, and the sha256 of its text.
    pub candidate: String,
    pub candidate_sha256: String,
    pub incumbent: String,
    /// `asks` (each state asked again) or `thresholds_only` (re-banded).
    pub change: String,
    /// `holdout`, `train`, or `judgments`.
    pub set: String,
    #[serde(default)]
    pub report: Option<String>,
    pub errors: bool,
    /// Whose labels graded both sides: `frozen` (the report's, for its
    /// holdout) or `today` (every label as of the run).
    pub labels: String,
    /// The set's judgments, those asked, and how each state went.
    pub judgments: u32,
    pub called: u32,
    pub stored: u32,
    pub rebuilt: u32,
    pub rebanded: u32,
    pub left_out: Vec<ReplayLeftOut>,
    /// The run's estimate, checked against `[judge] replay_limit_usd`, and
    /// what it cost.
    pub estimate_usd: f64,
    pub limit_usd: f64,
    pub cost_usd: f64,
    pub incumbent_report: PackReport,
    pub candidate_report: PackReport,
    /// The share of asked judgments where the candidate's top answer to
    /// every question equals the incumbent's.
    pub agreement: Option<f64>,
    pub per_judgment: Vec<ReplayJudgment>,
    pub fixed: u32,
    pub broken: u32,
    pub fell: Vec<ReplayClassFell>,
    /// A security candidate's planted-injection set, beside the incumbent.
    #[serde(default)]
    pub eval: Option<ReplayEval>,
}

/// `judge.audit`'s params.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct JudgeAuditParams {
    /// A pack version (`loop.v1`), or its id for the wired version.
    pub pack: String,
    /// How many judgments to sample.
    pub sample: u32,
    /// The model profile that answers (`[profiles.<name>]`).
    pub profile: String,
    /// The sample's seed; none takes the run's.
    #[serde(default)]
    #[cfg_attr(test, ts(type = "number | null"))]
    pub seed: Option<u64>,
}

/// `judge.audit`'s result.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct JudgeAuditResult {
    /// `aud_…`: the `judge.audit` row's key.
    pub id: String,
    pub pack: String,
    pub profile: String,
    pub model: String,
    #[cfg_attr(test, ts(type = "number"))]
    pub seed: u64,
    /// Answered judgments with no audit label, and those sampled.
    pub eligible: u32,
    pub sampled: u32,
    /// Requests sent, and those that failed.
    pub asked: u32,
    pub failed: u32,
    /// Audit labels written; answers dropped (outside the options, or not
    /// a question the pack asks).
    pub labels: u32,
    pub dropped: u32,
    /// Why the run stopped before its sample, when it did.
    #[serde(default)]
    pub stopped: Option<String>,
    pub limit_usd: f64,
    pub cost_usd: f64,
}

/// `judge.backfill`'s params.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct JudgeBackfillParams {
    /// A pack version (`loop.v1`), or its id for the wired version.
    pub pack: String,
    /// The local day the window starts (`2026-10-01`); it ends now.
    pub since: String,
}

/// `judge.backfill`'s result.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct JudgeBackfillResult {
    /// `bkf_…`: the `judge.backfill` row's key.
    pub id: String,
    pub pack: String,
    #[cfg_attr(test, ts(type = "number"))]
    pub since_ms: u64,
    #[cfg_attr(test, ts(type = "number"))]
    pub until_ms: u64,
    /// The sha256 of the config the run read its consent from.
    pub consent: String,
    /// Events in the window; those judged before (by the live point or an
    /// earlier backfill, skipped), those judged now, and those left out.
    pub events: u32,
    pub already: u32,
    pub judged: u32,
    pub failed: u32,
    pub left_out: Vec<ReplayLeftOut>,
    pub estimate_usd: f64,
    pub limit_usd: f64,
    pub cost_usd: f64,
}

/// `judge.learn`'s params (M5 25f): run the learning loop for one pack now,
/// the owner's act. `split` sets the boundary: train before it, holdout from
/// it until now (unix ms, an RFC 3339 time, a local day such as
/// `2026-10-04`, or a duration ago such as `2h`). Without it, the nightly
/// rule: interleaved below 200 labeled in the window, else the window.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct JudgeLearnParams {
    /// A version (`classify.v1`) or an id (`classify`): its lineage.
    pub pack: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub split: Option<String>,
}

/// One class's holdout numbers.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct LearnClass {
    pub class: String,
    pub precision: Option<f64>,
    pub recall: Option<f64>,
}

/// One question's holdout numbers on one side.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct LearnQuestion {
    pub question: String,
    pub labeled: u32,
    pub classes: Vec<LearnClass>,
    /// The mean of its classes' precisions, and of their recalls.
    pub precision: Option<f64>,
    pub recall: Option<f64>,
}

/// A threshold the re-fit moved (or kept), per question.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct LearnThreshold {
    pub question: String,
    pub act_was: f64,
    pub act: f64,
    pub confirm: f64,
    /// Labeled train answers the re-fit read.
    pub train: u32,
}

/// One proposal (`judge.proposal`, `prp_…`): what the learning loop did for
/// one lineage, and why. Also `judge.learn`'s result.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct JudgeProposal {
    pub id: String,
    /// The lineage's root (`classify.v1`).
    pub root: String,
    /// The version whose text was rewritten.
    pub parent: String,
    /// The candidate (`classify.v101`), once the writer gave one that loads.
    pub version: Option<String>,
    pub sha256: Option<String>,
    /// `nightly` or `owner`.
    pub trigger: String,
    /// `interleaved` or `time`, with the time split's bounds.
    pub split: String,
    pub split_start_ms: Option<u64>,
    pub split_end_ms: Option<u64>,
    /// The owner's labels the writer read (`lbl_…`), newest first, and
    /// their judgments.
    pub errors: Vec<String>,
    pub error_judgments: Vec<String>,
    /// New train errors found, of which `errors` the writer read.
    pub new_errors: u32,
    /// Labeled judgments on each side.
    pub train: u32,
    pub holdout: u32,
    pub replay: Option<String>,
    /// Train errors the candidate fixed, and labeled answers it broke.
    pub fixed: u32,
    pub broken: u32,
    pub sufficient: bool,
    pub parent_holdout: Vec<LearnQuestion>,
    pub candidate_holdout: Vec<LearnQuestion>,
    pub thresholds: Vec<LearnThreshold>,
    /// `live`, `canary`, `shadow`, `card`, `held`, `none` (too few errors),
    /// `skipped` (a budget, an open proposal), or `refused` (the writer's
    /// file).
    pub decision: String,
    pub why: String,
    /// A security pack's approval card.
    pub question: Option<String>,
    pub writer_model: Option<String>,
    pub writer_usd: f64,
    pub replay_usd: f64,
    /// The candidate's text against its parent's, line by line.
    pub diff: String,
    /// The notice's sentence.
    pub said: String,
}

/// `judge.prove`'s params (M5 L3, row 50; design §2.9, "The prove"): the
/// exit report for `loop.v1`'s canary, from the ledger. A read: it writes
/// nothing.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct JudgeProveParams {
    /// The first local day of tasks that ended (`2026-10-01`). None: since
    /// `loop.v1`'s latest move to canary, else everything.
    #[serde(default)]
    pub since: Option<String>,
    /// The last local day (`2026-10-04`), whole. None: up to now.
    #[serde(default)]
    pub until: Option<String>,
    /// Labeled tasks each arm needs before a rate is stated (default 30).
    #[serde(default)]
    pub min_tasks: Option<u32>,
    /// Labeled items each precision, recall, or rate needs (default the
    /// learning report's per acting class).
    #[serde(default)]
    pub min_labeled: Option<u32>,
    /// Also answer the records, as the generator's JSON lines.
    #[serde(default)]
    pub records: bool,
}

/// `judge.prove`'s result: the generator's report (`theseus_judge::prove`)
/// over the records the ledger gives, as its JSON and its Markdown.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct JudgeProveResult {
    /// `loop.v1`.
    pub pack: String,
    /// The window of task ends read, unix ms; none is open on that side.
    #[cfg_attr(test, ts(type = "number | null"))]
    pub since_ms: Option<u64>,
    #[cfg_attr(test, ts(type = "number | null"))]
    pub until_ms: Option<u64>,
    /// Where the window came from, in words.
    pub window: String,
    /// The verdict: `insufficient`, `canary_better`, `canary_worse`, or
    /// `no_difference`.
    pub verdict: String,
    /// The report as the generator's JSON (`theseus_judge::prove::Report`).
    #[cfg_attr(test, ts(type = "unknown"))]
    pub report: serde_json::Value,
    /// The report as the generator's Markdown: what `theseus-judge prove`
    /// writes over the same records, byte for byte.
    pub markdown: String,
    /// Finished tasks read in the window.
    pub tasks: u32,
    /// Records by arm (`canary`, `control`).
    pub arms: std::collections::BTreeMap<String, u32>,
    /// Tasks left out, by reason (`never_judged`, `no_arm`, `both_arms`,
    /// `cancelled`, `unreadable`).
    pub left_out: std::collections::BTreeMap<String, u32>,
    /// What the records cannot say yet, in words (the nudge's fields).
    pub notes: Vec<String>,
    /// Classification's decision quality against its baseline.
    #[serde(default)]
    pub classification: Vec<ClassifyQuality>,
    /// The records, as JSON lines, when asked.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub records: Option<String>,
    /// How long the build and the report took.
    #[cfg_attr(test, ts(type = "number"))]
    pub elapsed_ms: u64,
}

/// The prove's classification part (design §2.9): `classify.v1`'s decision
/// on one question against its baseline's, on the judgments an operator or
/// an audit labeled. `should_promote`'s baseline is the model's own
/// `task.create` in that turn, as the system's `task_create` label records it.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct ClassifyQuality {
    /// `classify.v1`.
    pub pack: String,
    pub question: String,
    /// Judgments in the window with an operator's or an audit's label on it.
    pub labeled: u32,
    /// Of those, the ones whose baseline is recorded: the compared set.
    pub compared: u32,
    /// Right on the compared set: Jev's lean, and the baseline's decision.
    pub jev_right: u32,
    pub baseline_right: u32,
    /// The shares right, when `compared` reaches the minimum.
    #[serde(default)]
    pub jev_rate: Option<f64>,
    #[serde(default)]
    pub baseline_rate: Option<f64>,
    /// `jev_better`, `baseline_better`, `no_difference` (McNemar's test on
    /// the pairs where they disagree, at 95%), or `insufficient`.
    pub verdict: String,
    /// Set when the sample is short: "compared 3 of 30".
    #[serde(default)]
    pub insufficient: Option<String>,
}
