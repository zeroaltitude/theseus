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
//! - [`entry`]: a leading heading set aside before the checks (a Markdown
//!   heading, a wholly bold line, or a short uncited title its next
//!   sentence restates), so a model's habit of titling an entry costs
//!   nothing, and a sentence that says something is never dropped.
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
/// A heading set aside: at most this many words when it is marked (`#`, or
/// wholly bold), and when it is a plain title its next sentence restates.
pub const HEADING_WORDS: usize = 8;

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

/// A synthesis's text, its leading heading set aside: what the checks read,
/// and what is kept.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Entry<'a> {
    /// The heading set aside, as written (its marks and stop kept).
    pub heading: Option<&'a str>,
    /// The rest, trimmed.
    pub text: &'a str,
}

/// `text` with its leading heading set aside, if it has one. Only its first
/// line, or its first sentence, can be one, and a heading cites nothing and
/// has at most [`HEADING_WORDS`] words:
///
/// - a first line that is a Markdown heading (`# …`) or wholly bold
///   (`**…**`, `__…__`): set aside by its form, even with nothing after it
///   (the entry is then empty, and [`check`] says [`Fault::Empty`]);
/// - a plain first line ("Kestrel relay", on a line of its own) or first
///   sentence ("Kestrel relay." on the entry's line), whose every word
///   appears in the sentence after it: a title restates its subject, and a
///   sentence that says something new is never set aside.
///
/// Anything else is the entry whole.
pub fn entry(text: &str) -> Entry<'_> {
    let text = text.trim();
    let whole = Entry {
        heading: None,
        text,
    };
    // The first line, when the text has more than one.
    if let Some((first, rest)) = text.split_once('\n') {
        let (first, rest) = (first.trim(), rest.trim());
        if let Some(inner) = marked(first) {
            if !inner.contains('[') && words(inner).len() <= HEADING_WORDS {
                return Entry {
                    heading: Some(first),
                    text: rest,
                };
            }
        } else if restated(first, rest) {
            return Entry {
                heading: Some(first),
                text: rest,
            };
        }
    } else if let Some(inner) = marked(text) {
        // The text is only a heading.
        if !inner.contains('[') && words(inner).len() <= HEADING_WORDS {
            return Entry {
                heading: Some(text),
                text: "",
            };
        }
    }
    // The first sentence, on the entry's own line: it ends at the first stop
    // followed by a space, before any citation.
    let stop = text.char_indices().find_map(|(i, c)| {
        let next = text[i + c.len_utf8()..].chars().next();
        (matches!(c, '.' | '!' | '?') && next.is_some_and(char::is_whitespace))
            .then_some(i + c.len_utf8())
    });
    if let Some(end) = stop {
        let (first, rest) = (text[..end].trim(), text[end..].trim());
        if restated(first, rest) {
            return Entry {
                heading: Some(first),
                text: rest,
            };
        }
    }
    whole
}

/// A Markdown heading's or a wholly bold line's words, without the marks.
fn marked(line: &str) -> Option<&str> {
    if let Some(h) = line.strip_prefix('#') {
        let h = h.trim_start_matches('#');
        return h.starts_with(' ').then(|| h.trim());
    }
    for m in ["**", "__"] {
        if let Some(inner) = line.strip_prefix(m).and_then(|l| l.strip_suffix(m)) {
            let inner = inner.trim();
            return (!inner.is_empty() && !inner.contains(m)).then_some(inner);
        }
    }
    None
}

/// Whether `first` is a plain title of what follows: it cites nothing, has
/// at most [`HEADING_WORDS`] words, and each of them appears in `rest`'s
/// first sentence.
fn restated(first: &str, rest: &str) -> bool {
    if first.contains('[') || rest.is_empty() {
        return false;
    }
    let title = words(first);
    if title.is_empty() || title.len() > HEADING_WORDS {
        return false;
    }
    let Some(next) = sentences(rest).into_iter().next() else {
        return false;
    };
    let next: BTreeSet<String> = words(&next.text).into_iter().collect();
    title.iter().all(|w| next.contains(w))
}

