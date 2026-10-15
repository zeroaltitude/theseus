//! The relevance floor and the subject bar as the core sets them for one
//! recall (theseus-55hlu): the pure arithmetic is `theseus_memory::floor`;
//! this reads the index's answer (the sources that answered and their
//! weights, the entities the question named) and the config's two shares.
//!
//! **Composition for a thread penalty** (theseus-f2g0, a later row): the
//! floor is a check on a candidate's own fused score, run in the pack's
//! filters before ranking. A penalty that lowers a candidate's score for
//! distance in a thread should lower `fused` before the filters (so the
//! floor sees it) or be a rank adjustment after them; it must not add a
//! second threshold of its own. The subject bar is the same kind of check
//! on the candidate's entities, and the two compose by dropping first at
//! whichever is reached first (`threshold`, then `subject`).

use std::collections::{BTreeMap, BTreeSet};

use theseus_memory::{floor, Candidate, Floor};
use theseus_protocol::memory::RecallRelevance;

use crate::config::MemoryConfig;

/// The `Floor` for a recall whose index answered with `weights`, and the
/// manifest's note of it; none of either when neither share is on.
pub fn of(
    cfg: &MemoryConfig,
    weights: &BTreeMap<String, f64>,
    ranked: &BTreeMap<String, u64>,
    candidates: &[Candidate],
) -> (Floor, Option<RecallRelevance>) {
    if cfg.recall_floor <= 0.0 && cfg.recall_subject_bar <= 0.0 {
        return (Floor::default(), None);
    }
    // A source that answered with no hit at all (the entity index, for a
    // question that names none) cannot lift any candidate: it sets no part
    // of the best a score can be.
    let live: BTreeMap<String, f64> = weights
        .iter()
        .filter(|(s, _)| ranked.contains_key(*s))
        .map(|(s, w)| (s.clone(), *w))
        .collect();
    let ceiling = floor::ceiling(&live);
    let asked: Vec<String> = candidates
        .iter()
        .flat_map(|c| c.matched.iter().cloned())
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect();
    let f = Floor::new(cfg.recall_floor, cfg.recall_subject_bar, ceiling, asked);
    let note = RecallRelevance {
        ceiling,
        min_fused: f.min_fused,
        subject_fused: f.subject_fused,
        asked: f.asked.clone(),
    };
    (f, Some(note))
}

/// The `Floor` a manifest's note records, for a pack again in Jev's order.
pub fn floor_of(r: &RecallRelevance) -> Floor {
    Floor {
        min_fused: r.min_fused,
        subject_fused: r.subject_fused,
        asked: r.asked.clone(),
    }
}
