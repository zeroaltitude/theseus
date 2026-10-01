//! The lexical probe (theseus-zaz.11): how hard is retrieval on an exam, at no
//! model cost? BM25 (k1 1.2, b 0.75) over every past node of the exam's store
//! (each node the fixture writer keys: the operator's and the agent's messages
//! and each tool's result, fetched text included, as exam-v1's probe read
//! them), queried with each item's task; then, per family, how much of the
//! gold ranks in the top k. It ports the exam lane's throwaway
//! `bm25probe.py` (34a); a test holds it to that script's ranks on exam-v1.
//!
//! - `Tokenizer::V1` is the script's: lower case; runs of `[a-z0-9]` that may
//!   hold `_ . - /` inside, so `127.0.0.1` and `x86_64-unknown-linux-musl` are
//!   one token each; less a stop list. `Tokenizer::Simple` splits on every
//!   character that is not a letter or a digit, as tantivy's default
//!   tokenizer does (29b's engine): a check that a family's difficulty is not
//!   the tokenizer's.
//! - Ties keep the store's key order, as the script's stable sort did, so a
//!   gold node that shares no word with the task ranks after every node that
//!   scores.
//! - `Rank::Recency` multiplies each score by 2^(−age / half-life), ages taken
//!   from the store's last node: a crude stand-in for a rank that knows time
//!   (not FSRS, not activation), to see whether the time family can tell one
//!   from plain BM25.

use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::fmt::Write as _;
use std::sync::OnceLock;

use anyhow::Result;
use regex::Regex;

use crate::item::{Exam, Family};
use crate::time::parse_local;

/// The script's stop list.
const STOP: &str = "a an the is are was were be to of in on at for and or it its this that what which \
                    when how do does did i we you my our with from by as now not no so if then than \
                    there here into out up down just only give me us your";

/// The cut-offs reported: 6 is the recall budget's item count (design
/// §2.4); 20 is the window `+rerank` reorders (§2.9).
pub const KS: [usize; 5] = [1, 3, 6, 10, 20];

fn stop() -> &'static BTreeSet<&'static str> {
    static S: OnceLock<BTreeSet<&'static str>> = OnceLock::new();
    S.get_or_init(|| STOP.split_whitespace().collect())
}

fn v1_re() -> &'static Regex {
    static R: OnceLock<Regex> = OnceLock::new();
    R.get_or_init(|| Regex::new(r"[a-z0-9][a-z0-9_.\-/]*[a-z0-9]|[a-z0-9]").expect("a valid regex"))
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Tokenizer {
    V1,
    Simple,
}

impl Tokenizer {
    pub fn parse(s: &str) -> Result<Tokenizer> {
        match s {
            "v1" => Ok(Tokenizer::V1),
            "simple" => Ok(Tokenizer::Simple),
            o => anyhow::bail!("unknown tokenizer {o:?}: v1 or simple"),
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Tokenizer::V1 => "v1 (the 34a script's)",
            Tokenizer::Simple => "simple (split on non-alphanumerics, as tantivy's default)",
        }
    }
}

/// A text's terms, in order, with repeats.
pub fn tokens(s: &str, t: Tokenizer) -> Vec<String> {
    let lower = s.to_lowercase();
    let stop = stop();
    match t {
        Tokenizer::V1 => v1_re()
            .find_iter(&lower)
            .map(|m| m.as_str())
            .filter(|w| !stop.contains(w))
            .map(String::from)
            .collect(),
        Tokenizer::Simple => lower
            .split(|c: char| !c.is_alphanumeric())
            .filter(|w| !w.is_empty() && !stop.contains(w))
            .map(String::from)
            .collect(),
    }
}

/// A crude English stemmer, for the families' definitions only (the probe
/// ranks unstemmed, as the script did): plurals, `-ed`, and `-ing`.
pub fn stem(w: &str) -> String {
    let n = w.len();
    let cut = |k: usize| w[..n - k].to_string();
    if n > 4 && w.ends_with("ies") {
        format!("{}y", cut(3))
    } else if n > 5 && w.ends_with("ing") {
        cut(3)
    } else if n > 4
        && ["ed", "ses", "xes", "zes", "ches", "shes"]
            .iter()
            .any(|e| w.ends_with(e))
    {
        cut(2)
    } else if n > 3 && w.ends_with('s') && !w.ends_with("ss") {
        cut(1)
    } else {
        w.to_string()
    }
}

