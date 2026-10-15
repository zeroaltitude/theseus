//! Clusters of near-duplicate lines, and the rules that rank and filter them
//! (theseus-slvl.6). Pure: no store, no clock, no model.
//!
//! - **Near-duplicates** are lines whose content words overlap (Jaccard at
//!   [`JOIN`] or more, or one nearly inside the other), joined greedily to the
//!   first line of a cluster, newest lines first, through an inverted index
//!   of the leaders' words so 10,000 lines do not compare pairwise.
//! - **Support** is the distinct episodes of a cluster; the rank is support,
//!   then how recently it was said.
//! - **Fresh** clusters are those whose episodes are mostly not in the set
//!   already seen (proposed, answered, or sent to the model before).

use std::collections::{HashMap, HashSet};

use super::evidence::Episode;

/// The Jaccard overlap at which a line joins a cluster.
pub const JOIN: f64 = 0.45;
/// The overlap at which a statement repeats a text it is compared with.
pub const SAME: f64 = 0.6;
/// The most lines a cluster keeps (the newest).
pub const KEEP_LINES: usize = 12;

/// One line of a cluster.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Member {
    /// The episode's index in the scan.
    pub episode: usize,
    pub text: String,
    pub at_ms: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Cluster {
    /// Newest first, at most [`KEEP_LINES`].
    pub lines: Vec<Member>,
    /// The distinct episodes, by scan index, newest line first.
    pub episodes: Vec<usize>,
    pub newest_ms: u64,
}

const STOP: &[&str] = &[
    "the", "and", "for", "that", "this", "with", "you", "your", "are", "was", "not", "but", "have",
    "has", "had", "can", "will", "would", "should", "could", "from", "they", "them", "their",
    "its", "his", "her", "our", "out", "one", "all", "any", "when", "what", "which", "who", "how",
    "than", "then", "into", "about", "just", "also", "been", "being", "does", "did", "dont",
    "doesnt", "very", "more", "some", "such", "only", "use", "make", "need", "want", "like", "get",
    "got", "let", "say", "said",
];

/// The content words of `s`: lowercase letters and digits, three characters
/// or more, no stop words, a trailing "s" dropped. Sorted and distinct.
pub fn words(s: &str) -> Vec<u64> {
    let mut out: Vec<u64> = s
        .split(|c: char| !c.is_alphanumeric())
        .filter(|w| w.chars().count() >= 3)
        .map(|w| w.to_lowercase())
        .filter(|w| !STOP.contains(&w.as_str()))
        .map(|w| {
            w.strip_suffix('s')
                .filter(|x| x.len() >= 3)
                .unwrap_or(&w)
                .to_string()
        })
        .map(|w| hash(&w))
        .collect();
    out.sort_unstable();
    out.dedup();
    out
}

fn hash(w: &str) -> u64 {
    // FNV-1a: stable across runs, which a hash for equality needs.
    let mut h = 0xcbf2_9ce4_8422_2325_u64;
    for b in w.bytes() {
        h ^= u64::from(b);
        h = h.wrapping_mul(0x0100_0000_01b3);
    }
    h
}

fn shared(a: &[u64], b: &[u64]) -> usize {
    let (mut i, mut j, mut n) = (0, 0, 0);
    while i < a.len() && j < b.len() {
        match a[i].cmp(&b[j]) {
            std::cmp::Ordering::Less => i += 1,
            std::cmp::Ordering::Greater => j += 1,
            std::cmp::Ordering::Equal => {
                n += 1;
                i += 1;
                j += 1;
            }
        }
    }
    n
}

/// Whether word sets `a` and `b` overlap at `bar` (Jaccard), or one of at
/// least `inside` words lies almost wholly inside the other.
fn overlaps(a: &[u64], b: &[u64], bar: f64, inside: usize) -> bool {
    if a.is_empty() || b.is_empty() {
        return false;
    }
    let n = shared(a, b) as f64;
    let (la, lb) = (a.len() as f64, b.len() as f64);
    n / (la + lb - n) >= bar || (la.min(lb) >= inside as f64 && n / la.min(lb) >= 0.9)
}

