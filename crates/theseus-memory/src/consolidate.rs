//! Consolidation's pure half (design M6 §2.7, step 31b): the clusters recall
//! admits together, a synthesis's citations and its deterministic checks,
//! and its shadow score. The core reads the rows and the nodes, asks the
//! profile and Jev, and writes; this decides.
//!
//! - [`clusters`]: node pairs admitted together in at least [`MIN_TURNS`]
//!   distinct turns form a graph; each connected component of
//!   [`MIN_NODES`] to [`MAX_NODES`] nodes is a cluster, unless it holds a
//!   node that may not be a source (a synthesis, a recall), or was
//!   synthesized before (its [`digest`]).
//! - [`sentences`] and [`check`]: a synthesis's sentences, each with the
//!   sources it cites (`[2]`, `[1][3]`, `[1, 3]`), and the deterministic
//!   checks: at most [`MAX_WORDS`] words, every sentence cites, every cited
//!   number names a source of the cluster.
//! - [`score`]: would a recall that admitted two or more of a synthesis's
//!   sources have selected it?

use std::collections::{BTreeMap, BTreeSet};

/// A pair must be admitted together in this many distinct turns.
pub const MIN_TURNS: usize = 3;
/// A cluster's least and most nodes.
pub const MIN_NODES: usize = 3;
pub const MAX_NODES: usize = 8;
/// A synthesis's most words.
pub const MAX_WORDS: usize = 120;

/// One recall's admitted nodes, from its `recall.shadow` or `recall.ran` row.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CoRecall {
    /// The turn it ran in (a row with none counts as its own turn).
    pub turn: String,
    pub nodes: Vec<String>,
}

/// A cluster to synthesize: its nodes, sorted, and how many distinct turns
/// admitted its weakest pair together.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Cluster {
    pub nodes: Vec<String>,
    pub digest: String,
    pub turns: usize,
}

/// Why a component is not a cluster.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Skip {
    /// Fewer than [`MIN_NODES`] or more than [`MAX_NODES`] nodes.
    Size,
    /// It holds a synthesis, or a recall: never a source.
    NotASource,
    /// Synthesized before.
    Done,
}

impl Skip {
    pub fn as_str(self) -> &'static str {
        match self {
            Skip::Size => "size",
            Skip::NotASource => "not_a_source",
            Skip::Done => "synthesized",
        }
    }
}

/// The digest of a cluster's nodes: FNV-1a over the sorted ids, 16 hex
/// digits, stable across builds.
pub fn digest(nodes: &[String]) -> String {
    let mut sorted: Vec<&str> = nodes.iter().map(String::as_str).collect();
    sorted.sort_unstable();
    sorted.dedup();
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for id in sorted {
        for b in id.bytes().chain(std::iter::once(b'\n')) {
            h ^= u64::from(b);
            h = h.wrapping_mul(0x0100_0000_01b3);
        }
    }
    format!("{h:016x}")
}

