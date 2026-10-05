//! The replay (instrument 2, design §2.9; step 34b): every arm recomputed
//! over the recorded turns of a copy of a store, each as of its own turn, and
//! scored against silver labels read deterministically from the record.
//!
//! - **The turns** are those with a recall row (`recall.shadow`, or
//!   `recall.ran`). Each turn's query is recomputed from its transcript as
//!   the core builds it (`theseus_core::recall::query_of`), and kept only
//!   when its digest equals the row's (`query_digest`): a turn whose query
//!   cannot be rebuilt is counted and left out.
//! - **As of the turn.** Each arm asks the index with the turn's `as_of`, and
//!   the replay never trusts it to: a hit at or after `as_of` is dropped and
//!   counted as a leak, so no later node answers (the leakage test).
//! - **Each arm** asks the index for its sources (`bm25`: BM25 and entities;
//!   `baseline`: with vectors), and its hits go through the real pipeline
//!   (`theseus_memory::recall::recall`, `baseline`'s science, §2.4's pack):
//!   the turn's own session is in context, the nodes the operator labeled
//!   `wrong` or `stale` before the turn are out. `none` admits nothing. The
//!   place rule is not applied: a copy of a store has no outbox to read
//!   places from, so every node is the turn's place's.
//! - **The silver labels** (§2.9's table), each a set of nodes of other
//!   sessions written before the turn: *re-supply*, an operator message
//!   sharing an 8-word run with the turn's (no cosine: the replay runs no
//!   model); *reference*, the node where an identifier the turn names (an
//!   issue id, a hash, a path) was first seen, when the turn's own session
//!   had not seen it; *re-derivation*, the result of an older call with the
//!   same tool and input as a call in the turn; and *should-have*, a node
//!   the operator labeled `should_have` on the turn's recall.
//! - **The metrics**, per arm and label: recall and precision of the pack
//!   (k is what it admitted, at most `recall_max_items`), MRR within it, over
//!   the turns with that label; and the stale rate, the share of admitted
//!   nodes the operator labeled `wrong` or `stale` after the turn (the record
//!   holds no supersession, so "already stale at the turn" cannot be read).

use std::collections::{BTreeMap, BTreeSet};
use std::fmt::Write as _;

use anyhow::Result;
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use theseus_core::ledger::LedgerRow;
use theseus_core::node::Body;
use theseus_core::recall::{query_of, text_of};
use theseus_core::session::SessionRecord;
use theseus_core::store::Store;
use theseus_memory::recall::{self as pipeline, Asker, Candidate, Place};
use theseus_memory::Baseline;
use theseus_store::kinds;

/// The hits each recall asks for (`theseus_core::recall::K`).
pub const K: usize = 40;
/// Words in a re-supply's shared run.
pub const RUN_WORDS: usize = 8;

/// One node of the store, as the replay reads it.
#[derive(Debug, Clone)]
pub struct CNode {
    pub node_id: String,
    pub session_id: String,
    pub position: u64,
    pub kind: &'static str,
    pub text: String,
    /// A call's `<tool> <input>`, to match a re-derivation by.
    pub call: Option<String>,
    /// A call's or a result's tool use id.
    pub tool_use_id: Option<String>,
}

/// A recorded turn with a recall row.
#[derive(Debug, Clone)]
pub struct Turn {
    pub session_id: String,
    pub turn_id: String,
    pub recall_id: String,
    pub as_of: u64,
    pub query: String,
    /// The turn's operator text.
    pub said: String,
    /// The turn's calls, as `<tool> <input>`.
    pub calls: Vec<String>,
}

/// An operator's label row.
#[derive(Debug, Clone)]
pub struct Label {
    pub node_id: String,
    pub label: String,
    pub recall_id: Option<String>,
    pub position: u64,
}

/// What the replay reads from a store.
#[derive(Debug, Default)]
pub struct Record {
    pub nodes: Vec<CNode>,
    pub turns: Vec<Turn>,
    pub labels: Vec<Label>,
    /// Recall rows whose query could not be rebuilt, with why.
    pub skipped: Vec<String>,
}