/// Whether two texts say the same thing, by their words.
pub fn similar(a: &str, b: &str) -> bool {
    overlaps(&words(a), &words(b), SAME, 2)
}

/// Cluster the lines of `eps`. Deterministic: lines are taken newest first,
/// ties by episode and text.
pub fn cluster(eps: &[Episode]) -> Vec<Cluster> {
    let mut all: Vec<(usize, &str, u64)> = eps
        .iter()
        .enumerate()
        .flat_map(|(i, e)| e.lines.iter().map(move |l| (i, l.text.as_str(), l.at_ms)))
        .collect();
    all.sort_by(|a, b| b.2.cmp(&a.2).then(a.0.cmp(&b.0)).then(a.1.cmp(b.1)));
    let mut leaders: Vec<Vec<u64>> = Vec::new();
    let mut clusters: Vec<Cluster> = Vec::new();
    let mut post: HashMap<u64, Vec<usize>> = HashMap::new();
    for (ep, text, at) in all {
        let w = words(text);
        if w.is_empty() {
            continue;
        }
        let mut hits: HashMap<usize, usize> = HashMap::new();
        for h in &w {
            for &c in post.get(h).into_iter().flatten() {
                *hits.entry(c).or_default() += 1;
            }
        }
        let mut best: Option<usize> = None;
        let mut cand: Vec<usize> = hits
            .into_iter()
            .filter(|(_, n)| *n >= 2 || w.len() == 1)
            .map(|(c, _)| c)
            .collect();
        cand.sort_unstable();
        for c in cand {
            if overlaps(&w, &leaders[c], JOIN, 4) {
                best = Some(c);
                break;
            }
        }
        let m = Member {
            episode: ep,
            text: text.to_string(),
            at_ms: at,
        };
        match best {
            Some(c) => {
                let cl = &mut clusters[c];
                if !cl.episodes.contains(&ep) {
                    cl.episodes.push(ep);
                }
                if cl.lines.len() < KEEP_LINES {
                    cl.lines.push(m);
                }
            }
            None => {
                let id = clusters.len();
                for h in &w {
                    post.entry(*h).or_default().push(id);
                }
                leaders.push(w);
                clusters.push(Cluster {
                    lines: vec![m],
                    episodes: vec![ep],
                    newest_ms: at,
                });
            }
        }
    }
    rank(&mut clusters, eps);
    clusters
}

/// Most support first, then the newest.
fn rank(clusters: &mut [Cluster], eps: &[Episode]) {
    clusters.sort_by(|a, b| {
        b.episodes
            .len()
            .cmp(&a.episodes.len())
            .then(b.newest_ms.cmp(&a.newest_ms))
            .then_with(|| key(a, eps).cmp(key(b, eps)))
    });
}

fn key<'a>(c: &Cluster, eps: &'a [Episode]) -> &'a str {
    eps[c.episodes[0]].id.as_str()
}

/// Whether most of the cluster's episodes are not in `seen`.
pub fn fresh(c: &Cluster, eps: &[Episode], seen: &HashSet<String>) -> bool {
    let new = c
        .episodes
        .iter()
        .filter(|&&e| !seen.contains(&eps[e].id))
        .count();
    new * 2 > c.episodes.len()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn words_drop_the_small_and_the_plural() {
        let a = words("Answers wanted, with an opinion");
        let b = words("answer wanted with the OPINION");
        assert_eq!(a, b);
    }

    #[test]
    fn similar_catches_a_restating_and_not_a_neighbour() {
        assert!(similar(
            "wants your opinion when you have one",
            "wants an opinion when you have one, not a menu of options"
        ));
        assert!(!similar(
            "wants your opinion when you have one",
            "keeps the build green before every commit"
        ));
    }
}
