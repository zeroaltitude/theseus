//! The `+retention` arm's science (design M6 §2.7, §2.9; step 32a's
//! wire-in): `baseline` in every verb but two. Its `schedule` is FSRS-6's
//! fold step (`fsrs6-default`), and its `rank` weighs each candidate's fused
//! score by the node's retrievability at the turn's time:
//!
//! `score = fused × ((1 − w) + w × R(now))`, with `w` = [`WEIGHT`].
//!
//! - **A node with no retention keeps its fused score** (as though `R` were
//!   1). Every node the memory pass has labeled has one (its first sight),
//!   so a node without is one written since the pass last ran, or before the
//!   projection was built: it was seen moments ago, when `R` is 1 for any
//!   stability, so its fused score is what retention would leave it.
//! - **The form and the weight are versioned data** in the parameter set's
//!   digest (`retention@<16 hex>`), with FSRS-6's 21 parameters and the
//!   baseline's own line, so a recall's row names exactly what ranked it.
//! - Pure: the core fills `RankCtx::retention` from its projection.

use crate::activation::Adjacency;
use crate::fsrs::{Fsrs6, Retention};
use crate::science::{
    fnv1a, Baseline, Demotion, Fresh, GateDecision, Heat, MemoryScience, Neighbour, RankCtx,
    ScienceId, Scored,
};
use crate::AccessEvent;

/// How much of the fused score retrievability can take away: at 0.5 a node
/// recall has forgotten (`R` near 0) keeps half its score, and two equal
/// candidates order by `R`. A starting point; the exam and the canary tune it.
pub const WEIGHT: f64 = 0.5;

/// The form's version: 1 is `fused × ((1 − w) + w × R)`.
pub const FORM: u32 = 1;

/// The `+retention` arm: the baseline, ranked by retention too.
#[derive(Clone, Debug, PartialEq)]
pub struct RetentionRank {
    pub base: Baseline,
    pub fsrs: Fsrs6,
    pub weight: f64,
    pub form: u32,
}

impl Default for RetentionRank {
    fn default() -> Self {
        Self {
            base: Baseline::default(),
            fsrs: Fsrs6::default(),
            weight: WEIGHT,
            form: FORM,
        }
    }
}

impl RetentionRank {
    /// Its parameters as one line: what the digest is of.
    fn canonical(&self) -> String {
        let w: Vec<String> = self.fsrs.params().iter().map(|x| x.to_string()).collect();
        format!(
            "{};form={};weight={};fsrs6={}",
            self.base.canonical(),
            self.form,
            self.weight,
            w.join(",")
        )
    }

    /// A candidate's score under the arm: its fused score, weighed by its
    /// node's retrievability at `now_ms` when it has a retention.
    pub fn score(&self, fused: f64, retention: Option<&Retention>, now_ms: u64) -> f64 {
        match retention {
            Some(r) => {
                let rr = self.fsrs.retrievability_at(r, now_ms);
                fused * ((1.0 - self.weight) + self.weight * rr)
            }
            None => fused,
        }
    }
}

/// `<node>#<chunk>`'s node.
fn node_of(key: &str) -> &str {
    key.rsplit_once('#').map_or(key, |(n, _)| n)
}

