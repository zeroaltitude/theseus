//! The request a cheap profile answers with one card statement per cluster,
//! and the answer read back (theseus-slvl.6). The instructions are one
//! sentence: the tool's schema and its field descriptions carry the contract
//! (the repository's minimal-prompting rule). Every line sent is scrubbed.
//!
//! A statement is kept only if it is **cited** (it names an episode the call
//! showed for its cluster), **its own words** (it does not copy a line of 60
//! characters or more), and **short**; and (theseus-ly75u) **lasting** (the
//! model did not mark it `one_task`), **specific** (two content words at
//! least), and **for whom it was said** (a line about acting on someone's
//! behalf keeps that in the statement). Whether it repeats something, and
//! which of two conflicting statements is offered, is the job's (`run.rs`).

use std::collections::BTreeMap;
use theseus_protocol::term::words::cut_marked;

use serde_json::{json, Value};
use theseus_protocol::{StandingKind, StandingScope, SURFACES};

use super::cluster::Cluster;
use super::evidence::Episode;
use crate::provider::one_tool::{one_tool, Forcing};
use crate::provider::ProviderRequest;

/// The one tool the statements come back through.
pub const TOOL: &str = "state_cards";
/// The answer's tokens at most: a statement a cluster, each a line or two.
pub const MAX_TOKENS: u32 = 1500;
/// The clusters one call reads, the lines shown of one, and the episodes one
/// cluster names.
pub const BATCH: usize = 8;
const SHOWN_LINES: usize = 4;
/// A statement's characters at most.
pub const TEXT_CHARS: usize = 160;
/// A line this long, copied, is a message and not a statement.
const PASTE_CHARS: usize = 60;
/// The scope a statement of one task, repository, branch or run is given.
pub const ONE_TASK: &str = "one_task";
/// The fewest content words a statement has: fewer says nothing a turn can act on.
const MIN_CONTENT: usize = 2;
/// The words that say on whose behalf one acts.
const BEHALF: &[&str] = &[
    "behalf",
    "in his name",
    "in her name",
    "in their name",
    "speak for",
];

/// What the model is told, beside the tool.
pub const INSTRUCTIONS: &str = "These clusters are the owner's own words, grouped by what they \
say. State each as a card entry with the state_cards tool, from the lines below and nothing else. \
Keep who acts and on whose behalf exactly as the lines say: a line telling an assistant not to \
speak on the owner's behalf must not become one telling it not to speak at all. Each entry says \
what to do or not do, and when, in words a later turn could act on; give none for a cluster that \
cannot be made that specific, and none for an assistant's instructions to itself (how to run a \
session, what file to read, how to keep its memory), which are not about the owner.";

/// One cluster as the call sees it.
#[derive(Debug, Clone, PartialEq)]
pub struct Shown {
    /// `K1`, `K2`, …
    pub label: String,
    /// The cluster's index in the caller's list.
    pub cluster: usize,
    /// The episodes shown, as (label `E<n>`, scan index).
    pub episodes: Vec<(String, usize)>,
    /// The lines, each with its episode's label.
    pub lines: Vec<(String, String)>,
}

/// The clusters of one call: `picked` are indexes into `clusters`.
pub fn shown(picked: &[usize], clusters: &[Cluster]) -> Vec<Shown> {
    let mut labels: Vec<usize> = Vec::new();
    picked
        .iter()
        .enumerate()
        .map(|(k, &ci)| {
            let c = &clusters[ci];
            let mut episodes: Vec<(String, usize)> = Vec::new();
            let mut lines = Vec::new();
            for m in c.lines.iter().take(SHOWN_LINES) {
                let pos = match labels.iter().position(|&e| e == m.episode) {
                    Some(p) => p,
                    None => {
                        labels.push(m.episode);
                        labels.len() - 1
                    }
                };
                let label = format!("E{}", pos + 1);
                if !episodes.iter().any(|(l, _)| *l == label) {
                    episodes.push((label.clone(), m.episode));
                }
                lines.push((label, m.text.clone()));
            }
            Shown {
                label: format!("K{}", k + 1),
                cluster: ci,
                episodes,
                lines,
            }
        })
        .collect()
}

