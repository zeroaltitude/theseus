//! The probe's results (theseus-zaz.11): per item, where the gold ranked,
//! and per family, how much of it ranks in the top k. `tender.rs` fills
//! them by asking a running index tender (`theseus-exam probe --tender`), which
//! answers BM25 and entities with no model; this module holds the shapes and
//! the recall arithmetic.
//!
//! A difficulty probe needs a running tender over the exam's store, and the
//! exam does not start one itself: a scratch daemon does. `write-store` into
//! `<state>/store`, start `theseusd` on that `--state-dir`, and the daemon
//! supervises `theseus-index serve`, which answers on `<state>/index/sock`:
//! that socket is `probe --tender`'s. Without the model weights the tender
//! answers BM25 and entities alone. (`theseus-index serve --store <state>/store
//! --index <dir> --no-vectors` by hand is the same tender without the daemon.)

use crate::item::Family;

/// The cut-offs reported: 6 is the recall budget's item count (design
/// §2.4); 20 is the window `+rerank` reorders (§2.9).
pub const KS: [usize; 5] = [1, 3, 6, 10, 20];

/// One item's result.
#[derive(Debug, Clone)]
pub struct ItemProbe {
    pub id: String,
    pub family: Family,
    pub held_out: bool,
    /// Each gold node's rank, from 1, in the item's gold order.
    pub gold_ranks: Vec<usize>,
    /// The best-ranked node of the item's own past that is not gold, and its
    /// rank: the trap, the stale value, the near-duplicate.
    pub trap: Option<(String, usize)>,
}

/// Recall over a set of items, at each of `KS`.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Recall {
    pub items: usize,
    pub gold: usize,
    /// Gold nodes ranked in the top k.
    pub nodes: [usize; 5],
    /// Items with all their gold in the top k.
    pub items_all: [usize; 5],
}

pub fn recall<'a>(ps: impl IntoIterator<Item = &'a ItemProbe>) -> Recall {
    let mut r = Recall::default();
    for p in ps {
        r.items += 1;
        r.gold += p.gold_ranks.len();
        for (j, &k) in KS.iter().enumerate() {
            r.nodes[j] += p.gold_ranks.iter().filter(|&&g| g <= k).count();
            r.items_all[j] += usize::from(p.gold_ranks.iter().all(|&g| g <= k));
        }
    }
    r
}

/// The rows a probe's tables show: each family with items, the ten
/// base families together, the hard ones together, and all.
pub fn rows(ps: &[ItemProbe]) -> Vec<(String, Recall)> {
    let mut out = Vec::new();
    for f in Family::ALL {
        let r = recall(ps.iter().filter(|p| p.family == f));
        if r.items > 0 {
            out.push((f.as_str().to_string(), r));
        }
    }
    let v1 = recall(ps.iter().filter(|p| !p.family.is_hard()));
    let hard = recall(ps.iter().filter(|p| p.family.is_hard()));
    if hard.items > 0 && v1.items > 0 {
        out.push(("the ten base families".into(), v1));
        out.push(("the hard families".into(), hard));
    }
    out.push(("**all**".into(), recall(ps)));
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn p(id: &str, family: Family, gold_ranks: &[usize]) -> ItemProbe {
        ItemProbe {
            id: id.into(),
            family,
            held_out: false,
            gold_ranks: gold_ranks.to_vec(),
            trap: None,
        }
    }

    /// By hand: two fact items (gold ranks 1; 2 and 7), one paraphrase (4).
    #[test]
    fn recall_counts_gold_nodes_and_items_with_all_their_gold() {
        let ps = [
            p("a", Family::Fact, &[1]),
            p("b", Family::Fact, &[2, 7]),
            p("c", Family::Paraphrase, &[4]),
        ];
        let r = recall(&ps);
        assert_eq!((r.items, r.gold), (3, 4));
        assert_eq!(r.nodes, [1, 2, 3, 4, 4]);
        assert_eq!(r.items_all, [1, 1, 2, 3, 3]);
        let names: Vec<String> = rows(&ps).into_iter().map(|(n, _)| n).collect();
        assert_eq!(
            names,
            [
                "fact",
                "paraphrase",
                "the ten base families",
                "the hard families",
                "**all**"
            ]
        );
    }
}
