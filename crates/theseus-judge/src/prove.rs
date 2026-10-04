//! The prove report (design §2.9 "The prove", L3): the exit metrics for a
//! canary, as a pure generator over plain records. No ledger, no clock, no
//! randomness: the same records give the same report byte for byte.
//!
//! # Input
//!
//! JSON lines, one finished task per line (blank lines are skipped). The
//! core's later wire-in produces them from the ledger; each field is the
//! core's answer to a question §2.9 defines, so nothing here guesses:
//!
//! ```json
//! {"task":"t-0001","arm":"canary","success":true,"spend_micros":412000,
//!  "judge_micros":9000,"turns":7,"nudges":1,"unnecessary_nudges":0,
//!  "false_completion":false,
//!  "stops":[{"decision":"stop","should_stop":true}]}
//! ```
//!
//! - `task` (required): the task's id. Unique across the file: an arm is
//!   sticky per session, so a task is in one arm only.
//! - `arm` (required): `canary` or `control`.
//! - `success` (required): `true`, `false`, or `null` when no outcome is
//!   known yet. A task with `null` is counted and left out of every rate.
//! - `spend_micros` (required): the task's whole spend in micro-dollars,
//!   judge calls included, so arms compare at equal total budget.
//! - `judge_micros` (default 0): the part of `spend_micros` the judge spent.
//! - `turns` (required): model turns the task took.
//! - `nudges`, `unnecessary_nudges` (default 0): nudges sent, and those after
//!   which the task made no new tool call and ended the same way.
//! - `false_completion` (default `null`): for a task the baseline or Jev
//!   called complete, whether a re-ask, a near-duplicate, or the audit says it
//!   was not (`true`); `null` when nothing called it complete or no label.
//! - `stops` (default empty): each stop-or-continue decision, with
//!   `should_stop` (`true`, `false`, or `null` unlabeled).
//!
//! # Rules
//!
//! - A number is stated only when its sample supports it: fewer than
//!   [`ProveMinimum`] tasks in an arm, or labels, and the metric says
//!   "insufficient" with its counts, and carries no value.
//! - Rates come per task and per dollar. A per-dollar rate is a ratio of sums
//!   over the labeled tasks (successes over dollars), with the ratio
//!   estimator's standard error, so a task that spends more weighs more.
//! - Intervals are 95%: Wilson for proportions, Newcombe's hybrid for their
//!   difference, normal for means, ratios, and differences of independent
//!   estimates.
//! - The verdict rests on the per-dollar completion difference (canary minus
//!   control) and guards on the per-task one. Its four answers are
//!   `insufficient`, `canary_better`, `canary_worse`, `no_difference`.

use std::collections::{BTreeMap, BTreeSet};

use anyhow::{bail, Context, Result};
use serde::{Deserialize, Serialize};

/// The two-sided 95% normal quantile.
const Z: f64 = 1.959_963_984_540_054;

/// The spend ratio (canary over control) outside which the arms are not at
/// equal total spend, and the report says so.
const BALANCE: (f64, f64) = (0.8, 1.25);

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ArmName {
    Canary,
    Control,
}

