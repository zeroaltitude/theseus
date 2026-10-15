//! The request a cheap profile answers with one card statement per cluster,
//! and the answer read back (theseus-slvl.6). The instructions are one
//! sentence: the tool's schema and its field descriptions carry the contract
//! (the repository's minimal-prompting rule). Every line sent is scrubbed.
//!
//! A statement is kept only if it is **cited** (it names an episode the call
//! showed for its cluster), **its own words** (it does not copy a line of 60
//! characters or more), and **short**. Whether it repeats something is the
//! job's check (`run.rs`).

use std::collections::BTreeMap;

use serde_json::{json, Value};
use theseus_protocol::{StandingKind, StandingScope, SURFACES};

use super::cluster::Cluster;
use super::evidence::Episode;
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

/// What the model is told, beside the tool.
pub const INSTRUCTIONS: &str = "These clusters are the owner's own words, grouped by what they \
say. State each as a card entry with the state_cards tool, from the lines below and nothing else.";

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
                            "scope": {"type": "string", "enum": scopes, "description": "Where it holds: everywhere, or only on the one surface the lines say it for."},
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

/// The request for `shown`, scrubbed by `scrub`.
pub fn request(model: &str, shown: &[Shown], scrub: &dyn Fn(&str) -> String) -> ProviderRequest {
    let mut body = String::new();
    for s in shown {
        body.push_str(&format!("{}:\n", s.label));
        for (e, t) in &s.lines {
            body.push_str(&format!("- {e}: {}\n", scrub(t)));
        }
        body.push('\n');
    }
    let mut extra = BTreeMap::new();
    extra.insert(
        "tool_choice".to_string(),
        json!({"type": "tool", "name": TOOL}),
    );
    ProviderRequest {
        model: model.to_string(),
        max_tokens: MAX_TOKENS,
        system: vec![json!({"type": "text", "text": INSTRUCTIONS})],
        messages: vec![json!({"role": "user", "content": [{"type": "text", "text": body}]})],
        tools: vec![tool()],
        thinking: None,
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
}

/// What the answer's `state_cards` call held: its kept statements, and how
/// many it dropped (an unknown cluster, uncited, a paste, empty, or a second
/// for one cluster).
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Parsed {
    pub statements: Vec<Statement>,
    pub dropped: u32,
}

/// The statements of an answer's content; `None` when it holds no such call.
pub fn parse(content: &[Value], shown: &[Shown]) -> Option<Parsed> {
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
        let cited = c["episodes"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(|l| l.as_str())
            .any(|l| s.is_some_and(|s| s.episodes.iter().any(|(e, _)| *e == l.trim())));
        let Some(s) = s.filter(|_| cited && !text.is_empty()) else {
            out.dropped += 1;
            continue;
        };
        if pasted(&text, s) || out.statements.iter().any(|x| x.cluster == s.cluster) {
            out.dropped += 1;
            continue;
        }
        let kind = match c["kind"].as_str() {
            Some("fact") => StandingKind::Fact,
            _ => StandingKind::Preference,
        };
        let scope = match c["scope"].as_str() {
            Some(x) if SURFACES.contains(&x) => StandingScope::Surface(x.to_string()),
            _ => StandingScope::Everywhere,
        };
        out.statements.push(Statement {
            cluster: s.cluster,
            text,
            kind,
            scope,
        });
    }
    Some(out)
}

fn cut(s: &str, max: usize) -> String {
    let one: String = s.split_whitespace().collect::<Vec<_>>().join(" ");
    match one.char_indices().nth(max) {
        Some((at, _)) => format!("{}…", &one[..at.min(one.len())]),
        None => one,
    }
}

fn norm(s: &str) -> String {
    s.split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .to_lowercase()
}

/// Whether `text` copies a line shown for its cluster, or a stretch of one.
fn pasted(text: &str, s: &Shown) -> bool {
    let t = norm(text);
    s.lines.iter().any(|(_, l)| {
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