/// A text's content words, stemmed, under both tokenizers (a word either
/// tokenizer would match counts), less single characters.
pub fn content_stems(s: &str) -> BTreeSet<String> {
    tokens(s, Tokenizer::V1)
        .into_iter()
        .chain(tokens(s, Tokenizer::Simple))
        .filter(|w| w.chars().count() > 1)
        .map(|w| stem(&w))
        .collect()
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Rank {
    Bm25,
    /// BM25 × 2^(−age / half-life).
    Recency {
        half_life_days: f64,
    },
}

impl Rank {
    pub fn describe(self) -> String {
        match self {
            Rank::Bm25 => "BM25".into(),
            Rank::Recency { half_life_days } => {
                format!("BM25 × 2^(−age / {half_life_days} days), ages from the store's last node")
            }
        }
    }
}

struct Doc {
    key: String,
    owner: String,
    at_ms: u64,
    tf: HashMap<String, u32>,
    len: usize,
}

/// Every keyed node of an exam's store, in the manifest's key order.
pub struct Corpus {
    docs: Vec<Doc>,
    df: HashMap<String, usize>,
    avgdl: f64,
    last_ms: u64,
    pub sessions: usize,
}

impl Corpus {
    pub fn of(exam: &Exam, tok: Tokenizer) -> Result<Corpus> {
        let off = exam.file.utc_offset_min;
        let mut by_key: BTreeMap<String, Doc> = BTreeMap::new();
        let mut sessions = 0;
        for (owner, s) in exam.pasts() {
            sessions += 1;
            for (k, n) in s.nodes.iter().enumerate() {
                let key = format!("{owner}/{}.{}", s.key, k + 1);
                let terms = tokens(&n.text, tok);
                let mut tf = HashMap::new();
                for t in &terms {
                    *tf.entry(t.clone()).or_insert(0) += 1;
                }
                by_key.insert(
                    key.clone(),
                    Doc {
                        key,
                        owner: owner.to_string(),
                        at_ms: parse_local(&n.at, off)?,
                        tf,
                        len: terms.len(),
                    },
                );
            }
        }
        let docs: Vec<Doc> = by_key.into_values().collect();
        let mut df = HashMap::new();
        for d in &docs {
            for t in d.tf.keys() {
                *df.entry(t.clone()).or_insert(0) += 1;
            }
        }
        let total: usize = docs.iter().map(|d| d.len).sum();
        let avgdl = total as f64 / docs.len().max(1) as f64;
        let last_ms = docs.iter().map(|d| d.at_ms).max().unwrap_or(0);
        Ok(Corpus {
            docs,
            df,
            avgdl,
            last_ms,
            sessions,
        })
    }

    pub fn len(&self) -> usize {
        self.docs.len()
    }

    pub fn is_empty(&self) -> bool {
        self.docs.is_empty()
    }

    /// The script's BM25, term for term (its constants written as it wrote
    /// them, so the floating point agrees).
    fn bm25(&self, query: &[String], d: &Doc) -> f64 {
        let n = self.docs.len() as f64;
        let mut s = 0.0;
        for t in query {
            let Some(&tf) = d.tf.get(t) else {
                continue;
            };
            let df = self.df[t] as f64;
            let idf = (1.0 + (n - df + 0.5) / (df + 0.5)).ln();
            let tf = f64::from(tf);
            s += idf * tf * 2.2 / (tf + 1.2 * (0.25 + 0.75 * d.len as f64 / self.avgdl));
        }
        s
    }

    /// Every node's index, best first; ties in key order.
    pub fn ranking(&self, text: &str, tok: Tokenizer, rank: Rank) -> Vec<usize> {
        let mut q: Vec<String> = Vec::new();
        for t in tokens(text, tok) {
            if !q.contains(&t) {
                q.push(t);
            }
        }
        let scores: Vec<f64> = self
            .docs
            .iter()
            .map(|d| {
                let s = self.bm25(&q, d);
                match rank {
                    Rank::Bm25 => s,
                    Rank::Recency { half_life_days } => {
                        let age_days = (self.last_ms - d.at_ms) as f64 / 86_400_000.0;
                        s * (-age_days / half_life_days).exp2()
                    }
                }
            })
            .collect();
        let mut idx: Vec<usize> = (0..self.docs.len()).collect();
        idx.sort_by(|&a, &b| scores[b].total_cmp(&scores[a]));
        idx
    }
}

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

/// Every item with gold, probed.
pub fn probe(exam: &Exam, tok: Tokenizer, rank: Rank) -> Result<(Corpus, Vec<ItemProbe>)> {
    let c = Corpus::of(exam, tok)?;
    let mut out = Vec::new();
    for item in exam.file.items.iter().filter(|i| !i.gold.is_empty()) {
        let order = c.ranking(&item.task, tok, rank);
        let mut pos = vec![0usize; c.docs.len()];
        for (r, &i) in order.iter().enumerate() {
            pos[i] = r + 1;
        }
        let rank_of: HashMap<&str, usize> = c
            .docs
            .iter()
            .enumerate()
            .map(|(i, d)| (d.key.as_str(), pos[i]))
            .collect();
        let gold_keys: Vec<String> = item
            .gold
            .iter()
            .map(|g| format!("{}/{g}", item.id))
            .collect();
        let gold_ranks = gold_keys.iter().map(|k| rank_of[k.as_str()]).collect();
        let trap = c
            .docs
            .iter()
            .filter(|d| d.owner == item.id && !gold_keys.contains(&d.key))
            .map(|d| (d.key.clone(), rank_of[d.key.as_str()]))
            .min_by_key(|(_, r)| *r)
            .map(|(k, r)| {
                (
                    k.split_once('/').map_or(k.clone(), |(_, s)| s.to_string()),
                    r,
                )
            });
        out.push(ItemProbe {
            id: item.id.clone(),
            family: item.family,
            held_out: item.held_out,
            gold_ranks,
            trap,
        });
    }
    Ok((c, out))
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

/// The rows a probe's tables show: each family with items, exam-v1's
/// families together, the hard ones together, and all.
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
        out.push(("exam-v1's families".into(), v1));
        out.push(("the hard families".into(), hard));
    }
    out.push(("**all**".into(), recall(ps)));
    out
}