fn kind_of(b: &Body) -> &'static str {
    match b {
        Body::UserMessage { .. } => "user_message",
        Body::AssistantMessage { .. } => "assistant_message",
        Body::ToolCall { .. } => "tool_call",
        Body::ToolResult { .. } => "tool_result",
        Body::Recall { .. } => "recall",
        Body::Arrangement { .. } => "arrangement",
        Body::Summary { .. } => "summary",
        Body::Synthesis { .. } => "synthesis",
    }
}

fn digest(q: &str) -> String {
    Sha256::digest(q.as_bytes())[..8]
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}

/// Read every node, every turn with a recall row, and every label.
pub fn read(store: &Store) -> Result<Record> {
    let mut out = Record::default();
    for s in store.list_sessions::<SessionRecord>()? {
        let sid = s.session_id;
        let transcript = store.transcript(&sid)?;
        for (p, n) in &transcript {
            let (call, tuid) = match &n.body {
                Body::ToolCall {
                    tool,
                    input,
                    tool_use_id,
                    ..
                } => (Some(format!("{tool} {input}")), Some(tool_use_id.clone())),
                Body::ToolResult { tool_use_id, .. } => (None, Some(tool_use_id.clone())),
                _ => (None, None),
            };
            out.nodes.push(CNode {
                node_id: n.id.clone(),
                session_id: sid.clone(),
                position: *p,
                kind: kind_of(&n.body),
                text: text_of(n),
                call,
                tool_use_id: tuid,
            });
        }
        for r in store.scope_after(&theseus_core::fact::recall::scope(&sid), 0)? {
            if r.kind != kinds::LEDGER {
                continue;
            }
            let row: LedgerRow = r.decode()?;
            if row.kind != "recall.shadow" && row.kind != "recall.ran" {
                continue;
            }
            let Some(turn_id) = row.turn_id.clone() else {
                continue;
            };
            let d = &row.data;
            let why = match query_of(&transcript, &turn_id) {
                None => Some("the turn brings nothing new".to_string()),
                Some((q, _)) if Some(digest(&q).as_str()) != d["query_digest"].as_str() => {
                    Some("its query's digest differs from the row's".into())
                }
                Some((_, a)) if Some(a) != d["as_of"].as_u64() => {
                    Some("its as_of differs from the row's".into())
                }
                Some((query, as_of)) => {
                    let mine = transcript
                        .iter()
                        .filter(|(_, n)| n.turn_id.as_deref() == Some(turn_id.as_str()));
                    let mut said = Vec::new();
                    let mut calls = Vec::new();
                    for (_, n) in mine {
                        match &n.body {
                            Body::UserMessage { text, .. } => said.push(text.clone()),
                            Body::ToolCall { tool, input, .. } => {
                                calls.push(format!("{tool} {input}"))
                            }
                            _ => {}
                        }
                    }
                    out.turns.push(Turn {
                        session_id: sid.clone(),
                        turn_id: turn_id.clone(),
                        recall_id: d["recall_id"].as_str().unwrap_or("").into(),
                        as_of,
                        query,
                        said: said.join("\n"),
                        calls,
                    });
                    None
                }
            };
            if let Some(why) = why {
                out.skipped.push(format!("{sid} {turn_id}: {why}"));
            }
        }
    }
    for r in store.scope_after(theseus_core::recall::labels::SCOPE, 0)? {
        if r.kind != kinds::LEDGER {
            continue;
        }
        let row: LedgerRow = r.decode()?;
        if row.kind != "memory.label" {
            continue;
        }
        out.labels.push(Label {
            node_id: row.data["node_id"].as_str().unwrap_or("").into(),
            label: row.data["label"].as_str().unwrap_or("").into(),
            recall_id: row.data["recall_id"].as_str().map(str::to_string),
            position: r.position,
        });
    }
    out.nodes.sort_by_key(|n| n.position);
    Ok(out)
}

