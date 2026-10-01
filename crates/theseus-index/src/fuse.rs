//! Reciprocal rank fusion (M6 §2.2's "Fusion"), weighted by source
//! (theseus-jz8): each source ranks on its own scale, and a hit's fused score
//! is `Σ w / (60 + rank)` over the sources that ranked it, so no source's raw
//! scores are weighed against another's. With every weight 1 it is the
//! design's `Σ 1 / (60 + rank)`, bit for bit.

use std::collections::{BTreeMap, HashMap};
use std::hash::Hash;

use crate::proto::SourceRank;

/// The fusion's constant.
pub const RRF_K: f64 = 60.0;

/// One fused key: its score, and its rank and raw score in each source.
#[derive(Debug, Clone, PartialEq)]
pub struct Fused<K> {
    pub key: K,
    pub score: f64,
    pub sources: BTreeMap<String, SourceRank>,
}

/// One source's list: its name, its weight, and its keys, best first, with
/// their raw scores.
pub type Ranked<'a, K> = (&'a str, f64, Vec<(K, f64)>);

/// Fuse `lists`. The keys come back in no particular order: the caller
/// sorts, with its own tie-break.
pub fn fuse<K: Hash + Eq + Clone>(lists: &[Ranked<'_, K>]) -> Vec<Fused<K>> {
    let mut at: HashMap<K, usize> = HashMap::new();
    let mut out: Vec<Fused<K>> = Vec::new();
    for (source, weight, ranked) in lists {
        for (i, (key, score)) in ranked.iter().enumerate() {
            let rank = i + 1;
            let n = *at.entry(key.clone()).or_insert_with(|| {
                out.push(Fused {
                    key: key.clone(),
                    score: 0.0,
                    sources: BTreeMap::new(),
                });
                out.len() - 1
            });
            let f = &mut out[n];
            // A source that lists a key twice counts its better rank once.
            if f.sources.contains_key(*source) {
                continue;
            }
            f.score += weight / (RRF_K + rank as f64);
            f.sources.insert(
                source.to_string(),
                SourceRank {
                    rank,
                    score: *score,
                },
            );
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sorted<K: Ord + Clone>(mut v: Vec<Fused<K>>) -> Vec<Fused<K>> {
        v.sort_by(|x, y| y.score.total_cmp(&x.score).then(x.key.cmp(&y.key)));
        v
    }

    #[test]
    fn rank_fusion_sums_one_over_sixty_plus_rank_across_sources() {
        let bm25 = vec![("a", 9.0), ("b", 7.5), ("c", 1.0)];
        let vector = vec![("c", 0.91), ("a", 0.80), ("d", 0.5)];
        let got = sorted(fuse(&[("bm25", 1.0, bm25), ("vector", 1.0, vector)]));
        let keys: Vec<&str> = got.iter().map(|f| f.key).collect();
        // a: 1/61 + 1/62; c: 1/63 + 1/61; b: 1/62; d: 1/63.
        assert_eq!(keys, ["a", "c", "b", "d"]);
        let a = &got[0];
        assert!((a.score - (1.0 / 61.0 + 1.0 / 62.0)).abs() < 1e-12);
        assert_eq!(
            a.sources["bm25"],
            SourceRank {
                rank: 1,
                score: 9.0
            }
        );
        assert_eq!(
            a.sources["vector"],
            SourceRank {
                rank: 2,
                score: 0.80
            }
        );
        assert!((got[1].score - (1.0 / 63.0 + 1.0 / 61.0)).abs() < 1e-12);
        assert_eq!(got[3].sources.len(), 1);
        // Raw scores never enter: scaling one source changes nothing.
        let louder = vec![("c", 910.0), ("a", 800.0), ("d", 500.0)];
        let again = sorted(fuse(&[
            ("bm25", 1.0, vec![("a", 9.0), ("b", 7.5), ("c", 1.0)]),
            ("vector", 1.0, louder),
        ]));
        assert_eq!(again.iter().map(|f| f.key).collect::<Vec<_>>(), keys);
        // A key listed twice by one source counts once, at its better rank.
        let twice = fuse(&[("bm25", 1.0, vec![("a", 2.0), ("a", 1.0)])]);
        assert_eq!(twice.len(), 1);
        assert!((twice[0].score - 1.0 / 61.0).abs() < 1e-12);
    }

    /// The fusion as it was before weights (29c's `fuse`, verbatim but for
    /// its argument's shape): every source counts `1 / (60 + rank)`.
    fn unweighted<K: Hash + Eq + Clone>(lists: &[(&str, Vec<(K, f64)>)]) -> Vec<Fused<K>> {
        let mut at: HashMap<K, usize> = HashMap::new();
        let mut out: Vec<Fused<K>> = Vec::new();
        for (source, ranked) in lists {
            for (i, (key, score)) in ranked.iter().enumerate() {
                let rank = i + 1;
                let n = *at.entry(key.clone()).or_insert_with(|| {
                    out.push(Fused {
                        key: key.clone(),
                        score: 0.0,
                        sources: BTreeMap::new(),
                    });
                    out.len() - 1
                });
                let f = &mut out[n];
                if f.sources.contains_key(*source) {
                    continue;
                }
                f.score += 1.0 / (RRF_K + rank as f64);
                f.sources.insert(
                    source.to_string(),
                    SourceRank {
                        rank,
                        score: *score,
                    },
                );
            }
        }
        out
    }

    /// Weights of 1 are the unweighted fusion exactly: the same keys in the
    /// same order, the same scores to the bit, the same ranks, over random
    /// lists (repeats, overlaps, empty sources).
    #[test]
    fn weights_of_one_reproduce_the_unweighted_fusion_bit_for_bit() {
        let mut x: u64 = 0x9e37_79b9_7f4a_7c15;
        let mut next = move |n: u64| {
            x ^= x << 13;
            x ^= x >> 7;
            x ^= x << 17;
            x % n
        };
        for _ in 0..500 {
            let lists: Vec<(&str, Vec<(u64, f64)>)> = ["bm25", "entity", "vector"]
                .into_iter()
                .map(|s| {
                    let len = next(40);
                    let keys = 1 + next(60);
                    (
                        s,
                        (0..len)
                            .map(|_| (next(keys), next(1000) as f64 / 7.0))
                            .collect(),
                    )
                })
                .collect();
            let weighted: Vec<Ranked<'_, u64>> =
                lists.iter().map(|(s, l)| (*s, 1.0, l.clone())).collect();
            let (a, b) = (unweighted(&lists), fuse(&weighted));
            assert_eq!(a.len(), b.len());
            for (p, q) in a.iter().zip(&b) {
                assert_eq!(p.key, q.key);
                assert_eq!(p.score.to_bits(), q.score.to_bits());
                assert_eq!(p.sources, q.sources);
            }
        }
    }

    /// The brief's case: a decoy BM25 ranks 1st and vectors 30th, against a
    /// gold only vectors find, 2nd. Equal weights let the decoy win
    /// (1/61 + 1/90 = 0.0275 against 1/62 = 0.0161); the gold wins once the
    /// vector weight w passes 3.27 (w/62 > 1/61 + w/90).
    #[test]
    fn a_vector_weight_lets_a_confident_vector_find_through() {
        let vector: Vec<(&str, f64)> = std::iter::once(("x", 0.9))
            .chain(std::iter::once(("gold", 0.8)))
            .chain((3..30).map(|_| ("filler", 0.5)))
            .chain(std::iter::once(("decoy", 0.3)))
            .collect();
        let order = |w: f64| {
            let mut v: Vec<(String, f64)> = fuse(&[
                ("bm25", 1.0, vec![("decoy", 12.0)]),
                ("vector", w, vector.clone()),
            ])
            .into_iter()
            .filter(|f| f.key == "gold" || f.key == "decoy")
            .map(|f| (f.key.to_string(), f.score))
            .collect();
            v.sort_by(|a, b| b.1.total_cmp(&a.1));
            v
        };
        // "filler" repeats, so the decoy is the vector source's 30th entry.
        assert_eq!(vector.len(), 30);
        let eq = order(1.0);
        assert_eq!(eq[0].0, "decoy");
        assert!((eq[0].1 - (1.0 / 61.0 + 1.0 / 90.0)).abs() < 1e-12);
        assert!((eq[1].1 - 1.0 / 62.0).abs() < 1e-12);
        assert_eq!(order(3.0)[0].0, "decoy");
        assert_eq!(order(3.3)[0].0, "gold");
        let w4 = order(4.0);
        assert_eq!(w4[0].0, "gold");
        assert!((w4[0].1 - 4.0 / 62.0).abs() < 1e-12);
        // A weight of 0 keeps a source's ranks in view and out of the score.
        let zero = fuse(&[("bm25", 0.0, vec![("a", 1.0)])]);
        assert_eq!(zero[0].score, 0.0);
        assert_eq!(zero[0].sources["bm25"].rank, 1);
    }
}