/// The tool's schema: the contract.
pub fn tool() -> Value {
    let mut scopes = vec!["everywhere".to_string()];
    scopes.extend(SURFACES.iter().map(|s| s.to_string()));
    scopes.push(ONE_TASK.to_string());
    json!({
        "name": TOOL,
        "description": "Entries for the owner's card: lasting facts and preferences about the owner.",
        "input_schema": {
            "type": "object",
            "properties": {
                "cards": {
                    "type": "array",
                    "description": "One entry per cluster that states something lasting about the owner; none for a cluster that does not (a one-off request, a task's detail, a feeling of the day).",
                    "items": {
                        "type": "object",
                        "properties": {
                            "cluster": {"type": "string", "description": "The cluster's label (K1, K2, ...)."},
                            "text": {"type": "string", "description": format!("The fact or preference, one short phrase about the owner that completes \"The owner ...\", in your own words, never a line copied, under {TEXT_CHARS} characters.")},
                            "kind": {"type": "string", "enum": ["preference", "fact"]},
                            "scope": {"type": "string", "enum": scopes, "description": "Where it holds: everywhere, only on the one surface the lines say it for, or one_task when the lines speak of one task, one repository, one branch or one run."},
                            "conflicts": {"type": "array", "items": {"type": "string"},
                                "description": "The labels of this call's other clusters (K1, ...), or of an earlier entry listed after them (P1, ...), whose entry gives the opposite instruction for the same situation, if any."},
                            "episodes": {"type": "array", "items": {"type": "string"},
                                "description": "The labels (E1, E2, ...) of the episodes it rests on; at least one, from that cluster's lines."}
                        },
                        "required": ["cluster", "text", "kind", "scope", "episodes"]
                    }
                }
            },
            "required": ["cards"]
        }
    })
}

/// The request for `shown`, scrubbed by `scrub`, with `thinking` as
/// `catalog::thinking_off` gives it: the cap would be spent thinking first
/// (theseus-opypg), and `how.force` whether the tool is forced or left to
/// `auto` and made strict (theseus-yym79). `earlier` are the entries this run made in earlier calls,
/// listed after the clusters as `P1`, `P2`, ... so a conflict with one is
/// named; the system text and the tool stay the same bytes for every call.
pub fn request(
    model: &str,
    how: Forcing,
    shown: &[Shown],
    earlier: &[String],
    scrub: &dyn Fn(&str) -> String,
) -> ProviderRequest {
    let mut body = String::new();
    for s in shown {
        body.push_str(&format!("{}:\n", s.label));
        for (e, t) in &s.lines {
            body.push_str(&format!("- {e}: {}\n", scrub(t)));
        }
        body.push('\n');
    }
    if !earlier.is_empty() {
        body.push_str("Entries already made earlier in this run:\n");
        for (i, t) in earlier.iter().enumerate() {
            body.push_str(&format!("P{}: {}\n", i + 1, scrub(t)));
        }
    }
    let (tool, choice) = one_tool(tool(), how.force);
    let mut extra = BTreeMap::new();
    extra.insert("tool_choice".to_string(), choice);
    ProviderRequest {
        model: model.to_string(),
        max_tokens: MAX_TOKENS,
        system: vec![json!({"type": "text", "text": INSTRUCTIONS})],
        messages: vec![json!({"role": "user", "content": [{"type": "text", "text": body}]})],
        tools: vec![tool],
        thinking: how.thinking,
        output_config: None,
        cache_control: None,
        betas: Vec::new(),
        extra,
        image_tokens: 0,
    }
}

/// A statement read back.
#[derive(Debug, Clone, PartialEq)]
pub struct Statement {
    /// The index into the caller's `clusters`.
    pub cluster: usize,
    pub text: String,
    pub kind: StandingKind,
    pub scope: StandingScope,
    /// The clusters (indexes into the caller's) of this call whose statement
    /// the model said gives the opposite instruction.
    pub conflicts: Vec<usize>,
}

/// What the answer's `state_cards` call held: its kept statements, how many
/// it dropped (an unknown cluster, uncited, a paste, empty, thin, a turned
/// "on whose behalf", or a second for one cluster), and how many were one
/// task's.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Parsed {
    pub statements: Vec<Statement>,
    pub dropped: u32,
    pub one_task: u32,
    /// (index into `statements`, index of an earlier entry of the run) for
    /// each statement the model says conflicts with that entry.
    pub earlier_clash: Vec<(usize, usize)>,
}