/// The clusters of `recalls`, and how many components each reason skipped.
/// `excluded` are the nodes that may not be sources (syntheses, recalls);
/// `done` the digests synthesized before.
pub fn clusters(
    recalls: &[CoRecall],
    excluded: &BTreeSet<String>,
    done: &BTreeSet<String>,
) -> (Vec<Cluster>, BTreeMap<Skip, usize>) {
    // Each pair's distinct turns.
    let mut pairs: BTreeMap<(&str, &str), BTreeSet<&str>> = BTreeMap::new();
    for r in recalls {
        let nodes: BTreeSet<&str> = r.nodes.iter().map(String::as_str).collect();
        let nodes: Vec<&str> = nodes.into_iter().collect();
        for (i, a) in nodes.iter().enumerate() {
            for b in &nodes[i + 1..] {
                pairs.entry((a, b)).or_default().insert(r.turn.as_str());
            }
        }
    }
    let strong: Vec<((&str, &str), usize)> = pairs
        .into_iter()
        .map(|(p, t)| (p, t.len()))
        .filter(|(_, n)| *n >= MIN_TURNS)
        .collect();
    // Components, by union-find over the strong pairs.
    let mut parent: BTreeMap<&str, &str> = BTreeMap::new();
    fn root<'a>(parent: &mut BTreeMap<&'a str, &'a str>, x: &'a str) -> &'a str {
        let mut r = x;
        while let Some(&p) = parent.get(r) {
            if p == r {
                break;
            }
            r = p;
        }
        parent.insert(x, r);
        r
    }
    for ((a, b), _) in &strong {
        parent.entry(a).or_insert(a);
        parent.entry(b).or_insert(b);
        let (ra, rb) = (root(&mut parent, a), root(&mut parent, b));
        if ra != rb {
            let (lo, hi) = if ra < rb { (ra, rb) } else { (rb, ra) };
            parent.insert(hi, lo);
        }
    }
    let ids: Vec<&str> = parent.keys().copied().collect();
    let mut comps: BTreeMap<&str, Vec<&str>> = BTreeMap::new();
    for id in ids {
        let r = root(&mut parent, id);
        comps.entry(r).or_default().push(id);
    }
    let mut out = Vec::new();
    let mut skipped: BTreeMap<Skip, usize> = BTreeMap::new();
    for nodes in comps.into_values() {
        let set: BTreeSet<&str> = nodes.iter().copied().collect();
        let weakest = strong
            .iter()
            .filter(|((a, b), _)| set.contains(a) && set.contains(b))
            .map(|(_, n)| *n)
            .min()
            .unwrap_or(0);
        let nodes: Vec<String> = nodes.into_iter().map(str::to_string).collect();
        let skip = if !(MIN_NODES..=MAX_NODES).contains(&nodes.len()) {
            Some(Skip::Size)
        } else if nodes.iter().any(|n| excluded.contains(n)) {
            Some(Skip::NotASource)
        } else if done.contains(&digest(&nodes)) {
            Some(Skip::Done)
        } else {
            None
        };
        match skip {
            Some(s) => *skipped.entry(s).or_default() += 1,
            None => out.push(Cluster {
                digest: digest(&nodes),
                nodes,
                turns: weakest,
            }),
        }
    }
    (out, skipped)
}

/// One sentence of a synthesis, and the source numbers it cites (from 1).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Sentence {
    /// Its words, citations left out.
    pub text: String,
    pub cites: Vec<usize>,
}

/// A synthesis's sentences: split after `.`, `!` or `?` and the citations
/// that follow them, each with the numbers its brackets hold.
pub fn sentences(text: &str) -> Vec<Sentence> {
    let mut out = Vec::new();
    let (mut words, mut cites) = (String::new(), Vec::new());
    let mut ended = false;
    let mut chars = text.chars().peekable();
    let flush = |words: &mut String, cites: &mut Vec<usize>, out: &mut Vec<Sentence>| {
        let w = words
            .split_whitespace()
            .collect::<Vec<_>>()
            .join(" ")
            .replace(" .", ".")
            .replace(" !", "!")
            .replace(" ?", "?");
        if !w.is_empty() || !cites.is_empty() {
            out.push(Sentence {
                text: w,
                cites: std::mem::take(cites),
            });
        }
        words.clear();
    };
    while let Some(c) = chars.next() {
        if c == '[' {
            let mut inner = String::new();
            for d in chars.by_ref() {
                if d == ']' {
                    break;
                }
                inner.push(d);
            }
            let nums: Vec<usize> = inner
                .split([',', ' ', ';'])
                .filter(|s| !s.is_empty())
                .map(|s| s.parse().unwrap_or(0))
                .collect();
            if nums.is_empty() {
                cites.push(0);
            }
            cites.extend(nums);
            continue;
        }
        if ended && !c.is_whitespace() {
            flush(&mut words, &mut cites, &mut out);
            ended = false;
        }
        if matches!(c, '.' | '!' | '?') {
            // A sentence ends here, after the citations that follow it; a
            // citation before the stop belongs to it too.
            words.push(c);
            ended = chars.peek().is_none_or(|n| n.is_whitespace() || *n == '[');
            continue;
        }
        words.push(c);
    }
    flush(&mut words, &mut cites, &mut out);
    out
}

/// What the deterministic checks found wrong, if anything.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Fault {
    Empty,
    /// More than [`MAX_WORDS`] words.
    Long(usize),
    /// A sentence (from 1) cites nothing.
    Uncited(usize),
    /// A sentence (from 1) cites a number that names no source.
    Unknown {
        sentence: usize,
        cited: usize,
    },
}

impl std::fmt::Display for Fault {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Fault::Empty => write!(f, "the synthesis is empty"),
            Fault::Long(n) => write!(f, "{n} words, over {MAX_WORDS}"),
            Fault::Uncited(s) => write!(f, "sentence {s} cites no source"),
            Fault::Unknown { sentence, cited } => {
                write!(
                    f,
                    "sentence {sentence} cites [{cited}], which is no source of the cluster"
                )
            }
        }
    }
}

