//! The `+rerank` arm's reorder (design M6 §2.7, step 32c): Jev says, of each
//! of the top 20 candidates, how likely it holds information that would
//! help answer the message, and the top 20 are re-sorted by it. Pure: the
//! core asks Jev, and this decides the order and what it would admit.
//!
//! - [`eligible`]: the candidates that pass every filter (the place rule
//!   first), in the science's order. Only these may reach Jev.
//! - [`reorder`]: the top [`TOP`] re-sorted by Jev's probability, highest
//!   first; an item Jev did not answer keeps its fused place, and the rest
//!   follow in the fused order.
//! - [`repack`]: the same pipeline as [`crate::recall::recall`] (the filters,
//!   then the pack under the budget), ranked by that order.

use std::collections::BTreeMap;

use crate::recall::{self, Asker, Candidate, Pack, Params};
use crate::science::{
    Demotion, Fresh, GateDecision, Heat, MemoryScience, Neighbour, RankCtx, ScienceId, Scored,
};
use crate::{AccessEvent, Adjacency, Retention};

/// How many of the fused order Jev re-sorts (§2.7).
pub const TOP: usize = 20;

/// The candidates that pass every filter, in `science`'s order: each is
/// filtered alone, so a second chunk of a node is kept here (the pack drops
/// it, as `in_context`, once the node is admitted).
pub fn eligible(
    science: &dyn MemoryScience,
    asker: &Asker<'_>,
    candidates: &[Candidate],
    p: &Params,
) -> Vec<Candidate> {
    let open = Params {
        budget_tokens: u64::MAX,
        max_items: usize::MAX,
        ..p.clone()
    };
    let kept: Vec<Candidate> = candidates
        .iter()
        .filter(|c| {
            !recall::recall(science, asker, vec![(*c).clone()], &open)
                .admitted
                .is_empty()
        })
        .cloned()
        .collect();
    let order = science.rank(
        kept.iter()
            .map(|c| Scored {
                key: c.key(),
                score: c.fused,
            })
            .collect(),
        &asker.rank_ctx(&kept),
    );
    let mut by_key: BTreeMap<String, Candidate> = kept.into_iter().map(|c| (c.key(), c)).collect();
    order.iter().filter_map(|s| by_key.remove(&s.key)).collect()
}

/// `fused` (keys, best first) with its first [`TOP`] re-sorted by `p`, Jev's
/// probability per key, highest first and ties in the fused order. A key
/// with no probability (unanswered, or not a number) keeps its place; the
/// keys past the top follow in the fused order.
pub fn reorder(fused: &[String], p: &BTreeMap<String, f64>) -> Vec<String> {
    let top = fused.len().min(TOP);
    let prob = |k: &String| p.get(k).copied().filter(|x| x.is_finite());
    let mut answered: Vec<(usize, f64)> = fused[..top]
        .iter()
        .enumerate()
        .filter_map(|(i, k)| prob(k).map(|x| (i, x)))
        .collect();
    let slots: Vec<usize> = answered.iter().map(|(i, _)| *i).collect();
    // Stable: equal probabilities keep the fused order.
    answered.sort_by(|a, b| b.1.total_cmp(&a.1));
    let mut out = fused.to_vec();
    for (slot, (from, _)) in slots.into_iter().zip(answered) {
        out[slot] = fused[from].clone();
    }
    out
}

/// A science whose rank is a given order of keys, over another's answers to
/// everything else: the pipeline's filters and pack, ranked by Jev.
pub struct Reranked<'a> {
    pub inner: &'a dyn MemoryScience,
    /// Every key, best first; a key not in it ranks after them all, in the
    /// inner science's order.
    pub order: Vec<String>,
}

