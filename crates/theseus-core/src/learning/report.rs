//! The report's numbers (design §2.9), per pack version and question, every
//! one from `theseus_judge::learn`: precision and recall per Choice class,
//! Brier, ECE and the reliability table, latency percentiles, the holdout's
//! split and its minimum. Pure: the run reads the scope and gives it here.

use std::collections::{BTreeMap, BTreeSet};

use theseus_judge::band::{Band, Top};
use theseus_judge::client::Kind;
use theseus_judge::judge::AnswerRecord;
use theseus_judge::learn::{self, Labeled, Minimum, Window};
use theseus_judge::{Outcome, Pack};
use theseus_protocol::learning::{
    BandShare, Calibration, ClassReport, Holdout, LatencyRow, PackReport, QuestionReport,
    ReliabilityBin,
};

use super::labels::{resolve, Truth};
use super::{LabelRow, Seen};

/// The reliability table's bins.
pub const BINS: usize = 10;

/// The Choice classes whose act-band answer a pack's live action reads
/// (§2.8): `loop.v1` nudges a task on `work_state: progressing`. role.v1's
/// classes are the roles table's, known only from each state, so its
/// minimum counts its deciding question alone.
pub const ACTING: &[(&str, &str, &[&str])] = &[("loop", "work_state", &["progressing"])];

fn acting(pack_id: &str) -> Option<(&'static str, &'static [&'static str])> {
    ACTING
        .iter()
        .find(|(p, _, _)| *p == pack_id)
        .map(|(_, q, c)| (*q, *c))
}

/// A judgment's whole answer to a question (not a per-item Noul's).
fn answer<'a>(s: &'a Seen, question: &str) -> Option<&'a AnswerRecord> {
    s.judgment
        .answers
        .iter()
        .find(|a| a.about.is_none() && a.def == question)
}

fn kind_name(k: Kind) -> &'static str {
    match k {
        Kind::Choice => "choice",
        Kind::Noul => "noul",
        Kind::Score => "score",
    }
}

pub(super) fn band_name(b: Band) -> &'static str {
    match b {
        Band::Act => "act",
        Band::Confirm => "confirm",
        Band::Escalate => "escalate",
    }
}

/// The confidence a Choice's or Score's top answer was given.
fn confidence(a: &AnswerRecord) -> f64 {
    use theseus_judge::client::Answer;
    match &a.answer {
        Answer::Choice { confidence, .. } | Answer::Score { confidence, .. } => *confidence,
        Answer::Noul { noul } => *noul,
    }
}

/// The pairs a question's labeled answers make, as `learn` takes them.
#[derive(Debug, Default, Clone, PartialEq)]
pub struct Pairs {
    /// A Noul's `(p, truth)`; a Choice's or Score's `(confidence, top was
    /// right)`.
    pub calibration: Vec<(f64, bool)>,
    /// A Choice's `(top, label)`. A label that names only a wrong class
    /// counts where it settles the answer (its top is that class), as
    /// `not:<class>`, which no class's recall reads.
    pub classes: Vec<(String, String)>,
}

/// One question over a set of judgments: its numbers, and the pairs they
/// came from.
pub fn question(
    def_kind: Option<Kind>,
    id: &str,
    decides: bool,
    options: &[String],
    seen: &[&Seen],
    labels: &std::collections::HashMap<String, Vec<LabelRow>>,
) -> (QuestionReport, Pairs) {
    let mut pairs = Pairs::default();
    let mut bands: BTreeMap<&'static str, u32> = BTreeMap::new();
    let (mut answered, mut labeled) = (0u32, 0u32);
    let mut kind = def_kind;
    for s in seen {
        let Some(a) = answer(s, id) else { continue };
        answered += 1;
        kind = kind.or(Some(a.answer.kind()));
        *bands.entry(band_name(a.band.band)).or_default() += 1;
        let ls = labels.get(&s.judgment.id).map_or(&[][..], Vec::as_slice);
        let Some((_, t)) = resolve(ls, id, a) else {
            continue;
        };
        if let Some((p, right)) = graded(a, &t) {
            labeled += 1;
            pairs.calibration.push((p, right));
            if let (Top::Choice(top), Some(l)) = (&a.band.top, class_label(&a.band.top, &t)) {
                pairs.classes.push((top.clone(), l));
            }
        }
    }
    let mut classes: BTreeSet<String> = options.iter().cloned().collect();
    for (p, l) in &pairs.classes {
        classes.insert(p.clone());
        if !l.starts_with("not:") {
            classes.insert(l.clone());
        }
    }
    let class_rows = if kind == Some(Kind::Choice) {
        classes
            .into_iter()
            .map(|c| {
                let (precision, recall) = learn::precision_recall(&pairs.classes, &c);
                ClassReport {
                    predicted: pairs.classes.iter().filter(|(p, _)| *p == c).count() as u32,
                    actual: pairs.classes.iter().filter(|(_, l)| *l == c).count() as u32,
                    class: c,
                    precision,
                    recall,
                }
            })
            .collect()
    } else {
        Vec::new()
    };
    let total = answered.max(1) as f64;
    let report = QuestionReport {
        question: id.to_string(),
        kind: kind.map_or("unknown", kind_name).to_string(),
        decides,
        answered,
        labeled,
        bands: ["act", "confirm", "escalate"]
            .iter()
            .map(|b| {
                let n = bands.get(b).copied().unwrap_or(0);
                BandShare {
                    band: (*b).into(),
                    n,
                    share: if answered == 0 {
                        0.0
                    } else {
                        f64::from(n) / total
                    },
                }
            })
            .collect(),
        classes: class_rows,
        calibration: (!pairs.calibration.is_empty())
            .then(|| calibration(&learn::calibration(&pairs.calibration, BINS))),
    };
    (report, pairs)
}