impl ArmName {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Canary => "canary",
            Self::Control => "control",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum StopDecision {
    Stop,
    Continue,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Stop {
    pub decision: StopDecision,
    #[serde(default)]
    pub should_stop: Option<bool>,
}

/// One finished task: a line of the input.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TaskRecord {
    pub task: String,
    pub arm: ArmName,
    #[serde(deserialize_with = "present")]
    pub success: Option<bool>,
    pub spend_micros: u64,
    #[serde(default)]
    pub judge_micros: u64,
    pub turns: u32,
    #[serde(default)]
    pub nudges: u32,
    #[serde(default)]
    pub unnecessary_nudges: u32,
    #[serde(default)]
    pub false_completion: Option<bool>,
    #[serde(default)]
    pub stops: Vec<Stop>,
}

/// A field that may be `null` but not missing: a record without `success` is
/// a malformed record, not a task of unknown outcome.
fn present<'de, D: serde::Deserializer<'de>>(d: D) -> Result<Option<bool>, D::Error> {
    Option::<bool>::deserialize(d)
}

/// Parse the input: JSON lines. An error names its line.
pub fn parse_records(text: &str) -> Result<Vec<TaskRecord>> {
    let mut out = Vec::new();
    let mut seen = BTreeSet::new();
    for (i, line) in text.lines().enumerate() {
        if line.trim().is_empty() {
            continue;
        }
        let n = i + 1;
        let r: TaskRecord =
            serde_json::from_str(line).with_context(|| format!("line {n}: not a task record"))?;
        if r.judge_micros > r.spend_micros {
            bail!(
                "line {n}: task {}: judge_micros {} exceeds spend_micros {} (spend includes the judge's)",
                r.task,
                r.judge_micros,
                r.spend_micros
            );
        }
        if r.unnecessary_nudges > r.nudges {
            bail!(
                "line {n}: task {}: unnecessary_nudges exceeds nudges",
                r.task
            );
        }
        if !seen.insert(r.task.clone()) {
            bail!(
                "line {n}: task {} appears twice (a task is in one arm only)",
                r.task
            );
        }
        out.push(r);
    }
    Ok(out)
}

/// The smallest samples a stated number needs. Below it a metric says
/// "insufficient" with its counts.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProveMinimum {
    /// Labeled tasks in each arm, for any rate over tasks or dollars.
    pub tasks_per_arm: usize,
    /// Labeled items behind a precision, a recall, a false-completion rate, or
    /// an unnecessary-nudge rate (the §2.9 minimum per acting class).
    pub labeled_per_metric: usize,
}

impl Default for ProveMinimum {
    fn default() -> Self {
        Self {
            tasks_per_arm: 30,
            labeled_per_metric: crate::learn::Minimum::default().per_acting_class,
        }
    }
}

/// One metric: a value with its 95% interval and the sample behind it, or
/// the reason it is not stated.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Estimate {
    /// The sample: tasks, or labeled items, as the metric counts.
    pub n: u64,
    pub value: Option<f64>,
    pub lo: Option<f64>,
    pub hi: Option<f64>,
    /// Set exactly when `value` is `None`: "labeled 12 of 30".
    pub insufficient: Option<String>,
}

impl Estimate {
    fn stated(n: u64, value: f64, lo: f64, hi: f64) -> Self {
        Self {
            n,
            value: Some(value),
            lo: Some(lo),
            hi: Some(hi),
            insufficient: None,
        }
    }

    fn short(n: u64, why: String) -> Self {
        Self {
            n,
            value: None,
            lo: None,
            hi: None,
            insufficient: Some(why),
        }
    }
}

/// An estimate's standard error is kept beside it for differences; it never
/// reaches the report.
struct Est {
    est: Estimate,
    se: f64,
}

fn clamp01(x: f64) -> f64 {
    x.clamp(0.0, 1.0)
}

/// The Wilson score interval of `k` in `n`.
fn wilson(k: u64, n: u64) -> (f64, f64, f64) {
    let (k, n) = (k as f64, n as f64);
    let p = k / n;
    let z2 = Z * Z;
    let d = 1.0 + z2 / n;
    let centre = (p + z2 / (2.0 * n)) / d;
    let half = Z * (p * (1.0 - p) / n + z2 / (4.0 * n * n)).sqrt() / d;
    (p, clamp01(centre - half), clamp01(centre + half))
}

fn proportion(k: u64, n: u64, min: usize, what: &str) -> (Estimate, Option<(f64, f64, f64)>) {
    if (n as usize) < min || n == 0 {
        return (Estimate::short(n, format!("{what}: {n} of {min}")), None);
    }
    let w = wilson(k, n);
    (Estimate::stated(n, w.0, w.1, w.2), Some(w))
}

fn mean_est(xs: &[f64], min: usize, what: &str) -> Est {
    let n = xs.len();
    if n < min || n < 2 {
        return Est {
            est: Estimate::short(n as u64, format!("{what}: {n} of {min}")),
            se: f64::NAN,
        };
    }
    let m = xs.iter().sum::<f64>() / n as f64;
    let var = xs.iter().map(|x| (x - m).powi(2)).sum::<f64>() / (n as f64 - 1.0);
    let se = (var / n as f64).sqrt();
    Est {
        est: Estimate::stated(n as u64, m, m - Z * se, m + Z * se),
        se,
    }
}

