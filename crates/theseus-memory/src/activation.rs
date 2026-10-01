//! Spreading activation (design M6 §2.7, step 32b): a weighted spread over
//! typed edges, from seeds, that enters fusion as one more ranked source.
//!
//! The spread, as built, in pulses:
//! - The seeds are the new node at 1.0 and the top fused hits at their
//!   normalized scores, each in [0, 1]. They are the first pulse.
//! - Each hop, every node that received at least the threshold in the last
//!   pulse fires: it passes what it received, times each edge's weight and the
//!   decay, along each of its edges. What a node receives in a pulse is the
//!   sum over its edges. At most `budget` nodes fire in a hop, the strongest
//!   first. Nothing is divided by a node's fan-out: a shared entity's weight
//!   already falls with how many nodes it ties.
//! - A node holds what it received over every pulse: the sum, over every walk
//!   of at most `hops` edges from a seed, of the seed's score times each
//!   edge's weight and the decay. So paths that meet add up, and raising a
//!   seed's score never lowers what another node holds (within the budget).
//! - The result is every node, other than a seed, that holds the threshold,
//!   strongest first, and at most `budget` of them. The seeds are fusion's own
//!   top hits; the arm adds their neighbours (§2.4), and never counts a seed
//!   twice.
//! - Two hops never bring activation back to a node outside the seeds. More
//!   hops can: a walk may return the way it came.
//!
//! Exposure never spreads activation: a recall's `derived_from` edge weighs
//! zero by construction, not by data (theseus-3nk).

use std::collections::BTreeMap;

/// An edge's kind, as the adjacency projection reads it (§2.7's table).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum EdgeKind {
    /// A tool call and its result, by `tool_use_id`.
    ToolResult,
    /// Neighbouring turns in a session, by position.
    Neighbour,
    /// A task's brief or report, by its `derived_from` EDGE.
    DerivedFrom,
    /// The memory pass's `same_entity` EDGE.
    SameEntity,
    /// A `supersedes` EDGE, walked toward the newer node.
    ToNewer,
    /// A `supersedes` EDGE, walked back toward the older node.
    ToOlder,
    /// A `contradicts` EDGE, once written.
    Contradicts,
    /// An entity both nodes mention, mentioned by `df` nodes in all.
    SharedEntity { df: u32 },
    /// A recall's `derived_from` EDGE: exposure, so it never spreads.
    Recall,
}

/// The weight of each edge kind: versioned data, the arm's to tune. A shared
/// entity weighs `1 / ln(1 + df)`, and a recall always weighs 0.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct EdgeWeights {
    pub tool_result: f32,
    pub neighbour: f32,
    pub derived_from: f32,
    pub same_entity: f32,
    pub to_newer: f32,
    pub to_older: f32,
    pub contradicts: f32,
}

impl Default for EdgeWeights {
    /// §2.7's table.
    fn default() -> Self {
        Self {
            tool_result: 0.8,
            neighbour: 0.3,
            derived_from: 0.6,
            same_entity: 1.0,
            to_newer: 1.0,
            to_older: 0.2,
            contradicts: 0.5,
        }
    }
}

impl EdgeWeights {
    /// An edge's weight. A weight that is negative or undefined spreads
    /// nothing.
    pub fn weight(&self, kind: EdgeKind) -> f32 {
        let w = match kind {
            EdgeKind::ToolResult => self.tool_result,
            EdgeKind::Neighbour => self.neighbour,
            EdgeKind::DerivedFrom => self.derived_from,
            EdgeKind::SameEntity => self.same_entity,
            EdgeKind::ToNewer => self.to_newer,
            EdgeKind::ToOlder => self.to_older,
            EdgeKind::Contradicts => self.contradicts,
            EdgeKind::SharedEntity { df } => shared_entity_weight(df),
            EdgeKind::Recall => 0.0,
        };
        if w > 0.0 && w.is_finite() {
            w
        } else {
            0.0
        }
    }
}

/// `1 / ln(1 + df)`: the fewer nodes an entity is in, the more it ties them.
/// An entity that two nodes share is in at least two, so a smaller `df` reads
/// as 2.
pub fn shared_entity_weight(df: u32) -> f32 {
    (1.0 / (1.0 + f64::from(df.max(2))).ln()) as f32
}

