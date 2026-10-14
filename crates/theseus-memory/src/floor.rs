//! The relevance floor and the subject bar (theseus-55hlu): what keeps a
//! candidate that is only near the question out of the pack.
//!
//! **The floor is on the fused score, and absolute.** Reciprocal rank fusion
//! scores a hit `Σ w / (60 + rank)` over the sources that ranked it, so the
//! best a hit can do is [`ceiling`], every answering source's top rank. A
//! floor of `s` admits a hit scoring at least `s × ceiling`: how much of the
//! sources' agreement it has, the same share whether two sources answered or
//! three (the words alone, when the vector search was late). It is not
//! relative to the query's best hit, because the best hit would then always
//! pass itself, and a question with nothing to recall would still bring a
//! note. Below the floor a candidate is dropped as `threshold`; when none
//! passes the pack is empty and the turn gets no `Recall` node.
//!
//! **The subject bar.** When the question names entities the index knows
//! (`Params::asked`, the query's entities that some hit holds) and a
//! candidate holds none of them but holds another of the same type (a
//! different host, path or crate), it is about something else of the same
//! kind, and must clear the higher bar: dropped as `subject` below it.
//!
//! Pure: the core fills the numbers from the index's answer.

use std::collections::BTreeMap;

/// The best a fused score can be: each answering source at rank 1, `Σ w /
/// 61`. `weights` is the index's answer's (source to weight); with none, one
/// source of weight 1.
pub fn ceiling(weights: &BTreeMap<String, f64>) -> f64 {
    let sum: f64 = weights.values().filter(|w| **w > 0.0).sum();
    (if sum > 0.0 { sum } else { 1.0 }) / (RRF_K + 1.0)
}

/// The fusion's constant, as the index fuses (`theseus-index`'s `RRF_K`).
pub const RRF_K: f64 = 60.0;

/// The pack's relevance numbers for one question.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Floor {
    /// The least fused score; 0 admits all.
    pub min_fused: f64,
    /// The least fused score for a candidate about another subject of the
    /// same kind; 0 is no bar.
    pub subject_fused: f64,
    /// The question's entities (`type:value`) that some hit holds.
    pub asked: Vec<String>,
}

impl Floor {
    /// The floor and bar as shares (`[memory] recall_floor`,
    /// `recall_subject_bar`) of `ceiling`.
    pub fn new(floor: f64, bar: f64, ceiling: f64, asked: Vec<String>) -> Self {
        Self {
            min_fused: floor * ceiling,
            subject_fused: bar * ceiling,
            asked,
        }
    }

    /// Whether `entities` (a candidate's all) are about another subject of
    /// the same kind as the question's: none is asked, and one has the type
    /// of an asked one.
    pub fn other_subject(&self, entities: &[String]) -> bool {
        if self.subject_fused <= 0.0 || self.asked.is_empty() {
            return false;
        }
        if entities.iter().any(|e| self.asked.contains(e)) {
            return false;
        }
        let kind = |e: &str| e.split_once(':').map_or("", |(t, _)| t).to_string();
        entities
            .iter()
            .any(|e| self.asked.iter().any(|a| kind(a) == kind(e)))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_ceiling_is_every_sources_top_rank() {
        let w: BTreeMap<String, f64> = [("bm25", 1.0), ("vector", 1.0), ("entity", 0.5)]
            .into_iter()
            .map(|(k, v)| (k.to_string(), v))
            .collect();
        assert!((ceiling(&w) - 2.5 / 61.0).abs() < 1e-12);
        assert!((ceiling(&BTreeMap::new()) - 1.0 / 61.0).abs() < 1e-12);
    }

    #[test]
    fn another_subject_is_one_of_the_same_type_the_question_did_not_name() {
        let f = Floor::new(0.3, 0.6, 0.05, vec!["host:db-prod.example".into()]);
        let s = |v: &[&str]| v.iter().map(|e| e.to_string()).collect::<Vec<_>>();
        assert!(f.other_subject(&s(&["host:db-stage.example"])));
        assert!(!f.other_subject(&s(&["host:db-prod.example", "host:db-stage.example"])));
        assert!(!f.other_subject(&s(&["file:notes.md"])), "another kind is not another subject");
        assert!(!f.other_subject(&[]));
        assert!(!Floor::new(0.3, 0.0, 0.05, vec!["host:a".into()]).other_subject(&s(&["host:b"])));
    }
}
