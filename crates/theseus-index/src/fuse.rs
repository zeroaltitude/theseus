//! Reciprocal rank fusion (M6 §2.2's "Fusion"): each source ranks on its own
//! scale, and a hit's fused score is `Σ 1 / (60 + rank)` over the sources
//! that ranked it, so no source's raw scores are weighed against another's.

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

/// Fuse `lists` (each a source's name and its keys, best first, with their
/// raw scores). The keys come back in no particular order: the caller
/// sorts, with its own tie-break.
pub fn fuse<K: Hash + Eq + Clone>(lists: &[(&str, Vec<(K, f64)>)]) -> Vec<Fused<K>> {
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
            // A source that lists a key twice counts its better rank once.
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rank_fusion_sums_one_over_sixty_plus_rank_across_sources() {
        let bm25 = vec![("a", 9.0), ("b", 7.5), ("c", 1.0)];
        let vector = vec![("c", 0.91), ("a", 0.80), ("d", 0.5)];
        let mut got = fuse(&[("bm25", bm25), ("vector", vector)]);
        got.sort_by(|x, y| y.score.total_cmp(&x.score).then(x.key.cmp(y.key)));
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
        let mut again = fuse(&[
            ("bm25", vec![("a", 9.0), ("b", 7.5), ("c", 1.0)]),
            ("vector", louder),
        ]);
        again.sort_by(|x, y| y.score.total_cmp(&x.score).then(x.key.cmp(y.key)));
        assert_eq!(again.iter().map(|f| f.key).collect::<Vec<_>>(), keys);
        // A key listed twice by one source counts once, at its better rank.
        let twice = fuse(&[("bm25", vec![("a", 2.0), ("a", 1.0)])]);
        assert_eq!(twice.len(), 1);
        assert!((twice[0].score - 1.0 / 61.0).abs() < 1e-12);
    }
}