/// The ratio of sums, `sum(y) / sum(x)`, with the ratio estimator's standard
/// error. `None` pairs are tasks with no usable value.
fn ratio_est(ys: &[f64], xs: &[f64], min: usize, what: &str) -> Est {
    let n = ys.len();
    let sx: f64 = xs.iter().sum();
    if n < min || n < 2 {
        return Est {
            est: Estimate::short(n as u64, format!("{what}: {n} of {min}")),
            se: f64::NAN,
        };
    }
    if sx <= 0.0 {
        return Est {
            est: Estimate::short(n as u64, format!("{what}: no spend")),
            se: f64::NAN,
        };
    }
    let r = ys.iter().sum::<f64>() / sx;
    let xbar = sx / n as f64;
    let resid: f64 = ys.iter().zip(xs).map(|(y, x)| (y - r * x).powi(2)).sum();
    let se = (resid / (n as f64 - 1.0) / n as f64).sqrt() / xbar;
    Est {
        est: Estimate::stated(n as u64, r, r - Z * se, r + Z * se),
        se,
    }
}

/// What the report says of one arm.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CohortReport {
    pub arm: ArmName,
    pub tasks: u64,
    /// Tasks with a known outcome: the ones every rate is over.
    pub labeled_tasks: u64,
    pub unlabeled_tasks: u64,
    pub successes: u64,
    /// Total spend of every task in the arm, the judge's included.
    pub spend_micros: u64,
    pub judge_micros: u64,
    /// Spend of the labeled tasks: the denominator of the per-dollar rates.
    pub labeled_spend_micros: u64,
    pub turns: u64,
    pub nudges: u64,
    pub unnecessary_nudges: u64,
    pub stops_labeled: u64,
    pub stop_true_positives: u64,
    // Per task.
    pub completion: Estimate,
    pub spend_usd_per_task: Estimate,
    pub turns_per_task: Estimate,
    pub false_completion: Estimate,
    pub unnecessary_nudge_rate: Estimate,
    // Per dollar.
    pub completions_per_usd: Estimate,
    pub turns_per_usd: Estimate,
    // Where labels exist.
    pub stop_precision: Estimate,
    pub stop_recall: Estimate,
}

struct Cohort {
    report: CohortReport,
    completion_ci: Option<(f64, f64, f64)>,
    per_usd: Est,
}

fn usd(micros: u64) -> f64 {
    micros as f64 / 1e6
}

