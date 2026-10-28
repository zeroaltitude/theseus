//! The observer's request and its answer read back (theseus-6f8t0). The
//! instructions are one sentence: the tool's schema carries the contract (the
//! repository's minimal-prompting rule). Every line sent is scrubbed. Code,
//! not the model, decides what is kept: `parse` drops an item that is not
//! well formed, cited, lasting, specific to one task, or a copy of a line;
//! `filter` drops what the card already holds.

use serde_json::{json, Value};
use theseus_protocol::{ObservationKind, CARD_OBS_MAX_TEXT};

use crate::card::{ask, cluster, evidence};
use crate::provider::one_tool::{one_tool, Forcing};
use crate::provider::ProviderRequest;

/// The one tool the observations come back through.
pub const TOOL: &str = "observe";
/// The answer's tokens at most: a few items, each a line or two.
pub const MAX_TOKENS: u32 = 1200;
/// The owner lines one call reads, the newest standing records of one scope
/// it lists, and the items one call keeps.
pub const MAX_LINES: usize = 40;
pub const MAX_RECORDS: usize = 64;
pub const MAX_KEPT: usize = 8;
/// A line's characters at most, as sent.
pub const LINE_CHARS: usize = 1000;

/// What the model is told, beside the tool.
pub const INSTRUCTIONS: &str = "Record with the observe tool only what the owner said about the \
owner that should hold from now on, never a judgment of the owner; none is better than a guess.";

/// One of the owner's own lines, with the node it came from.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OwnerLine {
    pub node: String,
    pub text: String,
}

/// A live standing record shown to the model, labelled `R<n>`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Shown {
    pub id: String,
    pub kind: &'static str,
    pub scope: String,
    pub text: String,
}

/// The tool's schema: the contract.
pub fn tool() -> Value {
    json!({
        "name": TOOL,
        "description": "What the owner's lines say about the owner that should hold from now on.",
        "input_schema": {
            "type": "object",
            "properties": {
                "observations": {
                    "type": "array",
                    "description": "One entry per lasting thing the owner said about the owner; none when there is nothing.",
                    "items": {
                        "type": "object",
                        "properties": {
                            "kind": {"type": "string", "enum": ["fact", "preference", "decision", "correction"]},
                            "text": {"type": "string",
                                "description": "One short phrase about the owner, in your own words, at most 160 characters."},
                            "evidence": {"type": "array", "items": {"type": "string"},
                                "description": "The labels (L1, L2, ...) of the owner's lines it rests on."},
                            "restates": {"type": "string",
                                "description": "The label (R1, ...) of the record it says again, or empty."},
                            "contradicts": {"type": "string",
                                "description": "The label (R1, ...) of the record it contradicts, or empty."},
                            "lasting": {"type": "boolean",
                                "description": "True when it should hold beyond this task or day."}
                        },
                        "required": ["kind", "text", "evidence", "restates", "contradicts", "lasting"]
                    }
                }
            },
            "required": ["observations"]
        }
    })
}

/// The request: the owner's lines `L<n>` and the live records `R<n>`, each
/// scrubbed by `scrub`, with `how` as `catalog::forcing` gives it.
pub fn request(
    model: &str,
    how: Forcing,
    lines: &[OwnerLine],
    records: &[Shown],
    scrub: &dyn Fn(&str) -> String,
) -> ProviderRequest {
    let mut body = String::from("The owner's lines:\n");
    for (i, l) in lines.iter().enumerate() {
        body.push_str(&format!("L{} {}\n", i + 1, scrub(&l.text)));
    }
    body.push_str("\nThe card's records:\n");
    for (i, r) in records.iter().enumerate() {
        body.push_str(&format!(
            "R{} ({} {}) {}\n",
            i + 1,
            r.kind,
            r.scope,
            scrub(&r.text)
        ));
    }
    let (tool, choice) = one_tool(tool(), how.force);
    let mut extra = std::collections::BTreeMap::new();
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

/// One item that passed `parse`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Item {
    pub kind: ObservationKind,
    pub text: String,
    /// The node ids of the lines it cites, in the order cited.
    pub nodes: Vec<String>,
    /// The index (into the shown records) of the record it says again.
    pub restates: Option<usize>,
    pub contradicts: Option<usize>,
}

/// An answer read: the items that passed and how many were dropped.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Parsed {
    pub items: Vec<Item>,
    /// How many items the model returned, dropped ones included.
    pub returned: u32,
}

/// A label `L3` / `R2` as its index, none for anything else.
fn label(v: &Value, letter: char) -> Option<usize> {
    let s = v.as_str()?.trim();
    let rest = s
        .strip_prefix(letter)
        .or_else(|| s.strip_prefix(letter.to_ascii_lowercase()))?;
    rest.trim().parse::<usize>().ok()?.checked_sub(1)
}

