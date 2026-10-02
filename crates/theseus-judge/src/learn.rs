//! The learning math (design §2.7, §2.9): pure functions, so the nightly
//! report (25c), the ladder (26a), and the prove (L3) all compute the same
//! numbers the same way.
//!
//! - Calibration: the Brier score, the expected calibration error (ECE), and
//!   a reliability table, for Nouls (p against the label) and for top choices
//!   (the confidence against whether the top choice was right). Judge
//!   probabilities are uncalibrated until these say otherwise (§3.7).
//! - Holdouts: time-separated and frozen. A window is a closed range of
//!   judgment times; the holdout is what falls inside, the training side is
//!   what came before, and anything later belongs to neither, so the same
//!   window gives the same holdout however much data arrives after.
//! - The minimum sample that earns a promotion: 200 labeled per deciding
//!   question, 30 per acting class, said as "labeled 37 of 200" when short.
//! - Canary arms: a session is in a pack's canary when the first 8 bytes of
//!   SHA-256(session, pack), read as a fraction, fall below the share.
//!   Sticky, and monotone as the share grows; nothing is stored for it.
//! - Rollback rules: each pack names its own; each fires on a named
//!   regression in the canary's events, and not on a near miss.
//!
//! The core gives this module plain values. Days are the core's local dates
//! (`2026-09-30`), already computed: there is no clock here.

use std::collections::{BTreeMap, BTreeSet};

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

// ------------------------------------------------------------- calibration

/// One reliability bin: predictions with `lo <= p < hi` (the last bin
/// includes 1.0).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Bin {
    pub lo: f64,
    pub hi: f64,
    pub n: usize,
    /// The mean predicted probability in the bin.
    pub mean_p: f64,
    /// How often the outcome was true in the bin.
    pub frequency: f64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Calibration {
    pub n: usize,
    /// Mean squared error of the probability against the outcome.
    pub brier: f64,
    /// Σ (n_b / n) · |frequency_b − mean_p_b| over the bins.
    pub ece: f64,
    /// Every bin, empty ones included, lowest first.
    pub bins: Vec<Bin>,
}

/// Calibration of `(p, outcome)` pairs over `bins` equal-width bins. Pairs
/// whose p is not a number in 0..=1 are left out.
pub fn calibration(predictions: &[(f64, bool)], bins: usize) -> Calibration {
    let bins = bins.max(1);
    let valid: Vec<(f64, bool)> = predictions
        .iter()
        .copied()
        .filter(|(p, _)| p.is_finite() && (0.0..=1.0).contains(p))
        .collect();
    let n = valid.len();
    let mut sums = vec![(0usize, 0.0f64, 0usize); bins];
    let mut brier = 0.0;
    for &(p, y) in &valid {
        let i = ((p * bins as f64) as usize).min(bins - 1);
        sums[i].0 += 1;
        sums[i].1 += p;
        sums[i].2 += usize::from(y);
        let y = if y { 1.0 } else { 0.0 };
        brier += (p - y) * (p - y);
    }
    let table: Vec<Bin> = sums
        .iter()
        .enumerate()
        .map(|(i, &(k, sum_p, trues))| Bin {
            lo: i as f64 / bins as f64,
            hi: (i + 1) as f64 / bins as f64,
            n: k,
            mean_p: if k == 0 { 0.0 } else { sum_p / k as f64 },
            frequency: if k == 0 { 0.0 } else { trues as f64 / k as f64 },
        })
        .collect();
    let ece = if n == 0 {
        0.0
    } else {
        table
            .iter()
            .map(|b| b.n as f64 / n as f64 * (b.frequency - b.mean_p).abs())
            .sum()
    };
    Calibration {
        n,
        brier: if n == 0 { 0.0 } else { brier / n as f64 },
        ece,
        bins: table,
    }
}