impl MemoryScience for Reranked<'_> {
    fn id(&self) -> ScienceId {
        self.inner.id()
    }

    fn min_score(&self) -> f64 {
        self.inner.min_score()
    }

    fn gate(&self, fresh: &Fresh, near: &[Neighbour]) -> GateDecision {
        self.inner.gate(fresh, near)
    }

    fn gate_thresholds(&self) -> (f32, f32) {
        self.inner.gate_thresholds()
    }

    fn prefers_newer(&self) -> bool {
        self.inner.prefers_newer()
    }

    fn reads_retention(&self) -> bool {
        self.inner.reads_retention()
    }

    fn synthesis(&self, node_id: &str) -> crate::science::SynthesisAdmit {
        self.inner.synthesis(node_id)
    }

    fn schedule(&self, prior: Option<&Retention>, ev: &AccessEvent) -> Option<Retention> {
        self.inner.schedule(prior, ev)
    }

    fn activate(
        &self,
        g: &dyn Adjacency<String>,
        seeds: &[(String, f32)],
        budget: usize,
    ) -> Vec<(String, f32)> {
        self.inner.activate(g, seeds, budget)
    }

    fn decay_sweep(&self, now_ms: u64, view: &[Heat]) -> Vec<Demotion> {
        self.inner.decay_sweep(now_ms, view)
    }

    fn rank(&self, fused: Vec<Scored>, ctx: &RankCtx) -> Vec<Scored> {
        let at: BTreeMap<&str, usize> = self
            .order
            .iter()
            .enumerate()
            .map(|(i, k)| (k.as_str(), i))
            .collect();
        let mut ranked = self.inner.rank(fused, ctx);
        // Stable: the keys outside the order keep the inner order, after.
        ranked.sort_by_key(|s| at.get(s.key.as_str()).copied().unwrap_or(usize::MAX));
        ranked
    }
}

