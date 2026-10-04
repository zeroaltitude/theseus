//! The `MemoryScience` trait and its baseline (design M6 §2.3, step 30a).
//!
//! The trait keeps §5.1's four verbs and makes every input explicit, so each
//! call is a pure function that replays from the record. Thresholds and
//! weights are data: a science names its parameter set by digest in its
//! [`ScienceId`], so a recall's row says exactly which numbers ranked it.
//!
//! Step 30a reads `id`, `min_score`, and `rank` (the recall step in shadow,
//! and `memory.search`). `gate` is read by the memory pass (row 57, 31a),
//! `schedule` by the retention projection (row 58, 32a), `activate` by the
//! adjacency projection (row 59, 32b), and `decay_sweep` by tiering (row 60,
//! 33); the baseline's answers to them are §2.3's table's.

use crate::activation::Adjacency;
use crate::fsrs::Retention;
use crate::AccessEvent;

/// A science and the parameter set it runs: `baseline@<digest>`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ScienceId {
    /// `baseline`, `native-v1`.
    pub name: &'static str,
    /// The parameter set's digest: 16 hex digits.
    pub params: String,
}

impl std::fmt::Display for ScienceId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}@{}", self.name, self.params)
    }
}

/// A node the gate sees for the first time.
#[derive(Clone, Debug, PartialEq)]
pub struct Fresh {
    pub node_id: String,
    /// An operator's correction of something said before.
    pub correction: bool,
}

/// One of a fresh node's nearest neighbours, by cosine.
#[derive(Clone, Debug, PartialEq)]
pub struct Neighbour {
    pub node_id: String,
    pub cosine: f32,
}

/// What the gate makes of a fresh node.
#[derive(Clone, Debug, PartialEq)]
pub enum GateDecision {
    /// Store it, at one default retention.
    Store,
    /// A near-duplicate: a `same_entity` edge to `0`; the duplicate stays.
    MergeInto(String),
    /// A correction of `0`: a `supersedes` edge.
    Supersedes(String),
}

/// A node's heat, as tiering's projection holds it.
#[derive(Clone, Debug, PartialEq)]
pub struct Heat {
    pub node_id: String,
    /// When it was written, and when it was last read, in ms since the epoch.
    pub written_ms: u64,
    pub last_read_ms: u64,
}

/// A hint that a node may leave the resident tier.
#[derive(Clone, Debug, PartialEq)]
pub struct Demotion {
    pub node_id: String,
    /// Days since it was last read.
    pub idle_days: f64,
}

/// A candidate in ranking: its key, and its fused score.
#[derive(Clone, Debug, PartialEq)]
pub struct Scored {
    /// `<node>#<chunk>`.
    pub key: String,
    pub score: f64,
}

/// What a ranking may weigh beside the scores: the turn's time.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct RankCtx {
    pub now_ms: u64,
}

/// §5.1's verbs, each a pure function of its inputs.
pub trait MemoryScience: Send + Sync {
    /// The science and its parameters' digest.
    fn id(&self) -> ScienceId;
    /// The least fused score a candidate needs: below it, `threshold`.
    fn min_score(&self) -> f64;
    /// What to do with a node seen for the first time (row 57, 31a: the
    /// memory pass).
    fn gate(&self, fresh: &Fresh, near: &[Neighbour]) -> GateDecision;
    /// The gate's thresholds, merge then supersede, for the row that
    /// records its decision.
    fn gate_thresholds(&self) -> (f32, f32);
    /// One step of a node's retention fold (row 58, 32a: the retention
    /// projection); `None`: the science keeps no retention.
    fn schedule(&self, prior: Option<&Retention>, ev: &AccessEvent) -> Option<Retention>;
    /// The nodes the seeds' activation reaches (row 59, 32b: the adjacency
    /// projection), strongest first.
    fn activate(
        &self,
        g: &dyn Adjacency<String>,
        seeds: &[(String, f32)],
        budget: usize,
    ) -> Vec<(String, f32)>;
    /// Hints for tiering (row 60, 33): the nodes that may leave the
    /// resident tier, coldest first.
    fn decay_sweep(&self, now_ms: u64, view: &[Heat]) -> Vec<Demotion>;
    /// The final order of recall's candidates.
    fn rank(&self, fused: Vec<Scored>, ctx: &RankCtx) -> Vec<Scored>;
    /// Whether recall reads the memory pass's edges and prefers the newer
    /// node (31a's "deterministic freshness and provenance rules").
    fn prefers_newer(&self) -> bool;
}