/// The silver labels.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Silver {
    ReSupply,
    Reference,
    ReDerivation,
    ShouldHave,
}

impl Silver {
    pub const ALL: [Silver; 4] = [
        Silver::ReSupply,
        Silver::Reference,
        Silver::ReDerivation,
        Silver::ShouldHave,
    ];

    pub fn as_str(self) -> &'static str {
        match self {
            Silver::ReSupply => "re-supply",
            Silver::Reference => "reference",
            Silver::ReDerivation => "re-derivation",
            Silver::ShouldHave => "should-have",
        }
    }
}

fn words(t: &str) -> Vec<String> {
    t.split(|c: char| !c.is_alphanumeric())
        .filter(|w| !w.is_empty())
        .map(str::to_lowercase)
        .collect()
}

fn runs(t: &str) -> BTreeSet<Vec<String>> {
    words(t)
        .windows(RUN_WORDS)
        .map(<[String]>::to_vec)
        .collect()
}

/// The identifiers a text names: issue ids (`theseus-ab12`), hashes (7 to
/// 64 hex digits, with a digit and a letter), and paths (two segments or
/// more).
pub fn identifiers(t: &str) -> BTreeSet<String> {
    let mut out = BTreeSet::new();
    for raw in t.split(|c: char| c.is_whitespace() || "\"'`(),;<>[]{}".contains(c)) {
        let w = raw.trim_end_matches(['.', ':', '!', '?']);
        let issue = w.split_once('-').is_some_and(|(a, b)| {
            a.len() >= 3
                && a.chars().all(|c| c.is_ascii_lowercase())
                && b.len() >= 3
                && b.chars().all(|c| c.is_ascii_alphanumeric() || c == '.')
                && b.chars().any(|c| c.is_ascii_digit())
        });
        let hash = (7..=64).contains(&w.len())
            && w.chars()
                .all(|c| c.is_ascii_hexdigit() && !c.is_ascii_uppercase())
            && w.chars().any(|c| c.is_ascii_digit())
            && w.chars().any(|c| c.is_ascii_alphabetic());
        let path = w.matches('/').count() >= 2
            && !w.contains("://")
            && w.chars()
                .all(|c| c.is_alphanumeric() || "/._-~".contains(c));
        if issue || hash || path {
            out.insert(w.to_string());
        }
    }
    out
}

/// `turn`'s silver labels: each a set of node ids of other sessions
/// written before the turn.
pub fn silver(turn: &Turn, rec: &Record) -> BTreeMap<Silver, BTreeSet<String>> {
    let mut out: BTreeMap<Silver, BTreeSet<String>> = BTreeMap::new();
    let before = |n: &&CNode| n.position < turn.as_of;
    let earlier: Vec<&CNode> = rec
        .nodes
        .iter()
        .filter(before)
        .filter(|n| n.session_id != turn.session_id)
        .collect();
    let own: Vec<&CNode> = rec
        .nodes
        .iter()
        .filter(before)
        .filter(|n| n.session_id == turn.session_id)
        .collect();
    // Re-supply: an operator message sharing an 8-word run.
    let mine = runs(&turn.said);
    if !mine.is_empty() {
        for n in earlier.iter().filter(|n| n.kind == "user_message") {
            if runs(&n.text).iter().any(|r| mine.contains(r)) {
                out.entry(Silver::ReSupply)
                    .or_default()
                    .insert(n.node_id.clone());
            }
        }
    }
    // Reference: where an identifier the turn names was first seen.
    for id in identifiers(&turn.said) {
        if own.iter().any(|n| n.text.contains(&id)) {
            continue;
        }
        if let Some(n) = earlier.iter().find(|n| n.text.contains(&id)) {
            out.entry(Silver::Reference)
                .or_default()
                .insert(n.node_id.clone());
        }
    }
    // Re-derivation: an older call with the same tool and input; its result.
    for call in &turn.calls {
        for c in earlier
            .iter()
            .filter(|n| n.call.as_deref() == Some(call.as_str()))
        {
            let result = earlier.iter().find(|n| {
                n.kind == "tool_result"
                    && n.session_id == c.session_id
                    && n.tool_use_id == c.tool_use_id
            });
            let id = result.map_or(&c.node_id, |r| &r.node_id);
            out.entry(Silver::ReDerivation)
                .or_default()
                .insert(id.clone());
        }
    }
    // Should-have: the operator's label on this turn's recall.
    for l in rec.labels.iter().filter(|l| {
        l.label == "should_have" && l.recall_id.as_deref() == Some(turn.recall_id.as_str())
    }) {
        out.entry(Silver::ShouldHave)
            .or_default()
            .insert(l.node_id.clone());
    }
    out
}