/// A line's words, lower case, without the punctuation at their ends.
fn words(s: &str) -> Vec<String> {
    s.split_whitespace()
        .map(|w| {
            w.trim_matches(|c: char| !c.is_alphanumeric())
                .to_lowercase()
        })
        .filter(|w| !w.is_empty())
        .collect()
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
/// [`MAX_WORDS`] words. The sentences, when it passes. The core checks an
/// [`entry`]'s text, its heading set aside.
pub fn check(text: &str, sources: usize) -> Result<Vec<Sentence>, Fault> {
    // A marked first line (a Markdown heading, a bold line) is a sentence of
    // its own, never glued to the next: one [`entry`] did not set aside is
    // checked as what it is.
    let s = match text.trim().split_once('\n') {
        Some((first, rest)) if marked(first.trim()).is_some() => {
            let mut s = sentences(first);
            s.extend(sentences(rest));
            s
        }
        _ => sentences(text),
    };
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

    const BODY: &str = "The Kestrel relay listens on port 7714 [1]. It logs to relay.log [2].";

    /// The entry's text checked, as the core checks it.
    fn checked(text: &str) -> Result<Vec<Sentence>, Fault> {
        check(entry(text).text, 3)
    }

    /// Each form of a leading heading is set aside, and the entry passes:
    /// the title with a stop on the entry's line (the live one), on a line
    /// of its own with or without a stop, a Markdown heading, and a wholly
    /// bold line; the words counted are the entry's.
    #[test]
    fn a_leading_heading_is_set_aside() {
        for (text, heading) in [
            (format!("Kestrel relay. {BODY}"), "Kestrel relay."),
            (format!("Kestrel relay.\n{BODY}"), "Kestrel relay."),
            (format!("Kestrel relay\n{BODY}"), "Kestrel relay"),
            (format!("Kestrel relay:\n\n{BODY}"), "Kestrel relay:"),
            (format!("# Kestrel relay\n{BODY}"), "# Kestrel relay"),
            (
                format!("## The relay's logs\n\n{BODY}"),
                "## The relay's logs",
            ),
            (format!("**Kestrel relay**\n{BODY}"), "**Kestrel relay**"),
            (format!("__Kestrel relay__\n{BODY}"), "__Kestrel relay__"),
        ] {
            let e = entry(&text);
            assert_eq!(e.heading, Some(heading), "{text:?}");
            assert_eq!(e.text, BODY, "{text:?}");
            let s = checked(&text).unwrap_or_else(|f| panic!("{text:?}: {f}"));
            assert_eq!(s.len(), 2);
            assert_eq!(s[0].text, "The Kestrel relay listens on port 7714.");
        }
        // An entry with no heading is the entry whole.
        assert_eq!(
            entry(BODY),
            Entry {
                heading: None,
                text: BODY
            }
        );
        // The words counted are the entry's: 120 of them pass under a title.
        let full = format!("# Kestrel relay\n{} [1].", "word ".repeat(120));
        assert!(checked(&full).is_ok());
        // A text that is only a marked heading is empty.
        assert_eq!(checked("# Kestrel relay"), Err(Fault::Empty));
        assert_eq!(checked("**Kestrel relay**\n"), Err(Fault::Empty));
    }

    /// What is not a heading keeps today's fault: a short first sentence
    /// that says something new, a long uncited first sentence, a title
    /// whose words the next sentence leaves out, a title that cites, an
    /// uncited sentence past the first, a too-long marked line, and a
    /// heading-like line that is not first.
    #[test]
    fn what_is_not_a_heading_is_still_rejected() {
        for (text, fault) in [
            // Short and uncited, but it says something new.
            (format!("The relay is fast. {BODY}"), Fault::Uncited(1)),
            (format!("It is retired.\n{BODY}"), Fault::Uncited(1)),
            // Every word restated, but more than a title's words.
            (
                "The Kestrel relay listens on TCP port 7714 now. \
                 The Kestrel relay listens on TCP port 7714 now [1]."
                    .to_string(),
                Fault::Uncited(1),
            ),
            // A title of words the next sentence leaves out.
            (format!("Kestrel gateway. {BODY}"), Fault::Uncited(1)),
            // An uncited sentence past the first.
            (
                format!("Kestrel relay. {BODY} It is fine."),
                Fault::Uncited(3),
            ),
            (
                "The relay listens on 7714 [1]. Kestrel relay. It logs [2].".to_string(),
                Fault::Uncited(2),
            ),
            // A marked line past a title's words is not set aside.
            (
                format!("# The Kestrel relay was moved to a new host in May\n{BODY}"),
                Fault::Uncited(1),
            ),
            // A heading that is not first.
            (
                "The relay listens on 7714 [1].\n# Logs\nIt logs to relay.log [2]. Logs end."
                    .to_string(),
                Fault::Uncited(3),
            ),
            // A heading set aside leaves the rest to the checks.
            (
                format!("# Kestrel relay\n{BODY} It restarts [4]."),
                Fault::Unknown {
                    sentence: 3,
                    cited: 4,
                },
            ),
        ] {
            assert_eq!(checked(&text), Err(fault), "{text:?}");
        }
        // A title that cites is not set aside: it is part of its sentence.
        let cited = format!("Kestrel relay [1]. {BODY}");
        assert_eq!(entry(&cited).heading, None);
        assert!(checked(&cited).is_ok());
        // A plain title alone, with nothing to restate it, is a sentence.
        assert_eq!(checked("Kestrel relay."), Err(Fault::Uncited(1)));
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