impl MemoryScience for RetentionRank {
    fn id(&self) -> ScienceId {
        ScienceId {
            name: "retention",
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
        self.fsrs.step(prior.copied(), ev)
    }

    fn activate(
        &self,
        g: &dyn Adjacency<String>,
        seeds: &[(String, f32)],
        budget: usize,
    ) -> Vec<(String, f32)> {
        self.base.activate(g, seeds, budget)
    }

    fn decay_sweep(&self, now_ms: u64, view: &[Heat]) -> Vec<Demotion> {
        self.base.decay_sweep(now_ms, view)
    }

    fn rank(&self, fused: Vec<Scored>, ctx: &RankCtx) -> Vec<Scored> {
        let mut out: Vec<Scored> = fused
            .into_iter()
            .map(|s| Scored {
                score: self.score(s.score, ctx.retention.get(node_of(&s.key)), ctx.now_ms),
                key: s.key,
            })
            .collect();
        // Strongest first; a tie by key, as the baseline's.
        out.sort_by(|a, b| b.score.total_cmp(&a.score).then_with(|| a.key.cmp(&b.key)));
        out
    }

    fn prefers_newer(&self) -> bool {
        self.base.prefers_newer()
    }

    fn reads_retention(&self) -> bool {
        true
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use super::*;
    use crate::fsrs::MS_PER_DAY;
    use crate::{Access, Durability, Outcome};

    const T0: u64 = 1_700_000_000_000;
    const DAY: u64 = MS_PER_DAY as u64;

    fn s(k: &str, score: f64) -> Scored {
        Scored {
            key: k.into(),
            score,
        }
    }

    fn fold(f: &Fsrs6, evs: &[(u64, Access)]) -> Retention {
        let evs: Vec<AccessEvent> = evs
            .iter()
            .map(|&(at_ms, access)| AccessEvent { at_ms, access })
            .collect();
        f.fold(&evs).unwrap()
    }

    /// Two candidates with equal fused scores order by retention: the one
    /// used `ok` above the one used and `corrected`, a week on; a node with
    /// no retention keeps its fused score; and the baseline ignores it all.
    #[test]
    fn equal_fused_scores_order_by_retention() {
        let arm = RetentionRank::default();
        let first = Access::FirstSight(Durability::Medium);
        let ok = fold(
            &arm.fsrs,
            &[(T0, first), (T0 + 2 * DAY, Access::Used(Outcome::Ok))],
        );
        let bad = fold(
            &arm.fsrs,
            &[
                (T0, first),
                (T0 + 2 * DAY, Access::Used(Outcome::Corrected)),
            ],
        );
        let ctx = RankCtx {
            now_ms: T0 + 9 * DAY,
            retention: BTreeMap::from([("nod_ash".to_string(), bad), ("nod_oak".to_string(), ok)]),
        };
        // `nod_ash` sorts first by key: only retention can put `nod_oak` above it.
        let fused = vec![
            s("nod_ash#0", 0.02),
            s("nod_oak#0", 0.02),
            s("nod_elm#0", 0.015),
        ];
        let ranked = arm.rank(fused.clone(), &ctx);
        let keys: Vec<&str> = ranked.iter().map(|s| s.key.as_str()).collect();
        assert_eq!(keys, ["nod_oak#0", "nod_ash#0", "nod_elm#0"], "{ranked:?}");
        assert!(ranked[0].score > ranked[1].score);
        // No retention: the fused score, untouched.
        assert_eq!(ranked[2].score, 0.015);
        let base = Baseline::default().rank(fused, &ctx);
        let keys: Vec<&str> = base.iter().map(|s| s.key.as_str()).collect();
        assert_eq!(keys, ["nod_ash#0", "nod_oak#0", "nod_elm#0"]);
    }

    /// The score is the form's: `R` of 1 keeps the fused score, it falls as
    /// `R` does, and never below `1 − w` of it.
    #[test]
    fn the_score_follows_the_form() {
        let arm = RetentionRank::default();
        let r = arm.fsrs.initial(crate::Grade::Again, T0);
        assert!((arm.score(0.04, Some(&r), T0) - 0.04).abs() < 1e-12);
        let day = arm.score(0.04, Some(&r), T0 + DAY);
        let far = arm.score(0.04, Some(&r), T0 + 3650 * DAY);
        assert!(0.02 < far && far < day && day < 0.04, "{far} {day}");
        let rr = arm.fsrs.retrievability_at(&r, T0 + DAY);
        assert!((day - 0.04 * (0.5 + 0.5 * rr)).abs() < 1e-12);
        assert_eq!(arm.score(0.04, None, T0), 0.04);
    }

    /// The id names the form, the weight, FSRS-6's parameters and the
    /// baseline's: a change of any one is a new digest.
    #[test]
    fn the_id_names_form_weight_and_parameters() {
        let a = RetentionRank::default();
        assert_eq!(a.id().name, "retention");
        assert_eq!(a.id().params.len(), 16);
        assert_eq!(a.id(), RetentionRank::default().id());
        let mut w = crate::FSRS6_DEFAULT;
        w[20] = 0.2;
        for other in [
            RetentionRank {
                weight: 0.3,
                ..a.clone()
            },
            RetentionRank {
                form: 2,
                ..a.clone()
            },
            RetentionRank {
                fsrs: Fsrs6::new(w).unwrap(),
                ..a.clone()
            },
            RetentionRank {
                base: Baseline {
                    min_score: 0.01,
                    ..Baseline::default()
                },
                ..a.clone()
            },
        ] {
            assert_ne!(other.id().params, a.id().params, "{other:?}");
        }
        assert!(a.reads_retention());
        assert!(!Baseline::default().reads_retention());
        // Its schedule is FSRS-6's step: a shown node is no review.
        let ev = AccessEvent {
            at_ms: T0,
            access: Access::Shown,
        };
        assert_eq!(a.schedule(None, &ev), None);
        let used = AccessEvent {
            at_ms: T0,
            access: Access::Used(Outcome::Ok),
        };
        assert_eq!(a.schedule(None, &used), a.fsrs.step(None, &used));
    }
}