/// Who answers a replay's queries: a running tender, or a test's stand-in.
pub trait Index {
    /// `index.query`'s hits, as JSON.
    fn query(&mut self, text: &str, as_of: u64, sources: &[&str], k: usize) -> Result<Vec<Value>>;
}

/// A tender over its socket.
pub struct Tender(pub crate::client::Client);

impl Index for Tender {
    fn query(&mut self, text: &str, as_of: u64, sources: &[&str], k: usize) -> Result<Vec<Value>> {
        let a = self.0.call(
            "index.query",
            json!({"text": text, "k": k, "as_of": as_of, "sources": sources, "wait_ms": 60_000}),
            std::time::Duration::from_secs(90),
        )?;
        Ok(a["hits"].as_array().cloned().unwrap_or_default())
    }
}

/// The arms a replay recomputes, with the index's sources each asks for
/// (`MemoryArm::sources`).
pub const ARMS: [(&str, &[&str]); 3] = [
    ("none", &[]),
    ("bm25", &["bm25", "entity"]),
    ("baseline", &["bm25", "entity", "vector"]),
];

/// One arm's pack for one turn.
#[derive(Debug, Clone, Default)]
pub struct Pack {
    pub admitted: Vec<String>,
    /// Hits at or after the turn's `as_of`, dropped.
    pub leaks: usize,
}

/// `arm`'s pack for `turn`: the index's hits as of the turn, through the
/// pipeline.
pub fn pack(turn: &Turn, rec: &Record, index: &mut dyn Index, sources: &[&str]) -> Result<Pack> {
    if sources.is_empty() {
        return Ok(Pack::default());
    }
    let hits = index.query(&turn.query, turn.as_of, sources, K)?;
    let mut leaks = 0;
    let mut cands = Vec::new();
    for (i, h) in hits.iter().enumerate() {
        let position = h["position"].as_u64().unwrap_or(u64::MAX);
        if position >= turn.as_of {
            leaks += 1;
            continue;
        }
        cands.push(Candidate {
            node_id: h["node_id"].as_str().unwrap_or("").into(),
            chunk: h["chunk"].as_u64().unwrap_or(0),
            session_id: h["session_id"].as_str().unwrap_or("").into(),
            position,
            kind: h["kind"].as_str().unwrap_or("").into(),
            origin: h["origin"].as_str().unwrap_or("").into(),
            external: h["external"].as_bool().unwrap_or(false),
            text: h["text"].as_str().unwrap_or("").into(),
            fused: h["fused"].as_f64().unwrap_or(0.0),
            index_rank: i + 1,
            place: Place::Private,
        });
    }
    let in_context: BTreeSet<String> = rec
        .nodes
        .iter()
        .filter(|n| n.session_id == turn.session_id)
        .map(|n| n.node_id.clone())
        .collect();
    // A node's latest label before the turn decides.
    let mut latest: BTreeMap<&str, &str> = BTreeMap::new();
    for l in rec.labels.iter().filter(|l| l.position < turn.as_of) {
        latest.insert(&l.node_id, &l.label);
    }
    let labeled: BTreeSet<String> = latest
        .into_iter()
        .filter(|(_, l)| theseus_core::recall::labels::excludes(l) == Some(true))
        .map(|(n, _)| n.to_string())
        .collect();
    let asker = Asker {
        session_id: &turn.session_id,
        place: &Place::Private,
        in_context: &in_context,
        labeled: &labeled,
        // The recording keeps no memory-pass edges (31a): no newer-node rule here.
        links: &[],
        now_ms: theseus_protocol::now_unix_ms(),
    };
    let p = pipeline::recall(
        &Baseline::default(),
        &asker,
        cands,
        &theseus_core::config::MemoryConfig::default().params(),
    );
    Ok(Pack {
        admitted: p
            .admitted
            .into_iter()
            .map(|a| a.candidate.node_id)
            .collect(),
        leaks,
    })
}