/// The deterministic checks (§2.7): every sentence cites, every cited
/// number is one of the cluster's `sources`, and the whole is at most
/// [`MAX_WORDS`] words. The sentences, when it passes.
pub fn check(text: &str, sources: usize) -> Result<Vec<Sentence>, Fault> {
    let s = sentences(text);
    if s.iter().all(|s| s.text.is_empty()) {
        return Err(Fault::Empty);
    }
    let words: usize = s.iter().map(|s| s.text.split_whitespace().count()).sum();
    if words > MAX_WORDS {
        return Err(Fault::Long(words));
    }
    for (i, sentence) in s.iter().enumerate() {
        if sentence.cites.is_empty() {
            return Err(Fault::Uncited(i + 1));
        }
        if let Some(&c) = sentence.cites.iter().find(|&&c| c == 0 || c > sources) {
            return Err(Fault::Unknown {
                sentence: i + 1,
                cited: c,
            });
        }
    }
    Ok(s)
}

/// One item a recall admitted, as its row keeps it.
#[derive(Clone, Debug, PartialEq)]
pub struct Admitted {
    pub node_id: String,
    /// Its rank in the science's order, from 1.
    pub rank: usize,
    pub tokens: u64,
}

/// A synthesis's shadow score against one recall.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Scored {
    pub would_select: bool,
    /// Its rank, as its best admitted source's, ahead of it.
    pub rank: usize,
    /// How many of its sources the recall admitted.
    pub sources: usize,
}