/// The baseline (§2.3): no retention model and no activation. It ranks by
/// the index's fused order, and gates by cosine alone.
#[derive(Clone, Debug, PartialEq)]
pub struct Baseline {
    /// Its version: 1 (30a) ranks the fused order alone; 2 (31a) also reads
    /// the memory pass's edges and prefers the newer node.
    pub version: u32,
    /// The least fused score admitted; 0 admits every hit, until shadow's
    /// rows calibrate it.
    pub min_score: f64,
    /// A neighbour at this cosine or above is a near-duplicate (0.92, as a
    /// starting point, for Nomic v1.5's space).
    pub merge_cosine: f32,
    /// A correction whose top neighbour is at this cosine or above
    /// supersedes it, however close: the correction rule comes before the
    /// near-duplicate line (theseus-lx3x).
    pub supersede_cosine: f32,
    /// A node unread this many days is a demotion hint.
    pub idle_days: f64,
}

impl Default for Baseline {
    fn default() -> Self {
        Self {
            version: 2,
            min_score: 0.0,
            merge_cosine: 0.92,
            supersede_cosine: 0.75,
            idle_days: 30.0,
        }
    }
}

const DAY_MS: f64 = 86_400_000.0;

impl Baseline {
    /// Its parameters as one line: what the digest is of.
    fn canonical(&self) -> String {
        let mut c = format!(
            "min_score={};merge_cosine={};supersede_cosine={};idle_days={}",
            self.min_score, self.merge_cosine, self.supersede_cosine, self.idle_days
        );
        // The first version's line is as it was, so its digest is too.
        if self.version != 1 {
            c.push_str(&format!(";version={}", self.version));
        }
        c
    }
}

impl MemoryScience for Baseline {
    fn id(&self) -> ScienceId {
        ScienceId {
            name: "baseline",
            params: format!("{:016x}", fnv1a(self.canonical().as_bytes())),
        }
    }

    fn min_score(&self) -> f64 {
        self.min_score
    }

    fn gate(&self, fresh: &Fresh, near: &[Neighbour]) -> GateDecision {
        let top = near
            .iter()
            .filter(|n| !n.cosine.is_nan())
            .max_by(|a, b| a.cosine.total_cmp(&b.cosine));
        match top {
            // A correction first (theseus-lx3x, Eddie's decision 10): it
            // restates most of what it corrects, so it can pass the
            // near-duplicate line (0.954 on Nomic v1.5, live), and it
            // supersedes all the same.
            Some(n) if fresh.correction && n.cosine >= self.supersede_cosine => {
                GateDecision::Supersedes(n.node_id.clone())
            }
            Some(n) if n.cosine >= self.merge_cosine => GateDecision::MergeInto(n.node_id.clone()),
            _ => GateDecision::Store,
        }
    }

    fn gate_thresholds(&self) -> (f32, f32) {
        (self.merge_cosine, self.supersede_cosine)
    }

    fn schedule(&self, _prior: Option<&Retention>, _ev: &AccessEvent) -> Option<Retention> {
        None
    }

    fn activate(
        &self,
        _g: &dyn Adjacency<String>,
        _seeds: &[(String, f32)],
        _budget: usize,
    ) -> Vec<(String, f32)> {
        Vec::new()
    }

    fn decay_sweep(&self, now_ms: u64, view: &[Heat]) -> Vec<Demotion> {
        let mut out: Vec<Demotion> = view
            .iter()
            .map(|h| Demotion {
                node_id: h.node_id.clone(),
                idle_days: now_ms.saturating_sub(h.last_read_ms.max(h.written_ms)) as f64 / DAY_MS,
            })
            .filter(|d| d.idle_days >= self.idle_days)
            .collect();
        out.sort_by(|a, b| {
            b.idle_days
                .total_cmp(&a.idle_days)
                .then_with(|| a.node_id.cmp(&b.node_id))
        });
        out
    }

    fn prefers_newer(&self) -> bool {
        self.version >= 2
    }

    fn rank(&self, mut fused: Vec<Scored>, _ctx: &RankCtx) -> Vec<Scored> {
        // The fused order, strongest first; a tie by key, so the order is the
        // same however the index listed them.
        fused.sort_by(|a, b| b.score.total_cmp(&a.score).then_with(|| a.key.cmp(&b.key)));
        fused
    }
}