/// What the pipeline would admit ranked by `order` (from [`reorder`]): the
/// same filters, the place rule first, and the same pack.
pub fn repack(
    science: &dyn MemoryScience,
    asker: &Asker<'_>,
    candidates: Vec<Candidate>,
    p: &Params,
    order: Vec<String>,
) -> Pack {
    recall::recall(
        &Reranked {
            inner: science,
            order,
        },
        asker,
        candidates,
        p,
    )
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeSet;

    use super::*;
    use crate::recall::{Place, Reason};
    use crate::science::Baseline;

    fn keys(v: &[&str]) -> Vec<String> {
        v.iter().map(|s| s.to_string()).collect()
    }

    fn probs(v: &[(&str, f64)]) -> BTreeMap<String, f64> {
        v.iter().map(|(k, p)| (k.to_string(), *p)).collect()
    }

    /// The top is re-sorted by probability, highest first; ties keep the
    /// fused order; an unanswered item keeps its place.
    #[test]
    fn the_top_is_resorted_and_an_unanswered_item_keeps_its_place() {
        let fused = keys(&["a", "b", "c", "d", "e"]);
        let p = probs(&[("a", 0.1), ("b", 0.9), ("d", 0.9), ("e", 0.5)]);
        // c is unanswered: it stays third.
        assert_eq!(reorder(&fused, &p), keys(&["b", "d", "c", "e", "a"]));
        assert_eq!(reorder(&fused, &BTreeMap::new()), fused, "no answers");
        let nan = probs(&[("a", f64::NAN), ("b", 0.2), ("c", 0.3)]);
        assert_eq!(
            reorder(&fused, &nan),
            keys(&["a", "c", "b", "d", "e"]),
            "NaN is no answer"
        );
    }

    /// Past the top 20, the fused order stands, whatever Jev said.
    #[test]
    fn the_rest_follows_in_the_fused_order() {
        let fused: Vec<String> = (0..25).map(|i| format!("k{i:02}")).collect();
        let mut p: BTreeMap<String, f64> = fused
            .iter()
            .enumerate()
            .map(|(i, k)| (k.clone(), i as f64 / 100.0))
            .collect();
        p.insert("k24".into(), 1.0);
        let out = reorder(&fused, &p);
        assert_eq!(out[0], "k19", "the top 20 reversed");
        assert_eq!(out[19], "k00");
        assert_eq!(out[20..], fused[20..], "k24 is past the top: unmoved");
        let set: BTreeSet<_> = out.iter().collect();
        assert_eq!(set.len(), 25, "a permutation");
    }

    fn cand(node: &str, place: Place, fused: f64) -> Candidate {
        Candidate {
            node_id: node.into(),
            chunk: 0,
            session_id: format!("ses_{node}"),
            position: 1,
            kind: "user_message".into(),
            origin: "operator".into(),
            external: false,
            text: format!("the text of {node}"),
            fused,
            index_rank: 1,
            place,
        }
    }

    /// Only what passes every filter is eligible, in the fused order; the
    /// repack admits by Jev's order under the same budget and filters.
    #[test]
    fn eligible_is_filtered_and_repack_admits_by_the_new_order() {
        let science = Baseline {
            min_score: 0.01,
            ..Baseline::default()
        };
        let in_context = BTreeSet::from(["seen".to_string()]);
        let here = Place::Private;
        let asker = Asker {
            session_id: "ses_here",
            place: &here,
            in_context: &in_context,
            labeled: &BTreeSet::new(),
            links: &[],
            now_ms: 0,
            retention: &BTreeMap::new(),
        };
        let mut second = cand("n1", Place::Private, 0.85);
        second.chunk = 1;
        // A private place draws on a shared place's sessions (theseus-1is6):
        // external text is the filtered one here.
        let mut ext = cand("ext", Place::Shared("discord:channel:9".into()), 0.95);
        ext.external = true;
        let cands = vec![
            cand("n1", Place::Private, 0.9),
            ext,
            cand("seen", Place::Private, 0.8),
            cand("n2", Place::Private, 0.7),
            cand("faint", Place::Private, 0.001),
            cand("n3", Place::Private, 0.6),
            second,
        ];
        let p = Params {
            max_items: 2,
            ..Params::default()
        };
        let el: Vec<String> = eligible(&science, &asker, &cands, &p)
            .iter()
            .map(Candidate::key)
            .collect();
        assert_eq!(el, keys(&["n1#0", "n1#1", "n2#0", "n3#0"]));
        let base = recall::recall(&science, &asker, cands.clone(), &p);
        let ids =
            |pk: &Pack| -> Vec<String> { pk.admitted.iter().map(|a| a.candidate.key()).collect() };
        assert_eq!(ids(&base), keys(&["n1#0", "n2#0"]));
        let order = reorder(&el, &probs(&[("n1#0", 0.2), ("n2#0", 0.4), ("n3#0", 0.95)]));
        // n1#1 is unanswered: it keeps its second place, and is admitted
        // there, as n1#0 now comes last.
        let re = repack(&science, &asker, cands, &p, order);
        assert_eq!(ids(&re), keys(&["n3#0", "n1#1"]));
        assert_eq!(re.admitted[0].rank, 1);
        // The filters are the same: the external one is dropped for it.
        assert!(re
            .dropped
            .iter()
            .any(|d| d.candidate.node_id == "ext" && d.reason == Reason::Untrusted));
        assert_eq!(re.admitted.len() + re.dropped.len(), 7);
    }

    use proptest::prelude::*;

    fn place_strategy() -> impl Strategy<Value = Place> {
        prop_oneof![
            Just(Place::Private),
            Just(Place::Unknown),
            (0u8..3).prop_map(|i| Place::Shared(format!("discord:channel:{i}"))),
        ]
    }

    proptest! {
        /// The place rule through the rerank, over generated candidates and
        /// any answers: nothing a turn may not draw on is eligible (so it
        /// never reaches Jev), and the repack drops every such candidate
        /// for its place, as the fused pack does.
        #[test]
        fn the_place_rule_holds_through_the_rerank(
            here in place_strategy(),
            places in proptest::collection::vec(place_strategy(), 1..30),
            scores in proptest::collection::vec(0.0f64..1.0, 30),
            answers in proptest::collection::vec(proptest::option::of(0.0f64..1.0), 30),
        ) {
            let science = Baseline { min_score: 0.01, ..Baseline::default() };
            let none = BTreeSet::new();
            let asker = Asker { session_id: "ses_here", place: &here, in_context: &none, labeled: &none, links: &[], now_ms: 0, retention: &BTreeMap::new() };
            let cands: Vec<Candidate> = places
                .iter()
                .enumerate()
                .map(|(i, pl)| cand(&format!("n{i}"), pl.clone(), scores[i]))
                .collect();
            let p = Params::default();
            let el = eligible(&science, &asker, &cands, &p);
            for c in &el {
                prop_assert!(here.may_draw_on(&c.place), "{here:?} would hand Jev {:?}", c.place);
            }
            let keys: Vec<String> = el.iter().map(Candidate::key).collect();
            let p_of: BTreeMap<String, f64> = keys
                .iter()
                .zip(&answers)
                .filter_map(|(k, a)| a.map(|x| (k.clone(), x)))
                .collect();
            let order = reorder(&keys, &p_of);
            let re = repack(&science, &asker, cands, &p, order);
            for a in &re.admitted {
                prop_assert!(here.may_draw_on(&a.candidate.place));
            }
            for d in &re.dropped {
                if !here.may_draw_on(&d.candidate.place) {
                    prop_assert_eq!(d.reason, Reason::Place);
                }
            }
        }
    }
}
