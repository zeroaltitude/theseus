//! A thread's card, written from its episodes (theseus-71ib): the request a
//! cheap profile answers through one tool, and the answer read back as card
//! lines that cite what they came from. The instructions are one sentence; the
//! tool's schema carries the contract (the repository's minimal-prompting
//! rule). Every line sent is scrubbed first, as every outbound state is.
//!
//! What a card line is, by construction:
//! - **cited**: it names episodes (`E1`, `E2`, …) the call was given; a line
//!   naming none is dropped. The card shows each as a recall handle
//!   (`[m:…]`, the end of the node's id, [`is_handle`](theseus_protocol::cite::is_handle)):
//!   the episode's summary node, else its newest owner message shown. Recall's
//!   handles are as short as a pack of a dozen needs; a card's is read by
//!   `theseus history` across the whole store, where an imported id's last six
//!   characters (`ummary`, `7e48_3`) name thousands, so it keeps [`UNIQUE_HEX`]
//!   digits of the episode's digest and its unit (`_3`, `_summary`).
//! - **a summary, never the history**: a line that repeats a message of 60
//!   characters or more word for word, or is a stretch of one, is dropped. The
//!   imported messages stay testimony, reached by recall with their
//!   provenance; the card is what the thread starts knowing, not a copy of
//!   them.
//! - **short**: at most [`LINES`] lines of [`TEXT_CHARS`] characters, the
//!   thread record's own bounds.

use std::collections::BTreeSet;
use theseus_protocol::term::words::cut_marked;

use serde_json::{json, Value};
use theseus_protocol::cite;
use theseus_protocol::threads::MAX_CARD;

use super::candidates::Input;
use crate::provider::one_tool::{one_tool, Forcing};
use crate::provider::ProviderRequest;

/// The one tool the card is answered through.
pub const TOOL: &str = "write_card";
/// The answer's tokens at most: twelve short lines and their citations.
pub const MAX_TOKENS: u32 = 1200;
/// The most episodes one call reads (the newest), the most characters of
/// summary and of one owner message, the owner messages shown for an episode
/// that has no summary, and the whole text sent.
pub const MAX_EPISODES: usize = 30;
const SUMMARY_CHARS: usize = 700;
const MESSAGE_CHARS: usize = 300;
const MESSAGES_SHOWN: usize = 3;
const TOTAL_CHARS: usize = 24_000;
/// A card's lines, and a line's text without its handles.
pub const LINES: usize = MAX_CARD;
pub const TEXT_CHARS: usize = 190;
/// A message this long, copied, is history and not a summary.
const PASTE_CHARS: usize = 60;
/// The most episodes one line cites, so its handles fit the line.
const MAX_CITES: usize = 4;
/// The digits of an imported id's digest a handle keeps: 48 bits, so two of
/// 20,000 episodes share them about once in 10^6 stores.
pub const UNIQUE_HEX: usize = 12;

/// What the model is told, beside the tool.
pub const INSTRUCTIONS: &str = "Write the card for this line of work with the write_card tool: \
where it stands, one fact a line, from the episodes below and nothing else.";

/// One episode as the call sees it.
#[derive(Debug, Clone, PartialEq)]
pub struct Episode {
    /// `E1`, `E2`, …, newest first.
    pub label: String,
    pub session_id: String,
    /// The node whose handle cites it.
    pub anchor: String,
    /// The text sent for it.
    pub shown: Vec<String>,
    /// Every text of it a line must not repeat.
    pub texts: Vec<String>,
}

/// The episodes a call reads, newest first, within the text budget.
pub fn episodes(inputs: &[Input]) -> Vec<Episode> {
    let mut out: Vec<Episode> = Vec::new();
    let mut total = 0usize;
    for i in inputs.iter().take(MAX_EPISODES) {
        let mut shown = Vec::new();
        let mut texts: Vec<String> = i.owner.iter().map(|m| m.text.clone()).collect();
        let anchor = match &i.summary {
            Some(s) => {
                shown.push(cut(&s.text, SUMMARY_CHARS));
                texts.push(s.text.clone());
                s.node.clone()
            }
            None => {
                let from = i.owner.len().saturating_sub(MESSAGES_SHOWN);
                shown.extend(i.owner[from..].iter().map(|m| cut(&m.text, MESSAGE_CHARS)));
                match i.owner.last() {
                    Some(m) => m.node.clone(),
                    None => continue,
                }
            }
        };
        let size: usize = shown.iter().map(|t| t.chars().count()).sum();
        if total + size > TOTAL_CHARS && !out.is_empty() {
            break;
        }
        total += size;
        out.push(Episode {
            label: format!("E{}", out.len() + 1),
            session_id: i.session_id.clone(),
            anchor,
            shown,
            texts,
        });
    }
    out
}

fn cut(text: &str, max: usize) -> String {
    let one: String = text.split_whitespace().collect::<Vec<_>>().join(" ");
    cut_marked(&one, max)
}