/// Precision and recall of one Choice class over `(predicted, label)` pairs;
/// `None` where the denominator is empty.
pub fn precision_recall(pairs: &[(String, String)], class: &str) -> (Option<f64>, Option<f64>) {
    let predicted = pairs.iter().filter(|(p, _)| p == class).count();
    let actual = pairs.iter().filter(|(_, l)| l == class).count();
    let hits = pairs
        .iter()
        .filter(|(p, l)| p == class && l == class)
        .count();
    let ratio = |a: usize, b: usize| (b > 0).then(|| a as f64 / b as f64);
    (ratio(hits, predicted), ratio(hits, actual))
}

/// The nearest-rank percentile (p in 0..=100) of `values`; none when empty.
pub fn percentile(values: &[u64], p: f64) -> Option<u64> {
    if values.is_empty() {
        return None;
    }
    let mut v = values.to_vec();
    v.sort_unstable();
    let rank = ((p.clamp(0.0, 100.0) / 100.0) * v.len() as f64).ceil() as usize;
    Some(v[rank.clamp(1, v.len()) - 1])
}

// ---------------------------------------------------------------- holdouts

/// A closed range of judgment times, `[start_ms, end_ms)`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Window {
    pub start_ms: u64,
    pub end_ms: u64,
}

pub const DAY_MS: u64 = 24 * 60 * 60 * 1000;

impl Window {
    /// The `days` before `end_ms` (by default the latest 14 days, ending at
    /// the local midnight the core passes, so a report run twice on one day
    /// freezes the same window).
    pub fn latest(end_ms: u64, days: u64) -> Self {
        Self {
            start_ms: end_ms.saturating_sub(days * DAY_MS),
            end_ms,
        }
    }

    pub fn contains(&self, at_ms: u64) -> bool {
        (self.start_ms..self.end_ms).contains(&at_ms)
    }
}

/// A labeled judgment, as the split sees it.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Labeled<T> {
    pub at_ms: u64,
    pub item: T,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Split<T> {
    /// Before the window: what a candidate's text and thresholds may use.
    pub train: Vec<Labeled<T>>,
    /// Inside the window: what a promotion is judged on, and nothing else.
    pub holdout: Vec<Labeled<T>>,
    /// After the window: in neither.
    pub later: usize,
}

/// The time-separated split around a frozen window.
pub fn holdout_split<T: Clone>(items: &[Labeled<T>], w: Window) -> Split<T> {
    let mut s = Split {
        train: Vec::new(),
        holdout: Vec::new(),
        later: 0,
    };
    for it in items {
        if it.at_ms < w.start_ms {
            s.train.push(it.clone());
        } else if w.contains(it.at_ms) {
            s.holdout.push(it.clone());
        } else {
            s.later += 1;
        }
    }
    s
}

/// The minimum sample a promotion needs (§2.9, Q15).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Minimum {
    pub per_deciding_question: usize,
    pub per_acting_class: usize,
}

impl Default for Minimum {
    fn default() -> Self {
        Self {
            per_deciding_question: 200,
            per_acting_class: 30,
        }
    }
}

/// Whether a holdout's labels reach the minimum, or the first shortfall in
/// words ("work_state: labeled 37 of 200").
pub fn sufficient(
    labeled_per_question: &BTreeMap<String, usize>,
    labeled_per_acting_class: &BTreeMap<String, usize>,
    deciding: &[&str],
    acting_classes: &[&str],
    min: Minimum,
) -> Result<(), String> {
    for q in deciding {
        let n = labeled_per_question.get(*q).copied().unwrap_or(0);
        if n < min.per_deciding_question {
            return Err(format!("{q}: labeled {n} of {}", min.per_deciding_question));
        }
    }
    for c in acting_classes {
        let n = labeled_per_acting_class.get(*c).copied().unwrap_or(0);
        if n < min.per_acting_class {
            return Err(format!("{c}: labeled {n} of {}", min.per_acting_class));
        }
    }
    Ok(())
}

// -------------------------------------------------------------------- arms

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Arm {
    Canary,
    Control,
}