/// The kind a word names; a trait, a behaviour or anything else is none.
fn kind_of(v: &Value) -> Option<ObservationKind> {
    serde_json::from_value(v.clone()).ok()
}

/// The items of an answer's `observe` call that are well formed: a kind that
/// parses, `lasting` true, a text of 1 to [`CARD_OBS_MAX_TEXT`] characters
/// (never cut), at least one real line cited, no one-task text or line, and
/// no copy of a line. `None` when the answer holds no such call.
pub fn parse(content: &[Value], lines: &[OwnerLine], records: &[Shown]) -> Option<Parsed> {
    let input = content
        .iter()
        .find(|b| b["type"] == "tool_use" && b["name"] == TOOL)?
        .get("input")?;
    let items = input["observations"].as_array()?;
    let mut out = Parsed {
        returned: items.len() as u32,
        ..Parsed::default()
    };
    for it in items {
        let Some(kind) = kind_of(&it["kind"]) else {
            continue;
        };
        if it["lasting"].as_bool() != Some(true) {
            continue;
        }
        let text = it["text"]
            .as_str()
            .unwrap_or("")
            .split_whitespace()
            .collect::<Vec<_>>()
            .join(" ");
        let chars = text.chars().count();
        if chars == 0 || chars > CARD_OBS_MAX_TEXT {
            continue;
        }
        let mut nodes: Vec<String> = Vec::new();
        for l in it["evidence"].as_array().into_iter().flatten() {
            if let Some(line) = label(l, 'L').and_then(|i| lines.get(i)) {
                if !nodes.contains(&line.node) {
                    nodes.push(line.node.clone());
                }
            }
        }
        if nodes.is_empty() {
            continue;
        }
        let cited = || {
            nodes
                .iter()
                .filter_map(|n| lines.iter().find(|l| &l.node == n))
        };
        if evidence::one_task(&text) || cited().any(|l| evidence::one_task(&l.text)) {
            continue;
        }
        if ask::pasted_in(&text, lines.iter().map(|l| l.text.as_str())) {
            continue;
        }
        let known = |v: &Value| label(v, 'R').filter(|&i| i < records.len());
        out.items.push(Item {
            kind,
            text,
            nodes,
            restates: known(&it["restates"]),
            contradicts: known(&it["contradicts"]),
        });
    }
    Some(out)
}

/// What the card already holds, for `filter`.
#[derive(Debug, Clone, Default)]
pub struct Held<'a> {
    /// Capture's open asks and noted corrections: lines already asked.
    pub asked: &'a [String],
    /// The words of this session's earlier observations.
    pub earlier: &'a [String],
}

/// The result of `filter`.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Filtered {
    /// Items to write as observations, at most [`MAX_KEPT`].
    pub keep: Vec<Item>,
    /// Items that say a live record again: the record's index and the node
    /// of the first line cited. No observation is written for them.
    pub restated: Vec<(usize, String)>,
}

/// Drop what the card already holds: an item the model says restates a record,
/// or that is `similar` to one, is counted as said again; one that is the same
/// as a line capture asked, or as an earlier observation of the session or of
/// this call, is dropped. Keeps at most [`MAX_KEPT`].
pub fn filter(parsed: Parsed, records: &[Shown], held: &Held<'_>) -> Filtered {
    let mut out = Filtered::default();
    for mut it in parsed.items {
        // A contradiction shares most of its words with the record it
        // contradicts: only a restatement the model names is taken as one then.
        let similar = records.iter().position(|r| {
            it.contradicts.is_none()
                && (cluster::similar(&r.text, &it.text)
                    || crate::capture::words::same(&r.text, &it.text))
        });
        // A named contradiction wins over a named restatement of the same item.
        if let Some(r) = it.restates.filter(|_| it.contradicts.is_none()).or(similar) {
            if !out.restated.iter().any(|(i, _)| *i == r) {
                out.restated.push((r, it.nodes[0].clone()));
            }
            continue;
        }
        let same = |t: &String| crate::capture::words::same(t, &it.text);
        if held.asked.iter().any(same)
            || held.earlier.iter().any(|t| cluster::similar(t, &it.text))
            || out.keep.iter().any(|k| cluster::similar(&k.text, &it.text))
        {
            continue;
        }
        // Past the cap, later restatements are still counted as said again.
        if out.keep.len() >= MAX_KEPT {
            continue;
        }
        // A contradiction the model named must be a record it was shown.
        it.contradicts = it.contradicts.filter(|&i| i < records.len());
        out.keep.push(it);
    }
    out
}
