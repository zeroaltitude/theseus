//! The `+activation` arm's science (design M6 §2.7, §2.9; step 32b's
//! wire-in): `baseline` in every verb but `activate`, which spreads over
//! the adjacency projection; and the numbers by which what it reaches
//! enters recall's fusion, as data.
//!
//! - **Seeds**: the turn's new node at 1.0, and the top [`Activated::seeds`]
//!   fused hits at their scores over the best one's.
//! - **A ranked source**: the reached nodes, strongest first, are one more
//!   list in the fusion, as the index's tender fuses its own (weighted
//!   reciprocal rank, `k = 60`): a hit the spread reached gains
//!   `weight / (60 + rank)`, and at most [`Activated::adds`] of the
//!   strongest the index did not return join the candidates with that term
//!   alone, before every filter.

use crate::activation::{spread, Adjacency, SpreadParams};
use crate::fsrs::Retention;
use crate::science::{
    fnv1a, Baseline, Demotion, Fresh, GateDecision, Heat, MemoryScience, Neighbour, RankCtx,
    ScienceId, Scored,
};
use crate::AccessEvent;

/// Reciprocal rank fusion's constant, the tender's (`theseus-index`'s
/// `fuse::RRF_K`).
pub const RRF_K: f64 = 60.0;

/// `baseline`, with spreading activation as a ranked source.
#[derive(Clone, Debug, PartialEq)]
pub struct Activated {
    pub base: Baseline,
    /// The spread's numbers (§2.7: two hops, 0.7 a hop, 0.1, 200 nodes).
    pub spread: SpreadParams,
    /// The source's weight in the fusion, beside the index's (each 1 by
    /// default).
    pub weight: f64,
    /// The fused hits that seed the spread.
    pub seeds: usize,
    /// The most reached nodes the index did not return that join the
    /// candidates.
    pub adds: usize,
}

impl Default for Activated {
    fn default() -> Self {
        Self {
            base: Baseline::default(),
            spread: SpreadParams::default(),
            weight: 1.0,
            seeds: 10,
            adds: 20,
        }
    }
}

impl Activated {
    /// A reached node's term in the fusion, at its rank from 1.
    pub fn term(&self, rank: usize) -> f64 {
        if self.weight > 0.0 && self.weight.is_finite() {
            self.weight / (RRF_K + rank as f64)
        } else {
            0.0
        }
    }

    /// Its parameters as one line: what the digest is of.
    fn canonical(&self) -> String {
        let s = &self.spread;
        let w = &s.weights;
        format!(
            "{};hops={};decay={};threshold={};budget={};tool_result={};neighbour={};\
             derived_from={};same_entity={};to_newer={};to_older={};contradicts={};\
             weight={};seeds={};adds={}",
            self.base.canonical(),
            s.hops,
            s.decay,
            s.threshold,
            s.budget,
            w.tool_result,
            w.neighbour,
            w.derived_from,
            w.same_entity,
            w.to_newer,
            w.to_older,
            w.contradicts,
            self.weight,
            self.seeds,
            self.adds,
        )
    }
}

impl MemoryScience for Activated {
    fn id(&self) -> ScienceId {
        ScienceId {
            name: "activation",
            params: format!("{:016x}", fnv1a(self.canonical().as_bytes())),
        }
    }

    fn min_score(&self) -> f64 {
        self.base.min_score()
    }

    fn gate(&self, fresh: &Fresh, near: &[Neighbour]) -> GateDecision {
        self.base.gate(fresh, near)
    }

    fn gate_thresholds(&self) -> (f32, f32) {
        self.base.gate_thresholds()
    }

    fn schedule(&self, prior: Option<&Retention>, ev: &AccessEvent) -> Option<Retention> {
        self.base.schedule(prior, ev)
    }

    /// The spread, at most `budget` nodes (and never more than its own).
    fn activate(
        &self,
        g: &dyn Adjacency<String>,
        seeds: &[(String, f32)],
        budget: usize,
    ) -> Vec<(String, f32)> {
        let p = SpreadParams {
            budget: budget.min(self.spread.budget),
            ..self.spread
        };
        spread(g, seeds, &p)
    }

    fn decay_sweep(&self, now_ms: u64, view: &[Heat]) -> Vec<Demotion> {
        self.base.decay_sweep(now_ms, view)
    }

    fn rank(&self, fused: Vec<Scored>, ctx: &RankCtx) -> Vec<Scored> {
        self.base.rank(fused, ctx)
    }

    fn prefers_newer(&self) -> bool {
        self.base.prefers_newer()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::activation::{AdjacencyList, EdgeKind};

    #[test]
    fn it_is_baseline_but_for_the_spread() {
        let a = Activated::default();
        let b = Baseline::default();
        assert_eq!(a.min_score(), b.min_score());
        assert_eq!(a.gate_thresholds(), b.gate_thresholds());
        assert_eq!(a.prefers_newer(), b.prefers_newer());
        let mut g = AdjacencyList::default();
        g.link("a".to_string(), "b".to_string(), EdgeKind::SameEntity);
        let got = a.activate(&g, &[("a".into(), 1.0)], 200);
        assert_eq!(got.len(), 1);
        assert!((got[0].1 - 0.7).abs() < 1e-6);
        assert!(b.activate(&g, &[("a".into(), 1.0)], 200).is_empty());
        // The budget given caps it.
        assert!(a.activate(&g, &[("a".into(), 1.0)], 0).is_empty());
    }

    #[test]
    fn its_term_is_the_tenders_and_its_id_names_its_numbers() {
        let a = Activated::default();
        assert!((a.term(1) - 1.0 / 61.0).abs() < 1e-12);
        let quiet = Activated {
            weight: 0.0,
            ..Activated::default()
        };
        assert_eq!(quiet.term(1), 0.0);
        assert_eq!(a.id().name, "activation");
        assert_eq!(a.id(), Activated::default().id());
        assert_ne!(a.id().params, quiet.id().params);
        assert_ne!(a.id().params, Baseline::default().id().params);
    }
}