/// The tool's schema: the contract.
pub fn tool() -> Value {
    json!({
        "name": TOOL,
        "description": "The thread's card: where this line of work stands.",
        "input_schema": {
            "type": "object",
            "properties": {
                "lines": {
                    "type": "array",
                    "description": format!("At most {LINES} lines, newest state first. Each is one fact in your own words, never a message copied, under {TEXT_CHARS} characters."),
                    "items": {
                        "type": "object",
                        "properties": {
                            "text": {"type": "string"},
                            "episodes": {"type": "array", "items": {"type": "string"},
                                "description": "The labels (E1, E2, ...) of the episodes the line comes from; at least one."}
                        },
                        "required": ["text", "episodes"]
                    }
                }
            },
            "required": ["lines"]
        }
    })
}

/// The request for `episodes`, scrubbed by `scrub`, with `how` as
/// `catalog::forcing` gives it (theseus-opypg, theseus-yym79).
pub fn request(
    model: &str,
    how: Forcing,
    title: &str,
    episodes: &[Episode],
    scrub: &dyn Fn(&str) -> String,
) -> ProviderRequest {
    let mut body = format!("Thread: {}\n", scrub(title.trim()));
    for e in episodes {
        body.push_str(&format!("\n{}:\n", e.label));
        for t in &e.shown {
            body.push_str(&format!("- {}\n", scrub(t)));
        }
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

/// A line read back: its text and the episodes it cites (indexes).
#[derive(Debug, Clone, PartialEq)]
pub struct Line {
    pub text: String,
    pub cites: Vec<usize>,
}

/// What the answer's `write_card` call held: its kept lines, and how many it
/// dropped (uncited, citing nothing the call was given, or a pasted message).
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Parsed {
    pub lines: Vec<Line>,
    pub dropped: u32,
}

/// The card of an answer's content; `None` when it holds no such call.
pub fn parse(content: &[Value], episodes: &[Episode]) -> Option<Parsed> {
    let input = content
        .iter()
        .find(|b| b["type"] == "tool_use" && b["name"] == TOOL)?
        .get("input")?;
    let mut out = Parsed::default();
    for l in input["lines"].as_array()? {
        if out.lines.len() == LINES {
            break;
        }
        let text = cut(l["text"].as_str().unwrap_or(""), TEXT_CHARS);
        let mut cites = Vec::new();
        for label in l["episodes"].as_array().into_iter().flatten() {
            let i = label
                .as_str()
                .and_then(|s| episodes.iter().position(|e| e.label == s.trim()));
            if let Some(i) = i.filter(|i| !cites.contains(i)) {
                cites.push(i);
            }
        }
        if text.is_empty() || cites.is_empty() || pasted(&text, episodes) {
            out.dropped += 1;
            continue;
        }
        cites.sort_unstable();
        cites.truncate(MAX_CITES);
        out.lines.push(Line { text, cites });
    }
    Some(out)
}

fn norm(s: &str) -> String {
    s.split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .to_lowercase()
}

/// Whether `text` repeats a message of the episodes, or a stretch of one.
fn pasted(text: &str, episodes: &[Episode]) -> bool {
    let line = norm(text);
    episodes.iter().flat_map(|e| &e.texts).any(|t| {
        let t = norm(t);
        (t.chars().count() >= PASTE_CHARS && line.contains(&t))
            || (line.chars().count() >= PASTE_CHARS && t.contains(&line))
    })
}

/// One citation of a card: the handle, its node and its session.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Cited {
    pub handle: String,
    pub node: String,
    pub session_id: String,
}

/// The card's lines, each ending with its episodes' handles, and the
/// citations they make, once each.
pub fn render(lines: &[Line], episodes: &[Episode]) -> (Vec<String>, Vec<Cited>) {
    let handles: Vec<String> = episodes.iter().map(|e| handle_of(&e.anchor)).collect();
    let mut cited: Vec<Cited> = Vec::new();
    let mut seen = BTreeSet::new();
    let card = lines
        .iter()
        .map(|l| {
            let marks: Vec<String> = l
                .cites
                .iter()
                .map(|&i| {
                    if seen.insert(i) {
                        cited.push(Cited {
                            handle: handles[i].clone(),
                            node: episodes[i].anchor.clone(),
                            session_id: episodes[i].session_id.clone(),
                        });
                    }
                    cite::bracketed(&handles[i])
                })
                .collect();
            format!("{} {}", l.text, marks.join(" "))
        })
        .collect();
    (card, cited)
}

/// The handle of an imported node's id: its last characters, from its unit
/// (`_3`, `_summary`) back through [`UNIQUE_HEX`] digits of the episode's
/// digest.
pub fn handle_of(anchor: &str) -> String {
    let unit = anchor.rsplit('_').next().map_or(0, |u| u.len() + 1);
    let n = (unit + UNIQUE_HEX).max(cite::MIN_TAIL);
    let len = anchor.chars().count();
    let tail: String = anchor.chars().skip(len.saturating_sub(n)).collect();
    format!("{}{tail}", cite::HANDLE_PREFIX)
}