fn frac(a: usize, n: usize) -> String {
    if n == 0 {
        "–".into()
    } else {
        format!("{a}/{n} ({:.0}%)", 100.0 * a as f64 / n as f64)
    }
}

/// The probe of one exam, as Markdown: its corpus, then items with all their
/// gold in the top k and gold nodes in the top k, per family; then, with
/// `per_item`, each item's gold ranks and its best own non-gold node.
pub fn render(
    exam: &Exam,
    c: &Corpus,
    ps: &[ItemProbe],
    tok: Tokenizer,
    rank: Rank,
    per_item: bool,
) -> String {
    let mut o = String::new();
    let _ = writeln!(
        o,
        "- Exam: {} ({}): {} items with gold; corpus {} nodes in {} sessions.",
        exam.file.version,
        exam.digest,
        ps.len(),
        c.len(),
        c.sessions
    );
    let _ = writeln!(
        o,
        "- Rank: {}; tokenizer {}.",
        rank.describe(),
        tok.as_str()
    );
    let _ = writeln!(o);
    let ks: Vec<String> = KS.iter().map(|k| format!("@{k}")).collect();
    let head = format!("| | items | gold | {} |", ks.join(" | "));
    let rule = format!("|---|---|---|{}", "---|".repeat(KS.len()));
    let rows = rows(ps);
    let _ = writeln!(o, "Items with all their gold in the top k:");
    let _ = writeln!(o);
    let _ = writeln!(o, "{head}");
    let _ = writeln!(o, "{rule}");
    for (name, r) in &rows {
        let cells: Vec<String> = r.items_all.iter().map(|&a| frac(a, r.items)).collect();
        let _ = writeln!(
            o,
            "| {name} | {} | {} | {} |",
            r.items,
            r.gold,
            cells.join(" | ")
        );
    }
    let _ = writeln!(o);
    let _ = writeln!(o, "Gold nodes in the top k:");
    let _ = writeln!(o);
    let _ = writeln!(o, "{head}");
    let _ = writeln!(o, "{rule}");
    for (name, r) in &rows {
        let cells: Vec<String> = r.nodes.iter().map(|&a| frac(a, r.gold)).collect();
        let _ = writeln!(
            o,
            "| {name} | {} | {} | {} |",
            r.items,
            r.gold,
            cells.join(" | ")
        );
    }
    if per_item {
        let _ = writeln!(o);
        let _ = writeln!(
            o,
            "| item | family | half | gold rank(s) | best own non-gold node, rank |"
        );
        let _ = writeln!(o, "|---|---|---|---|---|");
        for p in ps {
            let gr: Vec<String> = p.gold_ranks.iter().map(usize::to_string).collect();
            let _ = writeln!(
                o,
                "| {} | {} | {} | {} | {} |",
                p.id,
                p.family.as_str(),
                if p.held_out { "held out" } else { "tuning" },
                gr.join(", "),
                p.trap
                    .as_ref()
                    .map_or("–".to_string(), |(k, r)| format!("{k} @{r}"))
            );
        }
    }
    o
}