/// How a spread runs: §2.7's numbers, as data.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SpreadParams {
    /// How many edges activation crosses from a seed.
    pub hops: u32,
    /// What a hop keeps of the activation it carries.
    pub decay: f32,
    /// The least a node must receive in a pulse to fire, and hold to be in
    /// the result.
    pub threshold: f32,
    /// The most nodes the result holds, and that fire in one hop.
    pub budget: usize,
    pub weights: EdgeWeights,
}

impl Default for SpreadParams {
    /// Two hops, a decay of 0.7 a hop (Vestige's number, as a starting point),
    /// a threshold of 0.1, and at most 200 nodes.
    fn default() -> Self {
        Self {
            hops: 2,
            decay: 0.7,
            threshold: 0.1,
            budget: 200,
            weights: EdgeWeights::default(),
        }
    }
}

/// The graph a spread walks: the adjacency projection, which the wire-in
/// builds after serving.
pub trait Adjacency<K> {
    /// Appends every edge out of `node` to `out`, as (neighbour, kind), in any
    /// order: the spread's result does not depend on it.
    fn edges(&self, node: &K, out: &mut Vec<(K, EdgeKind)>);
}

/// An adjacency held in memory.
#[derive(Clone, Debug)]
pub struct AdjacencyList<K> {
    out: BTreeMap<K, Vec<(K, EdgeKind)>>,
}

impl<K> Default for AdjacencyList<K> {
    fn default() -> Self {
        Self {
            out: BTreeMap::new(),
        }
    }
}

impl<K: Ord + Clone> AdjacencyList<K> {
    /// An edge from `from` to `to`, one way.
    pub fn add(&mut self, from: K, to: K, kind: EdgeKind) {
        self.out.entry(from).or_default().push((to, kind));
    }

    /// An edge both ways, of one kind: any kind but a `supersedes`.
    pub fn link(&mut self, a: K, b: K, kind: EdgeKind) {
        self.add(a.clone(), b.clone(), kind);
        self.add(b, a, kind);
    }

    /// `newer` supersedes `older`: an edge toward the newer node, and one back.
    pub fn supersede(&mut self, newer: K, older: K) {
        self.add(older.clone(), newer.clone(), EdgeKind::ToNewer);
        self.add(newer, older, EdgeKind::ToOlder);
    }
}

impl<K: Ord + Clone> Adjacency<K> for AdjacencyList<K> {
    fn edges(&self, node: &K, out: &mut Vec<(K, EdgeKind)>) {
        if let Some(edges) = self.out.get(node) {
            out.extend_from_slice(edges);
        }
    }
}

/// Spreads activation from `seeds` over `g`: the nodes it reaches, other
/// than the seeds, with what each holds, strongest first and ties by key.
pub fn spread<K, G>(g: &G, seeds: &[(K, f32)], p: &SpreadParams) -> Vec<(K, f32)>
where
    K: Ord + Clone,
    G: Adjacency<K> + ?Sized,
{
    // Each seed once, at its best score, held in [0, 1].
    let mut seed_scores: BTreeMap<K, f32> = BTreeMap::new();
    for (key, score) in seeds {
        let score = if score.is_nan() {
            0.0
        } else {
            score.clamp(0.0, 1.0)
        };
        let best = seed_scores.entry(key.clone()).or_insert(score);
        *best = best.max(score);
    }
    // What each node received in the last pulse; the seeds are the first.
    let mut pulse = seed_scores.clone();
    // What each node received over every pulse.
    let mut held: BTreeMap<K, f32> = BTreeMap::new();
    let (mut edges, mut passed) = (Vec::new(), Vec::new());
    for _ in 0..p.hops {
        let ready = pulse.into_iter().filter(|(_, a)| *a >= p.threshold);
        let firing = strongest(ready, p.budget);
        if firing.is_empty() {
            break;
        }
        for (node, activation) in &firing {
            edges.clear();
            g.edges(node, &mut edges);
            for (to, kind) in edges.drain(..) {
                let a = activation * p.weights.weight(kind) * p.decay;
                if a > 0.0 && to != *node {
                    passed.push((to, a));
                }
            }
        }
        // A float sum depends on its order, so every pulse sums in one order,
        // whatever order the adjacency and the seeds came in.
        passed.sort_by(|(k1, a1), (k2, a2)| k1.cmp(k2).then(a1.total_cmp(a2)));
        pulse = BTreeMap::new();
        for (to, a) in passed.drain(..) {
            *pulse.entry(to).or_insert(0.0) += a;
        }
        for (node, a) in &pulse {
            *held.entry(node.clone()).or_insert(0.0) += a;
        }
    }
    let reached = held
        .into_iter()
        .filter(|(k, a)| *a >= p.threshold && !seed_scores.contains_key(k));
    strongest(reached, p.budget)
}