/// FNV-1a, 64 bits: a parameter set's digest, stable across builds.
fn fnv1a(bytes: &[u8]) -> u64 {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for b in bytes {
        h ^= u64::from(*b);
        h = h.wrapping_mul(0x0100_0000_01b3);
    }
    h
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::activation::AdjacencyList;
    use crate::{Access, Outcome};

    fn near(id: &str, cosine: f32) -> Neighbour {
        Neighbour {
            node_id: id.into(),
            cosine,
        }
    }

    /// §2.3's gate: a correction close enough supersedes, even past the
    /// near-duplicate line; a near-duplicate merges; anything else is stored.
    #[test]
    fn the_baseline_gate_follows_the_design_table() {
        let b = Baseline::default();
        let plain = Fresh {
            node_id: "n".into(),
            correction: false,
        };
        let fix = Fresh {
            correction: true,
            ..plain.clone()
        };
        assert_eq!(b.gate(&plain, &[]), GateDecision::Store);
        assert_eq!(
            b.gate(&plain, &[near("a", 0.5), near("b", 0.93)]),
            GateDecision::MergeInto("b".into())
        );
        assert_eq!(b.gate(&plain, &[near("a", 0.8)]), GateDecision::Store);
        assert_eq!(
            b.gate(&fix, &[near("a", 0.8)]),
            GateDecision::Supersedes("a".into())
        );
        assert_eq!(b.gate(&fix, &[near("a", 0.7)]), GateDecision::Store);
        // A correction past the merge line (theseus-lx3x: 0.954 on Nomic
        // v1.5, live): it supersedes, where a plain node merges.
        assert_eq!(
            b.gate(&fix, &[near("a", 0.954)]),
            GateDecision::Supersedes("a".into())
        );
        assert_eq!(
            b.gate(&plain, &[near("a", 0.954)]),
            GateDecision::MergeInto("a".into())
        );
        assert_eq!(b.gate(&plain, &[near("a", f32::NAN)]), GateDecision::Store);
    }

    /// No retention, and no activation.
    #[test]
    fn the_baseline_keeps_no_retention_and_spreads_nothing() {
        let b = Baseline::default();
        let ev = AccessEvent {
            at_ms: 1,
            access: Access::Used(Outcome::Ok),
        };
        assert_eq!(b.schedule(None, &ev), None);
        let mut g = AdjacencyList::default();
        g.link("a".to_string(), "b".to_string(), crate::EdgeKind::Neighbour);
        assert!(b.activate(&g, &[("a".into(), 1.0)], 10).is_empty());
    }

    /// The fused order, ties by key, whatever order the index listed them in.
    #[test]
    fn the_baseline_ranks_by_the_fused_score() {
        let s = |k: &str, score: f64| Scored {
            key: k.into(),
            score,
        };
        let ranked = Baseline::default().rank(
            vec![s("c", 0.1), s("b", 0.3), s("a", 0.3), s("d", 0.2)],
            &RankCtx::default(),
        );
        let keys: Vec<_> = ranked.iter().map(|s| s.key.as_str()).collect();
        assert_eq!(keys, ["a", "b", "d", "c"]);
    }

    /// Age and heat only: the long-unread, coldest first.
    #[test]
    fn the_baseline_sweep_hints_the_long_unread() {
        let day = 86_400_000u64;
        let now = 100 * day;
        let h = |id: &str, written: u64, read: u64| Heat {
            node_id: id.into(),
            written_ms: written * day,
            last_read_ms: read * day,
        };
        let out = Baseline::default().decay_sweep(
            now,
            &[
                h("fresh", 90, 95),
                h("cold", 10, 20),
                h("colder", 5, 0),
                h("old-but-read", 1, 99),
            ],
        );
        let ids: Vec<_> = out.iter().map(|d| d.node_id.as_str()).collect();
        assert_eq!(ids, ["colder", "cold"]);
        assert!((out[0].idle_days - 95.0).abs() < 1e-9);
    }

    /// The id names the parameters: a change of one changes the digest.
    #[test]
    fn the_id_names_the_parameter_set() {
        let a = Baseline::default().id();
        assert_eq!(a.name, "baseline");
        assert_eq!(a.params.len(), 16);
        assert_eq!(a, Baseline::default().id());
        let b = Baseline {
            min_score: 0.01,
            ..Baseline::default()
        }
        .id();
        assert_ne!(a.params, b.params);
        assert!(a.to_string().starts_with("baseline@"));
        // 31a's version is a new digest; the first keeps 30a's.
        let v1 = Baseline {
            version: 1,
            ..Baseline::default()
        };
        assert_ne!(v1.id().params, a.params);
        assert_eq!(
            v1.id().params,
            "46038939f14a4f49",
            "30a's digest, unchanged"
        );
    }
}