/// What a label grades an answer: the probability it gave, and whether its
/// lean was right. None where the label settles nothing of it (a class
/// known wrong that its top is not).
pub(super) fn graded(a: &AnswerRecord, t: &Truth) -> Option<(f64, bool)> {
    match (&a.band.top, t, &a.answer) {
        (_, Truth::Bool(b), theseus_judge::client::Answer::Noul { noul }) => Some((*noul, *b)),
        (Top::Choice(top), Truth::Class(c), _) => Some((confidence(a), top == c)),
        (Top::Choice(top), Truth::NotClass(c), _) => (top == c).then(|| (confidence(a), false)),
        (Top::Level(top), Truth::Level(n), _) => Some((confidence(a), top == n)),
        (Top::Level(top), Truth::NotLevel(n), _) => (top == n).then(|| (confidence(a), false)),
        _ => None,
    }
}

fn class_label(top: &Top, t: &Truth) -> Option<String> {
    match (top, t) {
        (Top::Choice(_), Truth::Class(c)) => Some(c.clone()),
        (Top::Choice(_), Truth::NotClass(c)) => Some(format!("not:{c}")),
        _ => None,
    }
}

pub(super) fn calibration(c: &learn::Calibration) -> Calibration {
    Calibration {
        n: c.n as u32,
        brier: c.brier,
        ece: c.ece,
        bins: c
            .bins
            .iter()
            .map(|b| ReliabilityBin {
                lo: b.lo,
                hi: b.hi,
                n: b.n as u32,
                mean_p: b.mean_p,
                frequency: b.frequency,
            })
            .collect(),
    }
}

/// The questions to report: the pack's, in its order, else what the
/// judgments answered.
fn questions_of(
    pack: Option<&Pack>,
    seen: &[Seen],
) -> Vec<(String, Option<Kind>, bool, Vec<String>)> {
    match pack {
        Some(p) => p
            .questions
            .iter()
            .filter(|q| q.per.is_none())
            .map(|q| {
                let options = q.options.iter().map(|o| o.id.clone()).collect();
                (q.id.clone(), Some(q.kind), q.decides, options)
            })
            .collect(),
        None => {
            let ids: BTreeSet<String> = seen
                .iter()
                .flat_map(|s| s.judgment.answers.iter())
                .filter(|a| a.about.is_none())
                .map(|a| a.def.clone())
                .collect();
            ids.into_iter().map(|q| (q, None, false, vec![])).collect()
        }
    }
}

/// One pack version's report: every judgment of it up to the run, and the
/// holdout of `window`, frozen.
pub fn pack_report(
    name: &str,
    seen: &[Seen],
    labels: &std::collections::HashMap<String, Vec<LabelRow>>,
    window: Window,
) -> PackReport {
    let pack = theseus_judge::pack::by_name(name);
    let pack_id = name.split('.').next().unwrap_or(name);
    let answered: Vec<&Seen> = seen
        .iter()
        .filter(|s| s.judgment.outcome == Outcome::Answered)
        .collect();
    let qs = questions_of(pack.as_deref(), seen);
    let mut questions: Vec<QuestionReport> = qs
        .iter()
        .map(|(id, k, d, o)| question(*k, id, *d, o, &answered, labels).0)
        .collect();
    // Per-item answers (32d), by definition, each graded as a Noul.
    questions.extend(super::items::questions(pack.as_deref(), &answered, labels));
    let labeled = answered
        .iter()
        .filter(|s| {
            qs.iter().any(|(q, ..)| {
                answer(s, q).is_some_and(|a| {
                    let ls = labels.get(&s.judgment.id).map_or(&[][..], Vec::as_slice);
                    resolve(ls, q, a).is_some_and(|(_, t)| graded(a, &t).is_some())
                })
            }) || !super::items::labels_of(pack.as_deref(), s, labels).is_empty()
        })
        .count() as u32;
    let disagreements = answered
        .iter()
        .filter(|s| crate::fact::judge::disagrees(&s.judgment))
        .count() as u32;
    let mut by_class: BTreeMap<String, Vec<u64>> = BTreeMap::new();
    for s in seen {
        let called = !matches!(s.judgment.outcome, Outcome::Skipped { .. });
        if called {
            let class = s
                .context("class")
                .unwrap_or(match pack.as_ref().map(|p| p.point) {
                    Some(theseus_judge::pack::Point::Inbound) => "inbound",
                    _ => "other",
                })
                .to_string();
            by_class
                .entry(class)
                .or_default()
                .push(s.judgment.timing.total_ms);
        }
    }
    let latency = by_class
        .into_iter()
        .map(|(class, ms)| LatencyRow {
            class,
            n: ms.len() as u32,
            p50_ms: learn::percentile(&ms, 50.0),
            p95_ms: learn::percentile(&ms, 95.0),
            p99_ms: learn::percentile(&ms, 99.0),
        })
        .collect();
    let count = |f: &dyn Fn(&Outcome) -> bool| {
        seen.iter().filter(|s| f(&s.judgment.outcome)).count() as u32
    };
    PackReport {
        pack: name.to_string(),
        calls: seen.len() as u32,
        answered: answered.len() as u32,
        failed: count(&|o| matches!(o, Outcome::Failed { .. })),
        skipped: count(&|o| matches!(o, Outcome::Skipped { .. })),
        labeled,
        baseline: pack
            .as_ref()
            .and_then(|p| serde_json::to_value(p.baseline).ok())
            .and_then(|v| v.as_str().map(str::to_string))
            .unwrap_or_default(),
        agreement: (!answered.is_empty())
            .then(|| f64::from(answered.len() as u32 - disagreements) / answered.len() as f64),
        disagreements,
        cost_usd: theseus_judge::price::micros_to_usd(
            seen.iter().filter_map(|s| s.judgment.cost_micros).sum(),
        ),
        latency,
        questions,
        holdout: super::items::with_items(
            holdout(pack_id, &qs, &answered, labels, window),
            pack.as_deref(),
            &answered,
            labels,
        ),
    }
}