/// The statements of an answer's content; `None` when it holds no such call.
/// `earlier` is how many earlier entries of the run the request listed.
pub fn parse(content: &[Value], shown: &[Shown], earlier: usize) -> Option<Parsed> {
    let input = content
        .iter()
        .find(|b| b["type"] == "tool_use" && b["name"] == TOOL)?
        .get("input")?;
    let mut out = Parsed::default();
    for c in input["cards"].as_array()? {
        let s = c["cluster"]
            .as_str()
            .and_then(|l| shown.iter().find(|s| s.label == l.trim()));
        let text = cut(c["text"].as_str().unwrap_or(""), TEXT_CHARS);
        let cites: Vec<&str> = c["episodes"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(|l| l.as_str())
            .map(str::trim)
            .filter(|l| s.is_some_and(|s| s.episodes.iter().any(|(e, _)| e == l)))
            .collect();
        let Some(s) = s.filter(|_| !cites.is_empty() && !text.is_empty()) else {
            out.dropped += 1;
            continue;
        };
        if pasted(&text, s)
            || thin(&text)
            || turned(&text, s, &cites)
            || out.statements.iter().any(|x| x.cluster == s.cluster)
        {
            out.dropped += 1;
            continue;
        }
        let kind = match c["kind"].as_str() {
            Some("fact") => StandingKind::Fact,
            _ => StandingKind::Preference,
        };
        let scope = match c["scope"].as_str() {
            // No scope narrower than everywhere holds one task's words: the
            // statement is not proposed.
            Some(ONE_TASK) => {
                out.one_task += 1;
                continue;
            }
            Some(x) if SURFACES.contains(&x) => StandingScope::Surface(x.to_string()),
            _ => StandingScope::Everywhere,
        };
        let labels: Vec<&str> = c["conflicts"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(|l| l.as_str())
            .map(str::trim)
            .collect();
        for l in &labels {
            let n = l.strip_prefix('P').and_then(|n| n.parse::<usize>().ok());
            if let Some(n) = n.filter(|&n| (1..=earlier).contains(&n)) {
                out.earlier_clash.push((out.statements.len(), n - 1));
            }
        }
        let conflicts = labels
            .iter()
            .filter_map(|l| shown.iter().find(|x| x.label == *l))
            .map(|x| x.cluster)
            .filter(|&k| k != s.cluster)
            .collect();
        out.statements.push(Statement {
            cluster: s.cluster,
            text,
            kind,
            scope,
            conflicts,
        });
    }
    Some(out)
}

fn cut(s: &str, max: usize) -> String {
    let one: String = s.split_whitespace().collect::<Vec<_>>().join(" ");
    cut_marked(&one, max)
}

fn norm(s: &str) -> String {
    s.split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .to_lowercase()
}

/// Whether `text` says too little for a turn to act on.
fn thin(text: &str) -> bool {
    super::cluster::words(text).len() < MIN_CONTENT
}

/// Whether `text` lost the "on whose behalf" its cluster's lines say: a line
/// about acting on someone's behalf, restated without it, has changed who
/// the instruction is about. Only the lines of the episodes `cites` names
/// count: a statement resting on the cluster's other lines is not turned by
/// one it does not use.
fn turned(text: &str, s: &Shown, cites: &[&str]) -> bool {
    let has = |t: &str| {
        let t = norm(t);
        BEHALF.iter().any(|b| t.contains(b))
    };
    !has(text)
        && s.lines
            .iter()
            .any(|(e, l)| cites.contains(&e.as_str()) && has(l))
}

/// Whether `text` copies a line shown for its cluster, or a stretch of one.
fn pasted(text: &str, s: &Shown) -> bool {
    pasted_in(text, s.lines.iter().map(|(_, l)| l.as_str()))
}

/// Whether `text` copies one of `lines`, or a stretch of one: the rule
/// `pasted` and the exchange-end observer (`observe`) share.
pub(crate) fn pasted_in<'a>(text: &str, lines: impl IntoIterator<Item = &'a str>) -> bool {
    let t = norm(text);
    lines.into_iter().any(|l| {
        let l = norm(l);
        (l.chars().count() >= PASTE_CHARS && t.contains(&l))
            || (t.chars().count() >= PASTE_CHARS && l.contains(&t))
    })
}

/// The episodes a scan shows for a cluster whole (not just the lines
/// shown): the proposal's sources.
pub fn cluster_episodes<'a>(c: &Cluster, eps: &'a [Episode]) -> Vec<&'a Episode> {
    c.episodes.iter().map(|&i| &eps[i]).collect()
}