fn cohort(arm: ArmName, rs: &[&TaskRecord], min: ProveMinimum) -> Cohort {
    let labeled: Vec<&&TaskRecord> = rs.iter().filter(|r| r.success.is_some()).collect();
    let successes = labeled.iter().filter(|r| r.success == Some(true)).count() as u64;
    let n = labeled.len() as u64;
    let (completion, completion_ci) = proportion(successes, n, min.tasks_per_arm, "labeled tasks");

    let spends: Vec<f64> = labeled.iter().map(|r| usd(r.spend_micros)).collect();
    let turns: Vec<f64> = labeled.iter().map(|r| f64::from(r.turns)).collect();
    let wins: Vec<f64> = labeled
        .iter()
        .map(|r| f64::from(u8::from(r.success == Some(true))))
        .collect();
    let per_usd = ratio_est(&wins, &spends, min.tasks_per_arm, "labeled tasks");
    let turns_per_usd = ratio_est(&turns, &spends, min.tasks_per_arm, "labeled tasks");

    // False completions: over the tasks that carry that label.
    let fc: Vec<bool> = labeled.iter().filter_map(|r| r.false_completion).collect();
    let fc_k = fc.iter().filter(|b| **b).count() as u64;
    let (false_completion, _) = proportion(
        fc_k,
        fc.len() as u64,
        min.labeled_per_metric,
        "tasks called complete and labeled",
    );

    // Unnecessary nudges: per nudge sent, over the labeled tasks.
    let nudges: u64 = rs.iter().map(|r| u64::from(r.nudges)).sum();
    let unnecessary: u64 = rs.iter().map(|r| u64::from(r.unnecessary_nudges)).sum();
    let (unnecessary_nudge_rate, _) =
        proportion(unnecessary, nudges, min.labeled_per_metric, "nudges sent");

    // Stops, where a label exists.
    let stops: Vec<&Stop> = rs
        .iter()
        .flat_map(|r| r.stops.iter())
        .filter(|s| s.should_stop.is_some())
        .collect();
    let tp = stops
        .iter()
        .filter(|s| s.decision == StopDecision::Stop && s.should_stop == Some(true))
        .count() as u64;
    let said_stop = stops
        .iter()
        .filter(|s| s.decision == StopDecision::Stop)
        .count() as u64;
    let should = stops.iter().filter(|s| s.should_stop == Some(true)).count() as u64;
    let (stop_precision, _) = proportion(
        tp,
        said_stop,
        min.labeled_per_metric,
        "labeled stop decisions",
    );
    let (stop_recall, _) = proportion(
        tp,
        should,
        min.labeled_per_metric,
        "labeled should-stop tasks",
    );

    let report = CohortReport {
        arm,
        tasks: rs.len() as u64,
        labeled_tasks: n,
        unlabeled_tasks: rs.len() as u64 - n,
        successes,
        spend_micros: rs.iter().map(|r| r.spend_micros).sum(),
        judge_micros: rs.iter().map(|r| r.judge_micros).sum(),
        labeled_spend_micros: labeled.iter().map(|r| r.spend_micros).sum(),
        turns: rs.iter().map(|r| u64::from(r.turns)).sum(),
        nudges,
        unnecessary_nudges: unnecessary,
        stops_labeled: stops.len() as u64,
        stop_true_positives: tp,
        completion,
        spend_usd_per_task: mean_est(&spends, min.tasks_per_arm, "labeled tasks").est,
        turns_per_task: mean_est(&turns, min.tasks_per_arm, "labeled tasks").est,
        false_completion,
        unnecessary_nudge_rate,
        completions_per_usd: per_usd.est.clone(),
        turns_per_usd: turns_per_usd.est,
        stop_precision,
        stop_recall,
    };
    Cohort {
        report,
        completion_ci,
        per_usd,
    }
}

/// A difference, canary minus control, with its 95% interval.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Diff {
    pub value: f64,
    pub lo: f64,
    pub hi: f64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum VerdictKind {
    Insufficient,
    CanaryBetter,
    CanaryWorse,
    NoDifference,
}

