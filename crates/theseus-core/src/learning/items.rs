//! Per-item answers in the learning ledger (M6 32d): a pack that asks a
//! question per item (`rerank.v1`'s `helps` and `helps_more`, one Noul a
//! note) names each answer as it is asked, `helps.1` to `helps.10`, with
//! `about` the item's key. A label names that question (`theseus judge
//! label <jdg> true --question helps.3`), and keeps the item's key; the
//! report grades each answer as a Noul by the label on its own question,
//! and reports them by definition (`helps`), beside the pack's whole
//! answers.

use std::collections::{BTreeMap, HashMap};

use serde_json::Value;
use theseus_judge::judge::AnswerRecord;
use theseus_judge::learn;
use theseus_judge::Pack;
use theseus_protocol::learning::{BandShare, QuestionReport};

use super::labels::resolve;
use super::report::{band_name, calibration, graded, BINS};
use super::{LabelRow, Seen};

/// A judgment's per-item answers to the definition `def`.
fn answers<'a>(s: &'a Seen, def: &'a str) -> impl Iterator<Item = &'a AnswerRecord> {
    s.judgment
        .answers
        .iter()
        .filter(move |a| a.about.is_some() && a.def == def)
}

/// The definitions `pack` asks per item, with whether each decides.
fn defs(pack: Option<&Pack>) -> Vec<(String, bool)> {
    pack.map(|p| {
        p.questions
            .iter()
            .filter(|q| q.per.is_some())
            .map(|q| (q.id.clone(), q.decides))
            .collect()
    })
    .unwrap_or_default()
}

/// Each per-item definition's report over `seen`: every item's answer, its
/// band, and, where a label settles it, its calibration as a Noul's.
pub fn questions(
    pack: Option<&Pack>,
    seen: &[&Seen],
    labels: &HashMap<String, Vec<LabelRow>>,
) -> Vec<QuestionReport> {
    defs(pack)
        .into_iter()
        .map(|(def, decides)| {
            let mut pairs = Vec::new();
            let mut bands: BTreeMap<&'static str, u32> = BTreeMap::new();
            let mut answered = 0u32;
            for s in seen {
                let ls = labels.get(&s.judgment.id).map_or(&[][..], Vec::as_slice);
                for a in answers(s, &def) {
                    answered += 1;
                    *bands.entry(band_name(a.band.band)).or_default() += 1;
                    if let Some(g) = resolve(ls, &a.question, a).and_then(|(_, t)| graded(a, &t)) {
                        pairs.push(g);
                    }
                }
            }
            QuestionReport {
                question: def,
                kind: "noul".into(),
                decides,
                answered,
                labeled: pairs.len() as u32,
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
                                f64::from(n) / f64::from(answered)
                            },
                        }
                    })
                    .collect(),
                classes: Vec::new(),
                calibration: (!pairs.is_empty())
                    .then(|| calibration(&learn::calibration(&pairs, BINS))),
            }
        })
        .collect()
}

/// The labels that grade `s`'s per-item answers, by their ids: whether
/// any grades one (the report's `labeled`), and which (a holdout's).
pub fn labels_of<'a>(
    pack: Option<&Pack>,
    s: &Seen,
    labels: &'a HashMap<String, Vec<LabelRow>>,
) -> Vec<&'a str> {
    let ls = labels.get(&s.judgment.id).map_or(&[][..], Vec::as_slice);
    defs(pack)
        .iter()
        .flat_map(|(def, _)| answers(s, def))
        .filter_map(|a| {
            let (l, t) = resolve(ls, &a.question, a)?;
            graded(a, &t).map(|_| l.id.as_str())
        })
        .collect()
}

/// Whether `question` is one of a judgment's per-item answers (`helps.3`),
/// and the item's key it is about.
pub fn asked_about(answers: &Value, question: &str) -> Option<String> {
    answers.as_array()?.iter().find_map(|a| {
        (a.get("question")?.as_str()? == question)
            .then(|| a.get("about")?.as_str().map(str::to_string))
            .flatten()
    })
}

/// A per-item question the judgment did not ask, refused: one of the
/// pack's per-item definitions (`helps`) or an item's name under one
/// (`helps.7`) that is not among its answers. Any other question is the
/// pack's own to check.
pub fn refuse_unasked(pack: &Pack, answers: &Value, question: &str) -> Result<(), String> {
    let def = question.split_once('.').map_or(question, |(d, _)| d);
    if !defs(Some(pack)).iter().any(|(d, _)| d == def) {
        return Ok(());
    }
    let asked: Vec<&str> = answers
        .as_array()
        .map(|a| {
            a.iter()
                .filter(|x| x.get("about").is_some_and(|v| v.is_string()))
                .filter_map(|x| x.get("question")?.as_str())
                .collect()
        })
        .unwrap_or_default();
    Err(if asked.is_empty() {
        format!("this judgment did not ask {question}: it answered about no item")
    } else {
        format!(
            "this judgment did not ask {question}: name one it answered, {}",
            asked.join(", ")
        )
    })
}

/// A holdout with its per-item answers: their questions' reports over the
/// judgments inside it, and the labels that grade them among its labels.
pub fn with_items(
    mut h: theseus_protocol::learning::Holdout,
    pack: Option<&Pack>,
    answered: &[&Seen],
    labels: &HashMap<String, Vec<LabelRow>>,
) -> theseus_protocol::learning::Holdout {
    let inside: Vec<&Seen> = answered
        .iter()
        .filter(|s| h.judgments.contains(&s.judgment.id))
        .copied()
        .collect();
    let mut ids: std::collections::BTreeSet<String> = h.labels.iter().cloned().collect();
    for s in &inside {
        ids.extend(labels_of(pack, s, labels).into_iter().map(str::to_string));
    }
    h.labels = ids.into_iter().collect();
    for q in questions(pack, &inside, labels) {
        if q.labeled > 0 {
            h.labeled_per_question.insert(q.question.clone(), q.labeled);
        }
        h.questions.push(q);
    }
    h
}