/// The `n` strongest nodes, ties by key.
fn strongest<K: Ord>(nodes: impl Iterator<Item = (K, f32)>, n: usize) -> Vec<(K, f32)> {
    let mut v: Vec<(K, f32)> = nodes.collect();
    v.sort_by(|(k1, a1), (k2, a2)| a2.total_cmp(a1).then_with(|| k1.cmp(k2)));
    v.truncate(n);
    v
}

#[cfg(test)]
mod tests {
    use super::*;
    use proptest::collection::vec;
    use proptest::prelude::*;

    fn close(got: f32, want: f32) -> bool {
        (got - want).abs() < 1e-6
    }

    fn keys(v: &[(u32, f32)]) -> Vec<u32> {
        v.iter().map(|(k, _)| *k).collect()
    }

    fn run(g: &AdjacencyList<u32>, seeds: &[(u32, f32)]) -> Vec<(u32, f32)> {
        spread(g, seeds, &SpreadParams::default())
    }

    #[test]
    fn the_defaults_are_the_designs() {
        let p = SpreadParams::default();
        assert_eq!((p.hops, p.decay, p.threshold, p.budget), (2, 0.7, 0.1, 200));
        let w = p.weights;
        let table = [
            (EdgeKind::ToolResult, 0.8),
            (EdgeKind::Neighbour, 0.3),
            (EdgeKind::DerivedFrom, 0.6),
            (EdgeKind::SameEntity, 1.0),
            (EdgeKind::ToNewer, 1.0),
            (EdgeKind::ToOlder, 0.2),
            (EdgeKind::Contradicts, 0.5),
            (EdgeKind::Recall, 0.0),
        ];
        for (kind, want) in table {
            assert!(close(w.weight(kind), want), "{kind:?}");
        }
    }

    /// 1 — 2 — 3 — 4, each a `same_entity`: 0.7 a hop, and two hops only.
    #[test]
    fn activation_decays_each_hop_and_stops_after_two() {
        let mut g = AdjacencyList::default();
        g.link(1, 2, EdgeKind::SameEntity);
        g.link(2, 3, EdgeKind::SameEntity);
        g.link(3, 4, EdgeKind::SameEntity);
        let got = run(&g, &[(1, 1.0)]);
        assert_eq!(keys(&got), [2, 3]);
        assert!(close(got[0].1, 0.7) && close(got[1].1, 0.49));
        // A seed's score scales all it spreads.
        let half = run(&g, &[(1, 0.5)]);
        assert!(close(half[0].1, 0.35) && close(half[1].1, 0.245));
        // A third hop, when the data asks for one, on one-way edges.
        let mut g = AdjacencyList::default();
        g.add(1, 2, EdgeKind::SameEntity);
        g.add(2, 3, EdgeKind::SameEntity);
        g.add(3, 4, EdgeKind::SameEntity);
        let p = SpreadParams {
            hops: 3,
            ..SpreadParams::default()
        };
        let got = spread(&g, &[(1, 1.0)], &p);
        assert_eq!(keys(&got), [2, 3, 4]);
        assert!(close(got[0].1, 0.7) && close(got[1].1, 0.49) && close(got[2].1, 0.343));
    }

    /// Two hops bring nothing back to a node outside the seeds. In a
    /// triangle, 2 and 3 each hold their direct 0.7, plus 0.49 by way of the
    /// other, and nothing of their own.
    #[test]
    fn two_hops_bring_nothing_back() {
        let mut g = AdjacencyList::default();
        g.link(1, 2, EdgeKind::SameEntity);
        g.link(2, 3, EdgeKind::SameEntity);
        g.link(3, 1, EdgeKind::SameEntity);
        let got = run(&g, &[(1, 1.0)]);
        assert_eq!(keys(&got), [2, 3]);
        assert!(got.iter().all(|(_, a)| close(*a, 0.7 + 0.49)));
    }

    /// Raising a seed's score never lowers what a node holds. Here 2 is a
    /// weak seed between the new node 1 and 3; it relays what 1 sends it, at
    /// any score of its own.
    #[test]
    fn a_stronger_seed_never_lowers_a_node() {
        let mut g = AdjacencyList::default();
        g.link(1, 2, EdgeKind::SameEntity);
        g.link(2, 3, EdgeKind::SameEntity);
        let mut last = 0.0;
        for score in [0.0, 0.05, 0.1, 0.5, 1.0] {
            let got = run(&g, &[(1, 1.0), (2, score)]);
            assert_eq!(keys(&got), [3]);
            assert!(
                got[0].1 >= last,
                "3 fell to {} at a seed score of {score}",
                got[0].1
            );
            last = got[0].1;
        }
        // 0.49 relayed from 1, plus 0.7 of 2's own.
        assert!(close(last, 0.49 + 0.7));
    }