/// One arm's scores against one label (or every label, `None`).
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Score {
    /// Turns with the label.
    pub turns: usize,
    pub recall: f64,
    /// Over the turns whose pack admitted something.
    pub precision: f64,
    pub precision_turns: usize,
    pub mrr: f64,
}

/// What a replay found.
#[derive(Debug, Default)]
pub struct Replay {
    pub turns: usize,
    pub skipped: Vec<String>,
    pub scores: BTreeMap<(String, Option<Silver>), Score>,
    pub admitted: BTreeMap<String, usize>,
    pub stale: BTreeMap<String, usize>,
    pub leaks: BTreeMap<String, usize>,
}

/// A score's running sums: its turns, then recall, precision and MRR.
type Sums = (Score, f64, f64, f64);

/// Every arm over every turn of `rec`.
pub fn replay(rec: &Record, index: &mut dyn Index) -> Result<Replay> {
    let mut out = Replay {
        turns: rec.turns.len(),
        skipped: rec.skipped.clone(),
        ..Replay::default()
    };
    let mut sums: BTreeMap<(String, Option<Silver>), Sums> = BTreeMap::new();
    for turn in &rec.turns {
        let labels = silver(turn, rec);
        let all: BTreeSet<String> = labels.values().flatten().cloned().collect();
        for (arm, sources) in ARMS {
            let p = pack(turn, rec, index, sources)?;
            *out.leaks.entry(arm.into()).or_default() += p.leaks;
            *out.admitted.entry(arm.into()).or_default() += p.admitted.len();
            *out.stale.entry(arm.into()).or_default() += p
                .admitted
                .iter()
                .filter(|id| {
                    rec.labels.iter().any(|l| {
                        &l.node_id == *id
                            && l.position >= turn.as_of
                            && theseus_core::recall::labels::excludes(&l.label) == Some(true)
                    })
                })
                .count();
            let mut kinds: Vec<(Option<Silver>, &BTreeSet<String>)> =
                labels.iter().map(|(k, v)| (Some(*k), v)).collect();
            if !all.is_empty() {
                kinds.push((None, &all));
            }
            for (k, set) in kinds {
                let hit = p.admitted.iter().filter(|id| set.contains(*id)).count();
                let rr = p
                    .admitted
                    .iter()
                    .position(|id| set.contains(id))
                    .map_or(0.0, |i| 1.0 / (i + 1) as f64);
                let e = sums.entry((arm.into(), k)).or_default();
                e.0.turns += 1;
                e.1 += hit as f64 / set.len() as f64;
                if !p.admitted.is_empty() {
                    e.0.precision_turns += 1;
                    e.2 += hit as f64 / p.admitted.len() as f64;
                }
                e.3 += rr;
            }
        }
    }
    for (key, (mut s, r, p, m)) in sums {
        let n = s.turns.max(1) as f64;
        s.recall = r / n;
        s.mrr = m / n;
        s.precision = if s.precision_turns > 0 {
            p / s.precision_turns as f64
        } else {
            f64::NAN
        };
        out.scores.insert(key, s);
    }
    Ok(out)
}