/// Would a recall that admitted `admitted` (in rank order) have selected a
/// synthesis of `sources`, `tokens` long, under `max_items` and
/// `budget_tokens`? Rows keep no query, so it scores as its best admitted
/// source, ranked just ahead of it: selected when the items and tokens
/// ranked before it, and it, fit. `None`: fewer than two of its sources
/// were admitted.
pub fn score(
    sources: &[String],
    tokens: u64,
    admitted: &[Admitted],
    max_items: usize,
    budget_tokens: u64,
) -> Option<Scored> {
    let mine: Vec<&Admitted> = admitted
        .iter()
        .filter(|a| sources.contains(&a.node_id))
        .collect();
    if mine.len() < 2 {
        return None;
    }
    let best = mine.iter().map(|a| a.rank).min()?;
    let before: Vec<&Admitted> = admitted.iter().filter(|a| a.rank < best).collect();
    let used: u64 = before.iter().map(|a| a.tokens).sum();
    Some(Scored {
        would_select: before.len() < max_items && used + tokens <= budget_tokens,
        rank: best,
        sources: mine.len(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn row(turn: &str, nodes: &[&str]) -> CoRecall {
        CoRecall {
            turn: turn.into(),
            nodes: nodes.iter().map(|s| s.to_string()).collect(),
        }
    }

    fn ids(c: &Cluster) -> Vec<&str> {
        c.nodes.iter().map(String::as_str).collect()
    }

    /// From a table of rows: a triangle admitted together in three turns is
    /// a cluster; a pair seen in two turns links nothing; the same turn
    /// twice counts once; a component too small, or too large, is skipped
    /// for its size; one holding a synthesis is not a cluster; and one
    /// synthesized before is not proposed again.
    #[test]
    fn clusters_come_from_pairs_admitted_together_in_three_turns() {
        let rows = vec![
            row("t1", &["a", "b", "c", "x"]),
            row("t2", &["a", "b", "c"]),
            row("t3", &["c", "b", "a", "y"]),
            // x and y: two turns with a, never three.
            row("t4", &["a", "x"]),
            // The same turn twice counts once.
            row("t5", &["p", "q"]),
            row("t5", &["p", "q"]),
            row("t6", &["p", "q"]),
            row("t7", &["p", "q"]),
            // A big component: nine nodes in a chain, three turns each.
            row(
                "u1",
                &["n1", "n2", "n3", "n4", "n5", "n6", "n7", "n8", "n9"],
            ),
            row(
                "u2",
                &["n1", "n2", "n3", "n4", "n5", "n6", "n7", "n8", "n9"],
            ),
            row(
                "u3",
                &["n1", "n2", "n3", "n4", "n5", "n6", "n7", "n8", "n9"],
            ),
            // One with a synthesis in it.
            row("v1", &["s1", "s2", "syn_1"]),
            row("v2", &["s1", "s2", "syn_1"]),
            row("v3", &["s1", "s2", "syn_1"]),
            // One synthesized before.
            row("w1", &["d1", "d2", "d3"]),
            row("w2", &["d1", "d2", "d3"]),
            row("w3", &["d1", "d2", "d3"]),
        ];
        let excluded = BTreeSet::from(["syn_1".to_string()]);
        let done = BTreeSet::from([digest(&["d3".into(), "d1".into(), "d2".into()])]);
        let (got, skipped) = clusters(&rows, &excluded, &done);
        assert_eq!(got.len(), 1, "{got:?}");
        assert_eq!(ids(&got[0]), ["a", "b", "c"]);
        assert_eq!(got[0].turns, 3);
        assert_eq!(got[0].digest, digest(&["c".into(), "a".into(), "b".into()]));
        assert_eq!(
            skipped.get(&Skip::Size),
            Some(&2),
            "p–q too small, n1–n9 too big"
        );
        assert_eq!(skipped.get(&Skip::NotASource), Some(&1));
        assert_eq!(skipped.get(&Skip::Done), Some(&1));
    }

    /// Two triangles joined by one strong pair are one component.
    #[test]
    fn a_component_joins_its_strong_pairs() {
        let mut rows = Vec::new();
        for t in ["t1", "t2", "t3"] {
            rows.push(row(t, &["a", "b", "c"]));
            rows.push(row(t, &["c", "d", "e"]));
        }
        let (got, _) = clusters(&rows, &BTreeSet::new(), &BTreeSet::new());
        assert_eq!(got.len(), 1);
        assert_eq!(ids(&got[0]), ["a", "b", "c", "d", "e"]);
    }

    /// Sentences end after their citations; brackets hold one number or a
    /// list.
    #[test]
    fn sentences_carry_their_citations() {
        let s = sentences(
            "The relay listens on port 7714 [1]. It logs to /var/log/relay.log [2][3]. \
             It restarts nightly [1, 3].",
        );
        assert_eq!(s.len(), 3);
        assert_eq!(s[0].text, "The relay listens on port 7714.");
        assert_eq!(s[0].cites, [1]);
        assert_eq!(s[1].cites, [2, 3]);
        assert_eq!(s[2].cites, [1, 3]);
        // A citation before the stop is the sentence's too.
        let s = sentences("It restarts nightly [2]. Done [1].");
        assert_eq!(s[0].cites, [2]);
        assert_eq!(s[1].cites, [1]);
    }

    /// The deterministic checks: every sentence cites, every number names
    /// a source, at most 120 words.
    #[test]
    fn the_deterministic_checks_reject_what_they_must() {
        assert!(check("A fact [1]. Another [2].", 2).is_ok());
        assert_eq!(check("A fact [1]. Another.", 2), Err(Fault::Uncited(2)));
        assert_eq!(
            check("A fact [1]. Another [4].", 3),
            Err(Fault::Unknown {
                sentence: 2,
                cited: 4
            })
        );
        assert_eq!(
            check("A fact [x].", 3),
            Err(Fault::Unknown {
                sentence: 1,
                cited: 0
            })
        );
        assert_eq!(check("  ", 3), Err(Fault::Empty));
        let long = format!("{} [1].", "word ".repeat(121));
        assert_eq!(check(&long, 1), Err(Fault::Long(121)));
    }

    /// The shadow score: as its best admitted source, ahead of it; none
    /// with fewer than two sources admitted; not selected past the items or
    /// the tokens.
    #[test]
    fn a_synthesis_scores_as_its_best_admitted_source() {
        let a = |id: &str, rank: usize| Admitted {
            node_id: id.into(),
            rank,
            tokens: 100,
        };
        let sources = vec!["s1".to_string(), "s2".to_string(), "s3".to_string()];
        let admitted = [a("x", 1), a("s2", 2), a("y", 3), a("s1", 4)];
        let got = score(&sources, 80, &admitted, 6, 1500).unwrap();
        assert_eq!((got.would_select, got.rank, got.sources), (true, 2, 2));
        assert_eq!(score(&sources, 80, &[a("s1", 1), a("x", 2)], 6, 1500), None);
        // One item before it, and room for one: selected; none: not.
        assert!(
            score(&sources, 80, &admitted, 2, 1500)
                .unwrap()
                .would_select
        );
        assert!(
            !score(&sources, 80, &admitted, 1, 1500)
                .unwrap()
                .would_select
        );
        assert!(!score(&sources, 80, &admitted, 6, 150).unwrap().would_select);
    }
}