/// Two probes side by side (a base exam, then a new one): items with all
/// their gold in the top k, per row, as `base → new`.
pub fn compare(base: (&str, &[ItemProbe]), new: (&str, &[ItemProbe])) -> String {
    let mut o = String::new();
    let (a, b) = (rows(base.1), rows(new.1));
    let ks: Vec<String> = KS.iter().map(|k| format!("@{k}")).collect();
    let _ = writeln!(
        o,
        "Items with all their gold in BM25's top k, {} → {}:",
        base.0, new.0
    );
    let _ = writeln!(o);
    let _ = writeln!(o, "| | items | {} |", ks.join(" | "));
    let _ = writeln!(o, "|---|---|{}", "---|".repeat(KS.len()));
    let pct = |x: usize, n: usize| -> String {
        if n == 0 {
            "–".into()
        } else {
            format!("{:.0}%", 100.0 * x as f64 / n as f64)
        }
    };
    for (name, rb) in &b {
        let ra = a.iter().find(|(n, _)| n == name).map(|(_, r)| r.clone());
        let items = match &ra {
            Some(ra) => format!("{} → {}", ra.items, rb.items),
            None => format!("– → {}", rb.items),
        };
        let cells: Vec<String> = (0..KS.len())
            .map(|j| match &ra {
                Some(ra) => format!(
                    "{} → {}",
                    pct(ra.items_all[j], ra.items),
                    pct(rb.items_all[j], rb.items)
                ),
                None => format!("– → {}", pct(rb.items_all[j], rb.items)),
            })
            .collect();
        let _ = writeln!(o, "| {name} | {items} | {} |", cells.join(" | "));
    }
    o
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::item::{EXAM_V1, EXAM_V2};

    #[test]
    fn the_v1_tokenizer_keeps_dotted_and_dashed_runs_and_drops_stop_words() {
        assert_eq!(
            tokens(
                "Health's link reads 127.0.0.1:7433, for x86_64-unknown-linux-musl.",
                Tokenizer::V1
            ),
            [
                "health",
                "s",
                "link",
                "reads",
                "127.0.0.1",
                "7433",
                "x86_64-unknown-linux-musl"
            ]
        );
        assert_eq!(
            tokens("Copy ~/.theseus/store, not bindings.toml.", Tokenizer::V1),
            ["copy", "theseus/store", "bindings.toml"]
        );
        assert_eq!(
            tokens(
                "Copy ~/.theseus/store, not bindings.toml.",
                Tokenizer::Simple
            ),
            ["copy", "theseus", "store", "bindings", "toml"]
        );
        assert_eq!(stem("jobs"), "job");
        assert_eq!(stem("batches"), "batch");
        assert_eq!(stem("handles"), "handle");
        assert_eq!(stem("pruned"), "prun");
        assert_eq!(stem("running"), "runn");
        assert_eq!(stem("replies"), "reply");
        assert_eq!(stem("class"), "class");
        assert_eq!(stem("bus"), "bus");
    }

    /// BM25 by hand. Three nodes; the query "port ui".
    ///
    /// - n1 "ui port 7433": 3 terms; n2 "port port": 2; n3 "redb": 1.
    ///   avgdl = 6/3 = 2.
    /// - df(port) = 2: idf = ln(1 + (3 − 2 + 0.5)/(2 + 0.5)) = ln 1.6
    ///   = 0.4700036292. df(ui) = 1: idf = ln(1 + 2.5/1.5) = ln(8/3)
    ///   = 0.9808292530.
    /// - n1: each term once, length 3, so the denominator is
    ///   1 + 1.2 × (0.25 + 0.75 × 1.5) = 2.65, and each term's share is
    ///   idf × 2.2/2.65 = idf × 0.8301886792: 0.3901916922 + 0.8142733421
    ///   = 1.2044650343.
    /// - n2: port twice, length 2: 0.4700036292 × 2 × 2.2/(2 + 1.2)
    ///   = 0.6462549902.
    /// - n3: 0.
    ///
    /// So n1 ranks first; with a recency half-life of seconds, the newer n2
    /// does.
    #[test]
    fn bm25_matches_a_hand_computation() {
        let src = r#"
version = "t"
utc_offset_min = -420
[[item]]
id = "x"
family = "fact"
task = "port ui"
gold = ["a.2"]
check = 'reply has "x"'
[[item.session]]
key = "a"
place = "cli"
[[item.session.node]]
at = "2026-09-01 10:00"
who = "eddie"
text = "ui port 7433"
[[item.session.node]]
at = "2026-09-01 10:01"
who = "theseus"
text = "port port"
[[item.session.node]]
at = "2026-09-01 10:02"
who = "theseus"
text = "redb"
"#;
        let exam = Exam::parse(src).unwrap();
        let c = Corpus::of(&exam, Tokenizer::V1).unwrap();
        let q: Vec<String> = vec!["port".into(), "ui".into()];
        let s: Vec<f64> = c.docs.iter().map(|d| c.bm25(&q, d)).collect();
        assert!((s[0] - 1.204_465_034_3).abs() < 1e-9, "{}", s[0]);
        assert!((s[1] - 0.646_254_990_2).abs() < 1e-9, "{}", s[1]);
        assert_eq!(s[2], 0.0);
        let (_, ps) = probe(&exam, Tokenizer::V1, Rank::Bm25).unwrap();
        assert_eq!(ps[0].gold_ranks, [2]);
        assert_eq!(ps[0].trap, Some(("a.1".into(), 1)));
        assert_eq!(c.ranking("port ui", Tokenizer::V1, Rank::Bm25), [0, 1, 2]);
        // A recency rank with a short half-life puts the newer node first.
        let recent = Rank::Recency {
            half_life_days: 0.0001,
        };
        assert_eq!(c.ranking("port ui", Tokenizer::V1, recent), [1, 0, 2]);
    }

    /// The port reproduces the 34a script's table on exam-v1, rank for rank
    /// (`~/reports/theseus-lane-exam/runs/bm25probe.md`, run on exam-v1.1;
    /// v1.2 renamed one project, a token for a token of the same counts).
    #[test]
    fn the_port_reproduces_the_34a_probe_on_exam_v1() {
        let exam = Exam::parse(EXAM_V1).unwrap();
        let (c, ps) = probe(&exam, Tokenizer::V1, Rank::Bm25).unwrap();
        assert_eq!(c.len(), 102);
        let want: [(&str, &[usize], (&str, usize)); 36] = [
            ("fact-1", &[1], ("a.2", 2)),
            ("fact-2", &[2], ("a.2", 1)),
            ("fact-3", &[2], ("a.1", 1)),
            ("fact-4", &[1], ("a.2", 51)),
            ("preference-1", &[1], ("a.2", 59)),
            ("preference-2", &[2], ("a.2", 4)),
            ("preference-3", &[3], ("a.2", 66)),
            ("preference-4", &[2], ("a.2", 62)),
            ("decision-1", &[11], ("a.2", 1)),
            ("decision-2", &[8], ("a.2", 9)),
            ("decision-3", &[1], ("a.2", 18)),
            ("decision-4", &[5], ("a.2", 1)),
            ("procedure-1", &[1], ("a.2", 84)),
            ("procedure-2", &[1], ("a.2", 2)),
            ("procedure-3", &[1], ("a.2", 84)),
            ("procedure-4", &[1], ("a.2", 87)),
            ("episode-1", &[1, 4], ("a.1", 2)),
            ("episode-2", &[2], ("a.1", 1)),
            ("episode-3", &[2], ("a.1", 1)),
            ("episode-4", &[42, 1], ("a.1", 41)),
            ("superseded-1", &[1, 2], ("a.2", 89)),
            ("superseded-2", &[1, 2], ("a.2", 94)),
            ("superseded-3", &[1, 3], ("a.2", 97)),
            ("superseded-4", &[1, 2], ("a.2", 4)),
            ("private-1", &[1], ("a.1", 2)),
            ("private-2", &[1], ("a.1", 68)),
            ("private-3", &[1], ("a.1", 5)),
            ("private-4", &[1], ("a.1", 2)),
            ("injection-1", &[3], ("a.1", 1)),
            ("injection-2", &[2], ("a.1", 1)),
            ("injection-3", &[4], ("a.1", 1)),
            ("injection-4", &[3], ("a.1", 4)),
            ("distractor-1", &[1], ("a.1", 2)),
            ("distractor-2", &[1], ("a.1", 40)),
            ("distractor-3", &[1], ("a.1", 42)),
            ("distractor-4", &[2], ("b.2", 1)),
        ];
        assert_eq!(ps.len(), want.len());
        for (p, (id, gold, (tk, tr))) in ps.iter().zip(want) {
            assert_eq!(p.id, id);
            assert_eq!(p.gold_ranks, gold, "{id}");
            assert_eq!(p.trap, Some((tk.to_string(), tr)), "{id}");
        }
        // The script's summary lines.
        let r = recall(&ps);
        assert_eq!(
            (r.gold, r.nodes[0], r.nodes[1], r.nodes[2]),
            (42, 21, 36, 39)
        );
        assert_eq!(
            (r.items, r.items_all[0], r.items_all[1], r.items_all[2]),
            (36, 15, 30, 33)
        );
    }

    /// exam-v2's hard families stay hard for lexical retrieval, under either
    /// tokenizer: BM25's top 6 holds all the gold for at most 4 of the 32
    /// (exam-v2 as written: 1 with v1's tokenizer, 3 with the simple one), and
    /// a paraphrase's gold at no k. An edit that made them easy fails here.
    #[test]
    fn the_hard_families_stay_hard_for_bm25() {
        let exam = Exam::parse(EXAM_V2).unwrap();
        for tok in [Tokenizer::V1, Tokenizer::Simple] {
            let (_, ps) = probe(&exam, tok, Rank::Bm25).unwrap();
            let hard = recall(ps.iter().filter(|p| p.family.is_hard()));
            assert_eq!(hard.items, 32);
            assert!(hard.items_all[2] <= 4, "{tok:?}: {hard:?}");
            let para = recall(ps.iter().filter(|p| p.family == Family::Paraphrase));
            assert_eq!(para.items_all[4], 0, "{tok:?}: {para:?}");
        }
    }

    /// The probe runs on exam-v2, and its tables name every family.
    #[test]
    fn the_probe_renders_exam_v2() {
        let exam = Exam::parse(EXAM_V2).unwrap();
        let (c, ps) = probe(&exam, Tokenizer::V1, Rank::Bm25).unwrap();
        assert_eq!(ps.len(), 68);
        assert!(c.len() > 1000, "{} nodes", c.len());
        let md = render(&exam, &c, &ps, Tokenizer::V1, Rank::Bm25, true);
        for f in Family::ALL.iter().filter(|f| **f != Family::NeedsNothing) {
            assert!(md.contains(&format!("| {} | ", f.as_str())), "{f:?}");
        }
        assert!(md.contains("| the hard families |") && md.contains("| **all** | 68 |"));
        let v1 = Exam::parse(EXAM_V1).unwrap();
        let (_, p1) = probe(&v1, Tokenizer::V1, Rank::Bm25).unwrap();
        let cmp = compare(("exam-v1.2", &p1), ("exam-v2", &ps));
        assert!(
            cmp.contains("| paraphrase | – → 8 |") && cmp.contains("| fact | 4 → 4 |"),
            "{cmp}"
        );
    }
}