impl VerdictKind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Insufficient => "insufficient",
            Self::CanaryBetter => "canary_better",
            Self::CanaryWorse => "canary_worse",
            Self::NoDifference => "no_difference",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Verdict {
    pub kind: VerdictKind,
    /// The counts and the rule that decided it, in words.
    pub reasons: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Report {
    pub minimum: ProveMinimum,
    pub canary: CohortReport,
    pub control: CohortReport,
    /// Canary's completion rate per task minus control's.
    pub completion_diff: Option<Diff>,
    /// Canary's completions per dollar minus control's.
    pub completions_per_usd_diff: Option<Diff>,
    /// Canary's total spend over control's.
    pub spend_ratio: Option<f64>,
    /// Whether the arms are at equal total spend (within 0.8 to 1.25).
    pub spend_balanced: Option<bool>,
    pub verdict: Verdict,
}

/// Compute the report. Pure: the records, the minimum, and nothing else.
pub fn prove(records: &[TaskRecord], min: ProveMinimum) -> Report {
    let pick = |a: ArmName| -> Vec<&TaskRecord> { records.iter().filter(|r| r.arm == a).collect() };
    let canary = cohort(ArmName::Canary, &pick(ArmName::Canary), min);
    let control = cohort(ArmName::Control, &pick(ArmName::Control), min);

    // Newcombe's hybrid score interval for the difference of two proportions.
    let completion_diff = match (canary.completion_ci, control.completion_ci) {
        (Some((p1, l1, u1)), Some((p2, l2, u2))) => {
            let d = p1 - p2;
            Some(Diff {
                value: d,
                lo: d - ((p1 - l1).powi(2) + (u2 - p2).powi(2)).sqrt(),
                hi: d + ((u1 - p1).powi(2) + (p2 - l2).powi(2)).sqrt(),
            })
        }
        _ => None,
    };
    let completions_per_usd_diff = match (canary.per_usd.est.value, control.per_usd.est.value) {
        (Some(a), Some(b)) => {
            let d = a - b;
            let se = (canary.per_usd.se.powi(2) + control.per_usd.se.powi(2)).sqrt();
            Some(Diff {
                value: d,
                lo: d - Z * se,
                hi: d + Z * se,
            })
        }
        _ => None,
    };

    let (cs, ks) = (canary.report.spend_micros, control.report.spend_micros);
    let spend_ratio = (ks > 0 && cs > 0).then(|| cs as f64 / ks as f64);
    let spend_balanced = spend_ratio.map(|r| r >= BALANCE.0 && r <= BALANCE.1);

    let verdict = verdict(
        &canary.report,
        &control.report,
        &completion_diff,
        &completions_per_usd_diff,
        spend_ratio,
        spend_balanced,
        min,
    );
    Report {
        minimum: min,
        canary: canary.report,
        control: control.report,
        completion_diff,
        completions_per_usd_diff,
        spend_ratio,
        spend_balanced,
        verdict,
    }
}

#[allow(clippy::too_many_arguments)]
fn verdict(
    canary: &CohortReport,
    control: &CohortReport,
    per_task: &Option<Diff>,
    per_usd: &Option<Diff>,
    ratio: Option<f64>,
    balanced: Option<bool>,
    min: ProveMinimum,
) -> Verdict {
    let counts = |c: &CohortReport| {
        format!(
            "{}: {} tasks, {} labeled, {} successes, ${:.2} spent",
            c.arm.as_str(),
            c.tasks,
            c.labeled_tasks,
            c.successes,
            usd(c.spend_micros)
        )
    };
    let (Some(task_d), Some(usd_d)) = (per_task, per_usd) else {
        let mut reasons = vec![format!(
            "the verdict needs {} labeled tasks in each arm, with spend",
            min.tasks_per_arm
        )];
        for c in [canary, control] {
            reasons.push(counts(c));
            if let Some(why) = c
                .completions_per_usd
                .insufficient
                .as_deref()
                .or(c.completion.insufficient.as_deref())
            {
                reasons.push(format!("{} arm: {why}", c.arm.as_str()));
            }
        }
        return Verdict {
            kind: VerdictKind::Insufficient,
            reasons,
        };
    };
    let mut reasons = vec![counts(canary), counts(control)];
    reasons.push(format!(
        "completions per dollar, canary minus control: {:+.3} [{:+.3}, {:+.3}]",
        usd_d.value, usd_d.lo, usd_d.hi
    ));
    reasons.push(format!(
        "completion per task, canary minus control: {:+.3} [{:+.3}, {:+.3}]",
        task_d.value, task_d.lo, task_d.hi
    ));
    if balanced == Some(false) {
        reasons.push(format!(
            "the arms are not at equal total spend (canary over control {:.2}, equal means 0.80 to 1.25): per-dollar rates carry the comparison",
            ratio.unwrap_or(f64::NAN)
        ));
    }
    let kind = if usd_d.hi < 0.0 || task_d.hi < 0.0 {
        reasons.push("canary is worse: an interval lies wholly below zero".into());
        VerdictKind::CanaryWorse
    } else if usd_d.lo > 0.0 {
        reasons.push("canary is better per dollar, and not worse per task".into());
        VerdictKind::CanaryBetter
    } else {
        reasons.push("the per-dollar interval includes zero: no difference shown".into());
        VerdictKind::NoDifference
    };
    Verdict { kind, reasons }
}

// ---------------------------------------------------------------- Markdown

fn est(e: &Estimate, digits: usize, pct: bool) -> String {
    match (e.value, e.lo, e.hi) {
        (Some(v), Some(lo), Some(hi)) if pct => {
            format!(
                "{:.1}% [{:.1}, {:.1}] (n={})",
                v * 100.0,
                lo * 100.0,
                hi * 100.0,
                e.n
            )
        }
        (Some(v), Some(lo), Some(hi)) => {
            format!("{v:.digits$} [{lo:.digits$}, {hi:.digits$}] (n={})", e.n)
        }
        _ => format!(
            "insufficient ({})",
            e.insufficient.as_deref().unwrap_or("no sample")
        ),
    }
}

fn diff(d: &Option<Diff>, digits: usize) -> String {
    match d {
        Some(d) => format!(
            "{:+.digits$} [{:+.digits$}, {:+.digits$}]",
            d.value, d.lo, d.hi
        ),
        None => "insufficient".into(),
    }
}

/// The report as Markdown: the verdict first, then a table per unit.
pub fn markdown(r: &Report) -> String {
    use std::fmt::Write;
    let mut s = String::new();
    let _ = writeln!(s, "# Prove report: JUDGE_STOP canary against control\n");
    let _ = writeln!(s, "## Verdict: {}\n", r.verdict.kind.as_str());
    for line in &r.verdict.reasons {
        let _ = writeln!(s, "- {line}");
    }
    let _ = writeln!(
        s,
        "\nMinimum: {} labeled tasks per arm; {} labeled items per precision, recall, false-completion, or nudge rate. Intervals are 95%.\n",
        r.minimum.tasks_per_arm, r.minimum.labeled_per_metric
    );
    let _ = writeln!(s, "## Cohorts\n");
    let _ = writeln!(s, "| | canary | control |\n|---|---|---|");
    let c = [&r.canary, &r.control];
    let row = |s: &mut String, name: &str, f: &dyn Fn(&CohortReport) -> String| {
        let _ = writeln!(s, "| {name} | {} | {} |", f(c[0]), f(c[1]));
    };
    row(&mut s, "tasks", &|x| x.tasks.to_string());
    row(&mut s, "labeled tasks", &|x| x.labeled_tasks.to_string());
    row(&mut s, "outcome unknown", &|x| {
        x.unlabeled_tasks.to_string()
    });
    row(&mut s, "successes", &|x| x.successes.to_string());
    row(&mut s, "total spend (judge included)", &|x| {
        format!("${:.2}", usd(x.spend_micros))
    });
    row(&mut s, "of which the judge's", &|x| {
        format!("${:.2}", usd(x.judge_micros))
    });
    row(&mut s, "turns", &|x| x.turns.to_string());
    row(&mut s, "nudges sent", &|x| x.nudges.to_string());
    let _ = writeln!(
        s,
        "\nCanary spend over control: {}{}.\n",
        r.spend_ratio.map_or("n/a".into(), |v| format!("{v:.2}")),
        match r.spend_balanced {
            Some(true) => " (equal total spend)",
            Some(false) => " (NOT equal total spend)",
            None => "",
        }
    );
    let _ = writeln!(s, "## Per task\n");
    let _ = writeln!(s, "| metric | canary | control |\n|---|---|---|");
    row(&mut s, "completion", &|x| est(&x.completion, 3, true));
    row(&mut s, "spend per task (USD)", &|x| {
        est(&x.spend_usd_per_task, 3, false)
    });
    row(&mut s, "turns per task", &|x| {
        est(&x.turns_per_task, 2, false)
    });
    row(
        &mut s,
        "false completion (of tasks called complete)",
        &|x| est(&x.false_completion, 3, true),
    );
    row(&mut s, "unnecessary nudges (of nudges sent)", &|x| {
        est(&x.unnecessary_nudge_rate, 3, true)
    });
    row(&mut s, "stop precision (labeled stops)", &|x| {
        est(&x.stop_precision, 3, true)
    });
    row(&mut s, "stop recall (labeled should-stop)", &|x| {
        est(&x.stop_recall, 3, true)
    });
    let _ = writeln!(
        s,
        "\nCompletion per task, canary minus control: {}\n",
        diff(&r.completion_diff, 3)
    );
    let _ = writeln!(s, "## Per dollar\n");
    let _ = writeln!(s, "| metric | canary | control |\n|---|---|---|");
    row(&mut s, "completions per USD", &|x| {
        est(&x.completions_per_usd, 3, false)
    });
    row(&mut s, "turns per USD", &|x| {
        est(&x.turns_per_usd, 2, false)
    });
    let _ = writeln!(
        s,
        "\nCompletions per USD, canary minus control: {}\n",
        diff(&r.completions_per_usd_diff, 3)
    );
    s
}

/// A summary of the input's shape, for the binary: how many tasks per arm.
pub fn arm_counts(records: &[TaskRecord]) -> BTreeMap<&'static str, usize> {
    let mut m = BTreeMap::new();
    for r in records {
        *m.entry(r.arm.as_str()).or_insert(0) += 1;
    }
    m
}