    #[test]
    fn the_threshold_stops_a_weak_spread() {
        // Neighbouring turns weigh 0.3: 0.21 after a hop, 0.0441 after two.
        let mut g = AdjacencyList::default();
        g.link(1, 2, EdgeKind::Neighbour);
        g.link(2, 3, EdgeKind::Neighbour);
        let got = run(&g, &[(1, 1.0)]);
        assert_eq!(keys(&got), [2]);
        assert!(close(got[0].1, 0.21));
        // A seed below the threshold does not fire.
        let mut g = AdjacencyList::default();
        g.link(1, 2, EdgeKind::SameEntity);
        assert!(run(&g, &[(1, 0.09)]).is_empty());
        // Nor does a node below it, even where firing would carry over it:
        // 2 holds 0.084, and a `same_entity` that weighs 2 would give 3 0.1176.
        let mut g = AdjacencyList::default();
        g.link(1, 2, EdgeKind::Neighbour);
        g.link(2, 3, EdgeKind::SameEntity);
        let heavy = EdgeWeights {
            same_entity: 2.0,
            ..EdgeWeights::default()
        };
        let p = SpreadParams {
            weights: heavy,
            ..SpreadParams::default()
        };
        assert!(spread(&g, &[(1, 0.4)], &p).is_empty());
    }

    /// Ten neighbours by shared entities of rising `df`, so falling weight.
    #[test]
    fn the_budget_keeps_the_strongest() {
        let mut g = AdjacencyList::default();
        for (n, df) in (10..20).zip(2..) {
            g.link(1, n, EdgeKind::SharedEntity { df });
        }
        let p = SpreadParams {
            budget: 3,
            ..SpreadParams::default()
        };
        let got = spread(&g, &[(1, 1.0)], &p);
        assert_eq!(keys(&got), [10, 11, 12]);
        assert!(close(got[0].1, 0.7 / 3f32.ln()));
        // The budget caps what fires, too: 300 neighbours, each with a leaf of
        // its own. The seed fires, then 200 of the 300.
        let mut g = AdjacencyList::default();
        for n in 0..300 {
            g.link(1, 1000 + n, EdgeKind::SameEntity);
            g.link(1000 + n, 5000 + n, EdgeKind::Contradicts);
        }
        let counted = Counting::new(&g);
        let got = spread(&counted, &[(1, 1.0)], &SpreadParams::default());
        assert_eq!(counted.fired.get(), 201);
        assert_eq!(got.len(), 200);
        assert!(got
            .iter()
            .all(|(k, a)| (1000..1200).contains(k) && close(*a, 0.7)));
        // With room for 500, all 300 fire, and the first 200 leaves fit.
        let counted = Counting::new(&g);
        let p = SpreadParams {
            budget: 500,
            ..SpreadParams::default()
        };
        let got = spread(&counted, &[(1, 1.0)], &p);
        assert_eq!(counted.fired.get(), 301);
        let leaves: Vec<u32> = keys(&got).into_iter().filter(|k| *k >= 5000).collect();
        assert_eq!(leaves, (5000..5200).collect::<Vec<u32>>());
    }