/// The holdout: the answered judgments inside the window (`learn`'s
/// split), their labels as of now, its numbers, and whether they reach
/// the minimum.
fn holdout(
    pack_id: &str,
    qs: &[(String, Option<Kind>, bool, Vec<String>)],
    answered: &[&Seen],
    labels: &std::collections::HashMap<String, Vec<LabelRow>>,
    window: Window,
) -> Holdout {
    let items: Vec<Labeled<usize>> = answered
        .iter()
        .enumerate()
        .map(|(i, s)| Labeled {
            at_ms: s.at_ms,
            item: i,
        })
        .collect();
    let split = learn::holdout_split(&items, window);
    let inside: Vec<&Seen> = split.holdout.iter().map(|l| answered[l.item]).collect();
    let mut label_ids: BTreeSet<String> = BTreeSet::new();
    let mut per_question: BTreeMap<String, usize> = BTreeMap::new();
    let mut per_class: BTreeMap<String, usize> = BTreeMap::new();
    let act = acting(pack_id);
    for s in &inside {
        let ls = labels.get(&s.judgment.id).map_or(&[][..], Vec::as_slice);
        for (q, ..) in qs {
            let Some(a) = answer(s, q) else { continue };
            let Some((l, t)) = resolve(ls, q, a) else {
                continue;
            };
            if graded(a, &t).is_none() {
                continue;
            }
            label_ids.insert(l.id.clone());
            *per_question.entry(q.clone()).or_default() += 1;
            if let (Some((aq, classes)), Top::Choice(top)) = (act, &a.band.top) {
                if aq == q && classes.contains(&top.as_str()) {
                    *per_class.entry(top.clone()).or_default() += 1;
                }
            }
        }
    }
    let deciding: Vec<&str> = qs
        .iter()
        .filter(|(_, _, d, _)| *d)
        .map(|(q, ..)| q.as_str())
        .collect();
    let classes: &[&str] = act.map_or(&[], |(_, c)| c);
    // Every shortfall, each in `learn`'s words: one question or class at a
    // time.
    let min = Minimum::default();
    let short: Vec<String> = deciding
        .iter()
        .filter_map(|q| learn::sufficient(&per_question, &per_class, &[q], &[], min).err())
        .chain(
            classes
                .iter()
                .filter_map(|c| learn::sufficient(&per_question, &per_class, &[], &[c], min).err()),
        )
        .collect();
    let enough = if short.is_empty() {
        Ok(())
    } else {
        Err(short.join("; "))
    };
    let questions = qs
        .iter()
        .map(|(id, k, d, o)| question(*k, id, *d, o, &inside, labels).0)
        .collect();
    let u32s = |m: BTreeMap<String, usize>| -> BTreeMap<String, u32> {
        m.into_iter().map(|(k, v)| (k, v as u32)).collect()
    };
    Holdout {
        start_ms: window.start_ms,
        end_ms: window.end_ms,
        judgments: inside.iter().map(|s| s.judgment.id.clone()).collect(),
        labels: label_ids.into_iter().collect(),
        train: split.train.len() as u32,
        later: split.later as u32,
        labeled_per_question: u32s(per_question),
        labeled_per_acting_class: u32s(per_class),
        sufficient: enough.is_ok(),
        insufficient: enough.err().map(|why| format!("insufficient: {why}")),
        questions,
    }
}