/// The first 8 bytes of SHA-256(session, NUL, pack), as a fraction in [0, 1).
pub fn arm_fraction(session_id: &str, pack: &str) -> f64 {
    let mut h = Sha256::new();
    h.update(session_id.as_bytes());
    h.update([0u8]);
    h.update(pack.as_bytes());
    let d = h.finalize();
    let mut b = [0u8; 8];
    b.copy_from_slice(&d[..8]);
    u64::from_be_bytes(b) as f64 / 18_446_744_073_709_551_616.0
}

/// The session's arm for a pack at a canary share (0..=1).
pub fn arm(session_id: &str, pack: &str, share: f64) -> Arm {
    if arm_fraction(session_id, pack) < share {
        Arm::Canary
    } else {
        Arm::Control
    }
}

// --------------------------------------------------------------- rollbacks

/// A pack's rollback rules, named in its pack file (§2.7's table).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "rule", rename_all = "snake_case", deny_unknown_fields)]
pub enum RollbackRule {
    /// A nudged turn ends with no new tool call and a near-identical final
    /// text: a nudge loop.
    NudgeLoop,
    /// The canary's spend per task passes `ratio` times the control's median,
    /// once each arm has `min_tasks` tasks.
    SpendRatio { ratio: f64, min_tasks: usize },
    /// The operator stops or cancels a task within its nudge.
    OperatorStopWithinNudge,
    /// The judgment's on-path p95 passes `max_ms`, once there are
    /// `min_samples` live judgments.
    OnPathP95 { max_ms: u64, min_samples: usize },
    /// More than `max` Jev notices in one day.
    NoticesPerDay { max: usize },
    /// `count` operator labels of `label` in one day.
    LabelsPerDay { label: String, count: usize },
    /// More than `max` role switches in one exchange.
    SwitchesPerExchange { max: usize },
}

impl RollbackRule {
    pub fn name(&self) -> &'static str {
        match self {
            RollbackRule::NudgeLoop => "nudge_loop",
            RollbackRule::SpendRatio { .. } => "spend_ratio",
            RollbackRule::OperatorStopWithinNudge => "operator_stop_within_nudge",
            RollbackRule::OnPathP95 { .. } => "on_path_p95",
            RollbackRule::NoticesPerDay { .. } => "notices_per_day",
            RollbackRule::LabelsPerDay { .. } => "labels_per_day",
            RollbackRule::SwitchesPerExchange { .. } => "switches_per_exchange",
        }
    }

    /// Whether the rule's numbers can hold, for the loader: a spend ratio
    /// above 1, a minimum of at least one, a label that names something.
    pub fn check(&self) -> Result<(), String> {
        let ok = match self {
            RollbackRule::SpendRatio { ratio, min_tasks } => {
                ratio.is_finite() && *ratio > 1.0 && *min_tasks >= 1
            }
            RollbackRule::OnPathP95 {
                max_ms,
                min_samples,
            } => *max_ms >= 1 && *min_samples >= 1,
            RollbackRule::LabelsPerDay { label, count } => !label.trim().is_empty() && *count >= 1,
            RollbackRule::NudgeLoop
            | RollbackRule::OperatorStopWithinNudge
            | RollbackRule::NoticesPerDay { .. }
            | RollbackRule::SwitchesPerExchange { .. } => true,
        };
        if ok {
            Ok(())
        } else {
            Err(format!("{} cannot hold as written: {self:?}", self.name()))
        }
    }
}

/// What the core tells the rules, as each canary outcome lands (and again
/// from the ledger, nightly).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "event", rename_all = "snake_case")]
pub enum CanaryEvent {
    /// A turn that a nudge continued has ended.
    NudgedTurnEnded {
        task: String,
        new_tool_calls: u32,
        /// The final text before the nudge, and after it.
        final_before: String,
        final_after: String,
    },
    /// A finished task's whole spend, by arm.
    TaskSpend { task: String, arm: Arm, micros: u64 },
    /// The operator stopped or cancelled a task.
    OperatorStop { task: String, within_nudge: bool },
    /// A live judgment's time on the path.
    OnPath { ms: u64 },
    /// A notice the pack posted.
    Notice { day: String },
    /// An operator label on one of the pack's judgments.
    Label { day: String, label: String },
    /// A role switch.
    RoleSwitch { exchange: String },
}