/// The replay over a copy of the store at `store`: read the record from a
/// copy, then serve another copy with a scratch daemon whose tender indexes
/// it (memory off, so the daemon writes no recall of its own), wait until the
/// tender holds it, replay every arm through that tender, and stop the
/// daemon. `work` is a directory of the replay's own.
pub fn run(
    theseusd: &std::path::Path,
    base: &toml::Table,
    store: &std::path::Path,
    work: &std::path::Path,
    env: &[(std::ffi::OsString, Option<std::ffi::OsString>)],
    settle: std::time::Duration,
    log: &mut dyn FnMut(&str),
) -> Result<Replay> {
    anyhow::ensure!(
        !work.exists(),
        "{} exists: give the replay a directory of its own",
        work.display()
    );
    let dir = work.join("daemon");
    crate::arms::copy_dir(store, &dir.join("state").join("store"))?;
    let (rec, last) = {
        let s = Store::open(&dir.join("state").join("store"))?;
        (read(&s)?, s.last_position())
    };
    log(&format!(
        "replay: {} turns with a recall row ({} left out), {} nodes, {} labels, through @{last}",
        rec.turns.len(),
        rec.skipped.len(),
        rec.nodes.len(),
        rec.labels.len()
    ));
    let mut d = crate::daemon::start(
        theseusd,
        &dir,
        "replay",
        &crate::daemon::replay_config(base),
        env,
    )?;
    d.wait_serving()?;
    d.wait_indexed(last, settle, log)?;
    let mut ix = Tender(crate::client::Client::connect(
        &d.state.join("index").join("sock"),
    )?);
    let r = replay(&rec, &mut ix);
    let killed = d.stop()?;
    anyhow::ensure!(
        killed.is_empty(),
        "the replay's daemon did not stop cleanly: {killed:?}"
    );
    r
}

/// The replay as Markdown.
pub fn render(r: &Replay) -> String {
    let mut o = String::new();
    let _ = writeln!(
        o,
        "# The replay: every arm over the recorded turns, each as of its turn"
    );
    let _ = writeln!(o);
    let _ = writeln!(
        o,
        "- Plan: `docs/m6-ablation-plan.md`, {}.",
        crate::report::plan_digest()
    );
    let _ = writeln!(
        o,
        "- Turns replayed: {}; recall rows left out: {} (their query could not be rebuilt).",
        r.turns,
        r.skipped.len()
    );
    let _ = writeln!(o, "- k is what each pack admitted (at most 6); the place rule is not applied (a store's copy has no outbox), and re-supply is the 8-word run alone (no model runs).");
    let _ = writeln!(o);
    let _ = writeln!(o, "| arm | label | turns | recall@k | precision@k | MRR |");
    let _ = writeln!(o, "|---|---|---|---|---|---|");
    for ((arm, k), s) in &r.scores {
        let p = if s.precision.is_nan() {
            "–".to_string()
        } else {
            format!("{:.2} (n = {})", s.precision, s.precision_turns)
        };
        let _ = writeln!(
            o,
            "| {arm} | {} | {} | {:.2} | {p} | {:.2} |",
            k.map_or("any", Silver::as_str),
            s.turns,
            s.recall,
            s.mrr
        );
    }
    let _ = writeln!(o);
    let _ = writeln!(
        o,
        "| arm | admitted | stale (labeled wrong or stale after the turn) | leaks dropped |"
    );
    let _ = writeln!(o, "|---|---|---|---|");
    for (arm, _) in ARMS {
        let a = r.admitted.get(arm).copied().unwrap_or(0);
        let s = r.stale.get(arm).copied().unwrap_or(0);
        let rate = if a > 0 {
            format!("{s} ({:.1}%)", 100.0 * s as f64 / a as f64)
        } else {
            "–".into()
        };
        let _ = writeln!(
            o,
            "| {arm} | {a} | {rate} | {} |",
            r.leaks.get(arm).copied().unwrap_or(0)
        );
    }
    let labeled: usize = Silver::ALL
        .iter()
        .filter_map(|k| r.scores.get(&("bm25".to_string(), Some(*k))))
        .map(|s| s.turns)
        .max()
        .unwrap_or(0);
    if labeled == 0 {
        let _ = writeln!(o);
        let _ = writeln!(
            o,
            "No turn has a silver label, so recall, precision and MRR are not measured."
        );
    }
    o
}