    /// An adjacency that counts the nodes that fire: one `edges` call each.
    struct Counting<'a> {
        g: &'a AdjacencyList<u32>,
        fired: std::cell::Cell<usize>,
    }

    impl<'a> Counting<'a> {
        fn new(g: &'a AdjacencyList<u32>) -> Self {
            Self {
                g,
                fired: std::cell::Cell::new(0),
            }
        }
    }

    impl Adjacency<u32> for Counting<'_> {
        fn edges(&self, node: &u32, out: &mut Vec<(u32, EdgeKind)>) {
            self.fired.set(self.fired.get() + 1);
            self.g.edges(node, out);
        }
    }

    #[test]
    fn a_node_fans_out_to_every_neighbour_undivided() {
        let mut g = AdjacencyList::default();
        for n in 2..=51 {
            g.link(1, n, EdgeKind::SameEntity);
        }
        let got = run(&g, &[(1, 1.0)]);
        assert_eq!(got.len(), 50);
        assert!(got.iter().all(|(_, a)| close(*a, 0.7)));
        // Equal activations go by key, so a budget takes the first keys.
        let p = SpreadParams {
            budget: 5,
            ..SpreadParams::default()
        };
        assert_eq!(keys(&spread(&g, &[(1, 1.0)], &p)), [2, 3, 4, 5, 6]);
        // A hub one hop out fans out the same way.
        let got = run(&g, &[(100, 1.0)]);
        assert!(got.is_empty());
        g.link(100, 1, EdgeKind::SameEntity);
        let got = run(&g, &[(100, 1.0)]);
        assert_eq!(got.len(), 51);
        assert!(close(got[0].1, 0.7) && got[1..].iter().all(|(_, a)| close(*a, 0.49)));
    }

    #[test]
    fn recall_edges_carry_nothing() {
        let mut g = AdjacencyList::default();
        g.link(1, 2, EdgeKind::Recall);
        g.link(2, 3, EdgeKind::SameEntity);
        assert!(run(&g, &[(1, 1.0)]).is_empty());
        // No weight in the data can make a recall spread.
        let w = EdgeWeights {
            tool_result: 9.0,
            neighbour: 9.0,
            derived_from: 9.0,
            same_entity: 9.0,
            to_newer: 9.0,
            to_older: 9.0,
            contradicts: 9.0,
        };
        assert_eq!(w.weight(EdgeKind::Recall), 0.0);
        let p = SpreadParams {
            weights: w,
            ..SpreadParams::default()
        };
        assert!(spread(&g, &[(1, 1.0)], &p).is_empty());
    }

    #[test]
    fn a_weight_that_is_not_positive_spreads_nothing() {
        let w = EdgeWeights {
            neighbour: -1.0,
            contradicts: f32::NAN,
            ..EdgeWeights::default()
        };
        assert_eq!(w.weight(EdgeKind::Neighbour), 0.0);
        assert_eq!(w.weight(EdgeKind::Contradicts), 0.0);
    }

    #[test]
    fn supersedes_leans_toward_the_newer_node() {
        let mut g = AdjacencyList::default();
        g.supersede(2, 1);
        let from_older = run(&g, &[(1, 1.0)]);
        let from_newer = run(&g, &[(2, 1.0)]);
        assert_eq!(keys(&from_older), [2]);
        assert!(close(from_older[0].1, 0.7));
        assert_eq!(keys(&from_newer), [1]);
        assert!(close(from_newer[0].1, 0.14));
    }

    #[test]
    fn a_shared_entity_weighs_by_its_rarity() {
        assert!(close(shared_entity_weight(2), 1.0 / 3f32.ln()));
        assert!(shared_entity_weight(10) > shared_entity_weight(100));
        assert_eq!(shared_entity_weight(0), shared_entity_weight(2));
        assert_eq!(shared_entity_weight(1), shared_entity_weight(2));
        // Past e^7 − 1 (1095.6) nodes, one shared entity cannot carry a 1.0
        // seed over the threshold alone.
        assert!(shared_entity_weight(1095) * 0.7 >= 0.1);
        assert!(shared_entity_weight(1096) * 0.7 < 0.1);
    }

    #[test]
    fn paths_that_meet_add_up() {
        let mut g = AdjacencyList::default();
        g.link(1, 3, EdgeKind::DerivedFrom);
        g.link(1, 3, EdgeKind::Neighbour);
        g.link(2, 3, EdgeKind::Neighbour);
        let got = run(&g, &[(1, 1.0), (2, 0.5)]);
        assert_eq!(keys(&got), [3]);
        assert!(close(got[0].1, 0.42 + 0.21 + 0.105));
        // Under the threshold after one hop, over it after two: 4 holds both.
        let mut g = AdjacencyList::default();
        g.link(1, 4, EdgeKind::SharedEntity { df: 5000 });
        g.link(1, 2, EdgeKind::SameEntity);
        g.link(2, 4, EdgeKind::SameEntity);
        let got = run(&g, &[(1, 1.0)]);
        assert_eq!(keys(&got), [2, 4]);
        assert!(close(got[1].1, 0.7 * shared_entity_weight(5000) + 0.49));
    }

    #[test]
    fn a_seed_is_never_in_the_result() {
        let mut g = AdjacencyList::default();
        g.link(1, 2, EdgeKind::SameEntity);
        g.link(2, 3, EdgeKind::SameEntity);
        // 3 holds 0.56 from 2, and 0.49 from 1 by way of 2.
        let got = run(&g, &[(1, 1.0), (2, 0.8)]);
        assert_eq!(keys(&got), [3]);
        assert!(close(got[0].1, 0.56 + 0.49));
        // A seed given twice counts at its best score.
        let got = run(&g, &[(2, 0.2), (2, 0.8), (1, 1.0)]);
        assert!(close(got[0].1, 0.56 + 0.49));
        // Seeds outside [0, 1] are held to it.
        let got = run(&g, &[(2, 7.0)]);
        assert!(close(got[0].1, 0.7));
        assert!(run(&g, &[(2, f32::NAN)]).is_empty());
    }

    fn any_kind() -> impl Strategy<Value = EdgeKind> {
        prop_oneof![
            Just(EdgeKind::ToolResult),
            Just(EdgeKind::Neighbour),
            Just(EdgeKind::DerivedFrom),
            Just(EdgeKind::SameEntity),
            Just(EdgeKind::ToNewer),
            Just(EdgeKind::ToOlder),
            Just(EdgeKind::Contradicts),
            (0u32..3000).prop_map(|df| EdgeKind::SharedEntity { df }),
            Just(EdgeKind::Recall),
        ]
    }

    proptest! {
        #![proptest_config(ProptestConfig {
            cases: 1000,
            failure_persistence: None,
            ..ProptestConfig::default()
        })]

        /// Any graph and seeds: within the budget, at or over the threshold,
        /// no seed, strongest first; and the same, bit for bit, whatever
        /// order the edges and seeds come in.
        #[test]
        fn any_spread_is_bounded_and_deterministic(
            edges in vec((0u32..60, 0u32..60, any_kind()), 0..300),
            seeds in vec((0u32..60, 0.0f32..=1.0), 0..12),
            budget in 0usize..60,
            hops in 0u32..4,
            threshold in 0.0f32..0.5,
            turn in 0usize..300,
        ) {
            let p = SpreadParams { hops, threshold, budget, ..SpreadParams::default() };
            let mut g1 = AdjacencyList::default();
            for &(a, b, kind) in &edges {
                g1.add(a, b, kind);
            }
            let mut other = edges;
            let turn = turn.min(other.len());
            other.rotate_left(turn);
            other.reverse();
            let mut g2 = AdjacencyList::default();
            for &(a, b, kind) in &other {
                g2.add(a, b, kind);
            }
            let mut seeds2 = seeds.clone();
            seeds2.reverse();
            let counted = Counting::new(&g1);
            let got = spread(&counted, &seeds, &p);
            prop_assert_eq!(&got, &spread(&g2, &seeds2, &p));
            prop_assert!(got.len() <= budget);
            prop_assert!(counted.fired.get() <= hops as usize * budget);
            for (k, a) in &got {
                prop_assert!(*a >= threshold && a.is_finite());
                prop_assert!(seeds.iter().all(|(s, _)| s != k), "seed {} in the result", k);
            }
            for pair in got.windows(2) {
                prop_assert!(pair[0].1 > pair[1].1 || (pair[0].1 == pair[1].1 && pair[0].0 < pair[1].0));
            }
        }

        /// Raising one seed's score, or adding an edge, never lowers what a
        /// node holds, while the budget has room for every node (40 keys, a
        /// budget of 200).
        #[test]
        fn a_stronger_seed_never_lowers_any_node(
            edges in vec((0u32..40, 0u32..40, any_kind()), 0..200),
            extra in (0u32..40, 0u32..40, any_kind()),
            seeds in vec((0u32..40, 0.0f32..=1.0), 1..8),
            which in 0usize..8,
            raise in 0.0f32..=1.0,
        ) {
            let mut g = AdjacencyList::default();
            for &(a, b, kind) in &edges {
                g.add(a, b, kind);
            }
            let p = SpreadParams::default();
            let before = spread(&g, &seeds, &p);
            g.add(extra.0, extra.1, extra.2);
            let mut stronger = seeds;
            let i = which % stronger.len();
            stronger[i].1 = (stronger[i].1 + raise).min(1.0);
            let after: BTreeMap<u32, f32> = spread(&g, &stronger, &p).into_iter().collect();
            for (k, a) in before {
                let now = after.get(&k).copied().unwrap_or(0.0);
                // The sums may round apart in their last bits.
                prop_assert!(now >= a * (1.0 - 1e-4), "{} fell from {} to {}", k, a, now);
            }
        }
    }
}