/// A rule that fired, and why, in words for the rollback's notice.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Fired {
    pub rule: String,
    pub why: String,
}

/// How alike two texts are: the Jaccard index of their word pairs
/// (lowercase, letters and digits), or of their words when a text has
/// fewer than two. 1.0 for two empty texts.
pub fn similarity(a: &str, b: &str) -> f64 {
    let words = |s: &str| -> Vec<String> {
        s.split(|c: char| !c.is_alphanumeric())
            .filter(|w| !w.is_empty())
            .map(str::to_lowercase)
            .collect()
    };
    let (wa, wb) = (words(a), words(b));
    let grams = |w: &[String], pairs: bool| -> BTreeSet<String> {
        if pairs {
            w.windows(2).map(|p| format!("{} {}", p[0], p[1])).collect()
        } else {
            w.iter().cloned().collect()
        }
    };
    let pairs = wa.len() >= 2 && wb.len() >= 2;
    let (ga, gb) = (grams(&wa, pairs), grams(&wb, pairs));
    if ga.is_empty() && gb.is_empty() {
        return 1.0;
    }
    let inter = ga.intersection(&gb).count();
    let union = ga.union(&gb).count();
    inter as f64 / union as f64
}

/// Two final texts "near-identical" for the nudge-loop rule.
pub const NEAR_IDENTICAL: f64 = 0.9;

fn median(v: &[u64]) -> Option<f64> {
    if v.is_empty() {
        return None;
    }
    let mut s = v.to_vec();
    s.sort_unstable();
    let m = s.len() / 2;
    Some(if s.len() % 2 == 1 {
        s[m] as f64
    } else {
        (s[m - 1] as f64 + s[m] as f64) / 2.0
    })
}

/// Whether one rule fires on a pack's canary events.
#[expect(clippy::too_many_lines, reason = "shape budget: split it")]
pub fn check(rule: &RollbackRule, events: &[CanaryEvent]) -> Option<Fired> {
    let fired = |why: String| {
        Some(Fired {
            rule: rule.name().to_string(),
            why,
        })
    };
    match rule {
        RollbackRule::NudgeLoop => events.iter().find_map(|e| match e {
            CanaryEvent::NudgedTurnEnded {
                task,
                new_tool_calls: 0,
                final_before,
                final_after,
            } if similarity(final_before, final_after) >= NEAR_IDENTICAL => fired(format!(
                "task {task}: a nudged turn made no new tool call and ended as before"
            )),
            _ => None,
        }),
        RollbackRule::SpendRatio { ratio, min_tasks } => {
            let spend = |arm: Arm| -> Vec<u64> {
                events
                    .iter()
                    .filter_map(|e| match e {
                        CanaryEvent::TaskSpend { arm: a, micros, .. } if *a == arm => Some(*micros),
                        _ => None,
                    })
                    .collect()
            };
            let (canary, control) = (spend(Arm::Canary), spend(Arm::Control));
            if canary.len() < *min_tasks || control.len() < *min_tasks {
                return None;
            }
            let per_task = canary.iter().sum::<u64>() as f64 / canary.len() as f64;
            let base = median(&control)?;
            (per_task > ratio * base).then(|| Fired {
                rule: rule.name().to_string(),
                why: format!(
                    "the canary spends {per_task:.0} micro-dollars a task, over {ratio} times the control's median of {base:.0} ({} and {} tasks)",
                    canary.len(),
                    control.len()
                ),
            })
        }
        RollbackRule::OperatorStopWithinNudge => events.iter().find_map(|e| match e {
            CanaryEvent::OperatorStop {
                task,
                within_nudge: true,
            } => fired(format!("the operator stopped task {task} within its nudge")),
            _ => None,
        }),
        RollbackRule::OnPathP95 {
            max_ms,
            min_samples,
        } => {
            let ms: Vec<u64> = events
                .iter()
                .filter_map(|e| match e {
                    CanaryEvent::OnPath { ms } => Some(*ms),
                    _ => None,
                })
                .collect();
            if ms.len() < *min_samples {
                return None;
            }
            let p95 = percentile(&ms, 95.0)?;
            (p95 > *max_ms).then(|| Fired {
                rule: rule.name().to_string(),
                why: format!(
                    "on-path p95 is {p95} ms over {} judgments, past {max_ms} ms",
                    ms.len()
                ),
            })
        }
        RollbackRule::NoticesPerDay { max } => {
            let mut per_day: BTreeMap<&str, usize> = BTreeMap::new();
            for e in events {
                if let CanaryEvent::Notice { day } = e {
                    *per_day.entry(day).or_default() += 1;
                }
            }
            per_day
                .into_iter()
                .find(|(_, n)| n > max)
                .and_then(|(day, n)| fired(format!("{n} Jev notices on {day}, more than {max}")))
        }
        RollbackRule::LabelsPerDay { label, count } => {
            let mut per_day: BTreeMap<&str, usize> = BTreeMap::new();
            for e in events {
                if let CanaryEvent::Label { day, label: l } = e {
                    if l == label {
                        *per_day.entry(day).or_default() += 1;
                    }
                }
            }
            per_day
                .into_iter()
                .find(|(_, n)| n >= count)
                .and_then(|(day, n)| {
                    fired(format!(
                        "the operator labeled {n} judgments \"{label}\" on {day}"
                    ))
                })
        }
        RollbackRule::SwitchesPerExchange { max } => {
            let mut per: BTreeMap<&str, usize> = BTreeMap::new();
            for e in events {
                if let CanaryEvent::RoleSwitch { exchange } = e {
                    *per.entry(exchange).or_default() += 1;
                }
            }
            per.into_iter().find(|(_, n)| n > max).and_then(|(x, n)| {
                fired(format!(
                    "{n} role switches in exchange {x}, more than {max}"
                ))
            })
        }
    }
}