#[cfg(test)]
mod tests {
    use super::*;

    fn node(id: &str, session: &str, position: u64, kind: &'static str, text: &str) -> CNode {
        CNode {
            node_id: id.into(),
            session_id: session.into(),
            position,
            kind,
            text: text.into(),
            call: None,
            tool_use_id: None,
        }
    }

    fn turn(said: &str, as_of: u64) -> Turn {
        Turn {
            session_id: "ses_now".into(),
            turn_id: "trn_now".into(),
            recall_id: "rcl_now".into(),
            as_of,
            query: said.into(),
            said: said.into(),
            calls: vec![],
        }
    }

    /// A stand-in index that answers every node of the record, best first by
    /// its order, and ignores `as_of`: the replay must not trust it.
    struct Leaky<'a>(&'a Record);

    impl Index for Leaky<'_> {
        fn query(&mut self, _: &str, _: u64, sources: &[&str], _: usize) -> Result<Vec<Value>> {
            assert!(!sources.is_empty());
            Ok(self
                .0
                .nodes
                .iter()
                .filter(|n| n.kind != "recall")
                .enumerate()
                .map(|(i, n)| {
                    json!({"node_id": n.node_id, "chunk": 0, "session_id": n.session_id,
                           "position": n.position, "kind": n.kind, "origin": "operator",
                           "external": false, "text": n.text, "fused": 1.0 / (61 + i) as f64})
                })
                .collect())
        }
    }

    const RUN: &str = "the nightly backup on kestrel must finish before the dunlin sync starts";

    #[test]
    fn the_silver_labels_read_from_the_record() {
        let mut said = node(
            "u1",
            "ses_a",
            3,
            "user_message",
            &format!("Remember: {RUN}, always."),
        );
        said.text.push_str(" See crates/theseus-core/src/turn.rs.");
        let mut call = node("c1", "ses_b", 5, "tool_call", "");
        call.call = Some("fs.read {\"path\":\"notes.md\"}".into());
        call.tool_use_id = Some("toolu_1".into());
        let mut result = node("r1", "ses_b", 6, "tool_result", "the notes");
        result.tool_use_id = Some("toolu_1".into());
        let later = node("u9", "ses_c", 50, "user_message", RUN);
        let rec = Record {
            nodes: vec![
                said,
                node(
                    "x1",
                    "ses_b",
                    4,
                    "user_message",
                    "theseus-ab12 is the one to fix",
                ),
                call,
                result,
                later,
            ],
            labels: vec![Label {
                node_id: "x1".into(),
                label: "should_have".into(),
                recall_id: Some("rcl_now".into()),
                position: 60,
            }],
            ..Record::default()
        };
        let mut t = turn(
            &format!("As I said, {RUN}. And theseus-ab12 and crates/theseus-core/src/turn.rs?"),
            20,
        );
        t.calls = vec!["fs.read {\"path\":\"notes.md\"}".into()];
        let s = silver(&t, &rec);
        let ids = |k: Silver| {
            s.get(&k)
                .cloned()
                .unwrap_or_default()
                .into_iter()
                .collect::<Vec<_>>()
        };
        // The later node shares the run too, but was written after the turn.
        assert_eq!(ids(Silver::ReSupply), ["u1"]);
        assert_eq!(ids(Silver::Reference), ["u1", "x1"]);
        assert_eq!(ids(Silver::ReDerivation), ["r1"]);
        assert_eq!(ids(Silver::ShouldHave), ["x1"]);
        // Seven words are no run.
        let short = turn("the nightly backup on kestrel must finish", 20);
        assert!(!silver(&short, &rec).contains_key(&Silver::ReSupply));
    }

    #[test]
    fn identifiers_are_issue_ids_hashes_and_paths() {
        let got = identifiers(
            "Fix theseus-6fn.5 at 3b9d05b in crates/theseus-exam/src/drive.rs, not https://x.y/z/w or deadbeef or 1234567.",
        );
        assert_eq!(
            got.into_iter().collect::<Vec<_>>(),
            [
                "3b9d05b",
                "crates/theseus-exam/src/drive.rs",
                "theseus-6fn.5"
            ]
        );
    }

    /// The leakage test: a node written after a turn never appears in that
    /// turn's recall, even from an index that offers it.
    #[test]
    fn a_node_written_after_a_turn_never_appears_in_its_recall() {
        let rec = Record {
            nodes: vec![
                node("old", "ses_a", 3, "user_message", RUN),
                node("now", "ses_now", 10, "user_message", RUN),
                node("after", "ses_b", 30, "user_message", RUN),
            ],
            ..Record::default()
        };
        let t = turn(RUN, 10);
        let mut ix = Leaky(&rec);
        for (arm, sources) in ARMS {
            let p = pack(&t, &rec, &mut ix, sources).unwrap();
            assert!(!p.admitted.contains(&"after".to_string()), "{arm}: {p:?}");
            assert!(
                !p.admitted.contains(&"now".to_string()),
                "{arm}: its own session is in context"
            );
            if !sources.is_empty() {
                assert_eq!(p.admitted, ["old"], "{arm}");
                // `now` is at the turn's as_of, `after` past it.
                assert_eq!(p.leaks, 2, "{arm}");
            }
        }
    }

    /// Recall, precision and MRR against the labels, the stale rate from a
    /// label written after the turn, and a wrong label before it keeping a
    /// node out.
    #[test]
    fn the_metrics_read_as_computed_by_hand() {
        let rec = Record {
            nodes: vec![
                node("a", "ses_a", 1, "user_message", &format!("{RUN} one")),
                node("b", "ses_b", 2, "user_message", &format!("{RUN} two")),
                node("w", "ses_w", 3, "user_message", "an unrelated note"),
                node("now", "ses_now", 10, "user_message", RUN),
            ],
            turns: vec![turn(RUN, 10)],
            labels: vec![
                // Labeled wrong before the turn: out of every pack.
                Label {
                    node_id: "w".into(),
                    label: "wrong".into(),
                    recall_id: None,
                    position: 5,
                },
                // Labeled stale after it: counts in the stale rate.
                Label {
                    node_id: "b".into(),
                    label: "stale".into(),
                    recall_id: None,
                    position: 20,
                },
            ],
            ..Record::default()
        };
        let r = replay(&rec, &mut Leaky(&rec)).unwrap();
        // The index offers a, b, w, now (fused falling); now is in context and
        // w labeled wrong, so the pack is [a, b]; both share the run, so both
        // are re-supply labels: recall 1, precision 1, MRR 1.
        let s = &r.scores[&("bm25".to_string(), Some(Silver::ReSupply))];
        assert_eq!(
            (s.turns, s.recall, s.precision, s.precision_turns, s.mrr),
            (1, 1.0, 1.0, 1, 1.0)
        );
        assert_eq!(r.admitted["baseline"], 2);
        assert_eq!(r.stale["baseline"], 1, "b was labeled stale after the turn");
        // none admits nothing: recall 0, no precision.
        let n = &r.scores[&("none".to_string(), None)];
        assert_eq!((n.turns, n.recall, n.mrr), (1, 0.0, 0.0));
        assert!(n.precision.is_nan());
        let md = render(&r);
        assert!(
            md.contains("| bm25 | re-supply | 1 | 1.00 | 1.00 (n = 1) | 1.00 |"),
            "{md}"
        );
        assert!(md.contains("| baseline | 2 | 1 (50.0%) | 1 |"), "{md}");
    }
}