/// Every rule of a pack that fires.
pub fn check_all(rules: &[RollbackRule], events: &[CanaryEvent]) -> Vec<Fired> {
    rules.iter().filter_map(|r| check(r, events)).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A small deterministic generator (xorshift64*), for synthetic data.
    struct Rng(u64);

    impl Rng {
        fn unit(&mut self) -> f64 {
            self.0 ^= self.0 >> 12;
            self.0 ^= self.0 << 25;
            self.0 ^= self.0 >> 27;
            (self.0.wrapping_mul(0x2545_F491_4F6C_DD1D) >> 11) as f64 / (1u64 << 53) as f64
        }
    }

    #[test]
    fn known_answers_for_ece_and_brier() {
        // Three wrong and one right at 0.25: that bin is calibrated.
        let c = calibration(
            &[(0.25, false), (0.25, false), (0.25, false), (0.25, true)],
            10,
        );
        assert_eq!(c.n, 4);
        assert!(c.ece.abs() < 1e-12);
        assert!((c.brier - (3.0 * 0.0625 + 0.5625) / 4.0).abs() < 1e-12);
        assert_eq!(c.bins[2].n, 4);
        assert_eq!(c.bins[2].frequency, 0.25);
        // Always 0.9, never true: ECE 0.9, Brier 0.81.
        let c = calibration(&[(0.9, false); 10], 10);
        assert!((c.ece - 0.9).abs() < 1e-12 && (c.brier - 0.81).abs() < 1e-12);
        // 1.0 lands in the last bin; junk is left out.
        let c = calibration(&[(1.0, true), (f64::NAN, true), (1.5, false)], 10);
        assert_eq!(c.n, 1);
        assert_eq!(c.bins[9].n, 1);
        assert_eq!(calibration(&[], 10).ece, 0.0);
    }

    #[test]
    fn a_calibrated_set_has_an_ece_near_zero_and_a_shuffled_one_does_not() {
        let mut rng = Rng(0x9E37_79B9_7F4A_7C15);
        let calibrated: Vec<(f64, bool)> = (0..100_000)
            .map(|_| {
                let p = rng.unit();
                (p, rng.unit() < p)
            })
            .collect();
        let c = calibration(&calibrated, 10);
        assert!(c.ece < 0.01, "calibrated ECE {}", c.ece);
        // The same outcomes against probabilities that say the opposite.
        let inverted: Vec<(f64, bool)> = calibrated.iter().map(|&(p, y)| (1.0 - p, y)).collect();
        let s = calibration(&inverted, 10);
        assert!(s.ece > 0.3, "inverted ECE {}", s.ece);
        // And against a shuffle of the probabilities: no relation left.
        let mut ps: Vec<f64> = calibrated.iter().map(|&(p, _)| p).collect();
        for i in (1..ps.len()).rev() {
            let j = (rng.unit() * (i + 1) as f64) as usize;
            ps.swap(i, j);
        }
        let shuffled: Vec<(f64, bool)> = ps
            .iter()
            .zip(&calibrated)
            .map(|(&p, &(_, y))| (p, y))
            .collect();
        let sh = calibration(&shuffled, 10);
        assert!(sh.ece > 0.15, "shuffled ECE {}", sh.ece);
        assert!(sh.brier > c.brier);
    }

    #[test]
    fn precision_recall_and_percentiles() {
        let pairs: Vec<(String, String)> =
            [("a", "a"), ("a", "b"), ("b", "b"), ("b", "a"), ("a", "a")]
                .iter()
                .map(|(p, l)| (p.to_string(), l.to_string()))
                .collect();
        let (p, r) = precision_recall(&pairs, "a");
        assert_eq!(p, Some(2.0 / 3.0));
        assert_eq!(r, Some(2.0 / 3.0));
        assert_eq!(precision_recall(&pairs, "c"), (None, None));
        let ms: Vec<u64> = (1..=100).collect();
        assert_eq!(percentile(&ms, 50.0), Some(50));
        assert_eq!(percentile(&ms, 95.0), Some(95));
        assert_eq!(percentile(&ms, 99.0), Some(99));
        assert_eq!(percentile(&ms, 100.0), Some(100));
        assert_eq!(percentile(&[7], 95.0), Some(7));
        assert_eq!(percentile(&[], 50.0), None);
    }

    #[test]
    fn the_holdout_is_time_separated_and_frozen() {
        let day = DAY_MS;
        let items: Vec<Labeled<u32>> = (0..40)
            .map(|d| Labeled {
                at_ms: d * day + 5,
                item: d as u32,
            })
            .collect();
        let w = Window::latest(30 * day, 14);
        assert_eq!(w.start_ms, 16 * day);
        let s = holdout_split(&items, w);
        assert!(s.train.iter().all(|i| i.at_ms < w.start_ms));
        assert!(s.holdout.iter().all(|i| w.contains(i.at_ms)));
        assert_eq!(s.train.len(), 16);
        assert_eq!(s.holdout.len(), 14);
        assert_eq!(s.later, 10);
        // More data later changes nothing about the frozen window.
        let mut more = items;
        more.extend((40..60).map(|d| Labeled {
            at_ms: d * day,
            item: d as u32,
        }));
        let again = holdout_split(&more, w);
        assert_eq!(again.holdout, s.holdout);
        assert_eq!(again.train, s.train);
        assert_eq!(again.later, 30);
        // Its edges: the start is inside, the end is not.
        assert!(w.contains(w.start_ms) && !w.contains(w.end_ms));
    }

    #[test]
    fn the_minimum_sample_says_what_is_short() {
        let q = BTreeMap::from([
            ("work_state".to_string(), 37usize),
            ("announced_unfinished".to_string(), 250),
        ]);
        let c = BTreeMap::from([("progressing".to_string(), 30usize)]);
        assert_eq!(
            sufficient(
                &q,
                &c,
                &["announced_unfinished", "work_state"],
                &["progressing"],
                Minimum::default()
            ),
            Err("work_state: labeled 37 of 200".into())
        );
        let q = BTreeMap::from([("work_state".to_string(), 200usize)]);
        assert_eq!(
            sufficient(
                &q,
                &c,
                &["work_state"],
                &["progressing"],
                Minimum::default()
            ),
            Ok(())
        );
        let c = BTreeMap::from([("progressing".to_string(), 29usize)]);
        assert_eq!(
            sufficient(
                &q,
                &c,
                &["work_state"],
                &["progressing"],
                Minimum::default()
            ),
            Err("progressing: labeled 29 of 30".into())
        );
    }

    #[test]
    fn arms_are_sticky_and_monotone_as_the_share_grows() {
        let sessions: Vec<String> = (0..10_000).map(|i| format!("ses_{i:05}")).collect();
        let shares = [0.0, 0.05, 0.1, 0.2, 0.5, 0.9, 1.0];
        let mut previous: BTreeSet<&str> = BTreeSet::new();
        for share in shares {
            let canary: BTreeSet<&str> = sessions
                .iter()
                .filter(|s| arm(s, "loop", share) == Arm::Canary)
                .map(String::as_str)
                .collect();
            assert!(previous.is_subset(&canary), "monotone at {share}");
            let frac = canary.len() as f64 / sessions.len() as f64;
            assert!((frac - share).abs() < 0.02, "share {share}: {frac}");
            previous = canary;
        }
        assert_eq!(previous.len(), sessions.len());
        // Sticky: the same inputs, the same arm, every time.
        for s in sessions.iter().take(100) {
            assert_eq!(arm(s, "loop", 0.3), arm(s, "loop", 0.3));
            assert_eq!(arm_fraction(s, "loop"), arm_fraction(s, "loop"));
        }
        // Independent per pack: another pack's canary is another sample.
        let same = sessions
            .iter()
            .filter(|s| arm(s, "loop", 0.5) == arm(s, "role", 0.5))
            .count();
        assert!((4_500..5_500).contains(&same), "{same}");
    }

    fn nudged(task: &str, calls: u32, before: &str, after: &str) -> CanaryEvent {
        CanaryEvent::NudgedTurnEnded {
            task: task.into(),
            new_tool_calls: calls,
            final_before: before.into(),
            final_after: after.into(),
        }
    }

    #[test]
    fn the_nudge_loop_rule_fires_on_a_loop_and_not_on_progress() {
        let r = RollbackRule::NudgeLoop;
        let said = "Next I'll run the test suite and report the results.";
        assert!(check(&r, &[nudged("a1", 0, said, said)]).is_some());
        assert!(check(
            &r,
            &[nudged(
                "a1",
                0,
                said,
                "Next, I'll run the test suite and report the results!"
            )]
        )
        .is_some());
        // Near misses: a new tool call, or a different ending.
        assert!(check(&r, &[nudged("a1", 1, said, said)]).is_none());
        assert!(check(
            &r,
            &[nudged(
                "a1",
                0,
                said,
                "All 214 tests pass; the fix is in commit 3f2a."
            )]
        )
        .is_none());
    }

    #[test]
    fn the_spend_rule_fires_past_twice_the_control_median_with_enough_tasks() {
        let r = RollbackRule::SpendRatio {
            ratio: 2.0,
            min_tasks: 10,
        };
        let run = |canary: u64, n: usize| -> Vec<CanaryEvent> {
            let mut v: Vec<CanaryEvent> = (0..10)
                .map(|i| CanaryEvent::TaskSpend {
                    task: format!("c{i}"),
                    arm: Arm::Control,
                    micros: 1_000 + i as u64,
                })
                .collect();
            v.extend((0..n).map(|i| CanaryEvent::TaskSpend {
                task: format!("k{i}"),
                arm: Arm::Canary,
                micros: canary,
            }));
            v
        };
        // The control's median is 1,004.5; twice it is 2,009.
        assert!(check(&r, &run(2_100, 10)).is_some());
        assert!(
            check(&r, &run(2_000, 10)).is_none(),
            "1.99 times is a near miss"
        );
        assert!(
            check(&r, &run(5_000, 9)).is_none(),
            "nine tasks are too few"
        );
    }

    #[test]
    fn the_stop_and_latency_rules_fire_and_their_near_misses_do_not() {
        let stop = RollbackRule::OperatorStopWithinNudge;
        assert!(check(
            &stop,
            &[CanaryEvent::OperatorStop {
                task: "a".into(),
                within_nudge: true
            }]
        )
        .is_some());
        assert!(check(
            &stop,
            &[CanaryEvent::OperatorStop {
                task: "a".into(),
                within_nudge: false
            }]
        )
        .is_none());
        let p95 = RollbackRule::OnPathP95 {
            max_ms: 1000,
            min_samples: 20,
        };
        let lat = |slow: u64, n: usize| -> Vec<CanaryEvent> {
            (0..n)
                .map(|i| CanaryEvent::OnPath {
                    ms: if i % 10 == 0 { slow } else { 400 },
                })
                .collect()
        };
        // 1 in 10 slow: the 95th percentile is a slow one.
        assert!(check(&p95, &lat(1_200, 40)).is_some());
        assert!(check(&p95, &lat(990, 40)).is_none(), "under a second");
        assert!(check(&p95, &lat(1_200, 19)).is_none(), "too few samples");
    }

    #[test]
    fn the_daily_rules_count_within_a_day_only() {
        let notices = RollbackRule::NoticesPerDay { max: 30 };
        let day = |d: &str, n: usize| -> Vec<CanaryEvent> {
            (0..n)
                .map(|_| CanaryEvent::Notice { day: d.into() })
                .collect()
        };
        assert!(check(&notices, &day("2026-10-01", 31)).is_some());
        assert!(check(&notices, &day("2026-10-01", 30)).is_none());
        let mut two_days = day("2026-10-01", 20);
        two_days.extend(day("2026-10-02", 20));
        assert!(
            check(&notices, &two_days).is_none(),
            "40 notices over two days"
        );
        let noise = RollbackRule::LabelsPerDay {
            label: "noise".into(),
            count: 3,
        };
        let label = |d: &str, l: &str| CanaryEvent::Label {
            day: d.into(),
            label: l.into(),
        };
        assert!(check(
            &noise,
            &[
                label("d1", "noise"),
                label("d1", "noise"),
                label("d1", "noise")
            ]
        )
        .is_some());
        assert!(check(
            &noise,
            &[
                label("d1", "noise"),
                label("d1", "noise"),
                label("d2", "noise")
            ]
        )
        .is_none());
        assert!(check(
            &noise,
            &[
                label("d1", "noise"),
                label("d1", "noise"),
                label("d1", "useful")
            ]
        )
        .is_none());
        let wrong = RollbackRule::LabelsPerDay {
            label: "wrong role".into(),
            count: 2,
        };
        assert!(check(
            &wrong,
            &[label("d1", "wrong role"), label("d1", "wrong role")]
        )
        .is_some());
        assert!(check(
            &wrong,
            &[label("d1", "wrong role"), label("d2", "wrong role")]
        )
        .is_none());
    }

    #[test]
    fn the_switch_rule_counts_within_an_exchange() {
        let r = RollbackRule::SwitchesPerExchange { max: 2 };
        let sw = |x: &str| CanaryEvent::RoleSwitch { exchange: x.into() };
        assert!(check(&r, &[sw("x1"), sw("x1"), sw("x1")]).is_some());
        assert!(check(&r, &[sw("x1"), sw("x1"), sw("x2")]).is_none());
        let fired = check_all(&[r, RollbackRule::NudgeLoop], &[sw("x"), sw("x"), sw("x")]);
        assert_eq!(fired.len(), 1);
        assert_eq!(fired[0].rule, "switches_per_exchange");
    }

    #[test]
    fn similarity_is_word_pairs_and_symmetric() {
        assert_eq!(similarity("", ""), 1.0);
        assert_eq!(similarity("run the tests", "run the tests"), 1.0);
        assert_eq!(similarity("a b", "c d"), 0.0);
        let (x, y) = ("I will run the tests next.", "Next I will run the tests.");
        assert_eq!(similarity(x, y), similarity(y, x));
        assert!(similarity(x, y) < NEAR_IDENTICAL);
        assert_eq!(similarity("done", "Done!"), 1.0);
    }
}
