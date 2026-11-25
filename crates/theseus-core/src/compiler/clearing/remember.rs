//! What a cleared result's stub remembers (theseus-5lzox): the lines of a
//! file the read returned, and the first sentence the model wrote after the
//! result. Both are pure functions of stored nodes, so every render of a
//! stub is the same bytes and no model is asked.

use std::collections::{HashMap, HashSet};

use serde_json::Value;

use crate::node::{Body, Node};

/// The most characters of a note.
pub const NOTE_CHARS: usize = 120;

/// The first sentence of the model's text in `blocks`, whitespace folded,
/// at most `NOTE_CHARS` characters, cut at a word with an ellipsis when
/// longer. None when there is no text.
pub fn note_of(blocks: &[Value]) -> Option<String> {
    let text = blocks
        .iter()
        .filter(|b| b.get("type").and_then(Value::as_str) == Some("text"))
        .filter_map(|b| b.get("text").and_then(Value::as_str))
        .find(|t| !t.trim().is_empty())?;
    let words: Vec<&str> = text.split_whitespace().collect();
    let mut sentence = String::new();
    for w in &words {
        if !sentence.is_empty() {
            sentence.push(' ');
        }
        sentence.push_str(w);
        if w.ends_with(['.', '!', '?', ':']) {
            break;
        }
    }
    let sentence = sentence.replace('"', "'");
    if sentence.chars().count() <= NOTE_CHARS {
        return Some(sentence);
    }
    // The word end and the trimmed stops are checked again on the whole
    // sentence: a value the head kept whole may hold that space or stop.
    let cuts = crate::scrub::Cuts::live();
    let (cut, _) = crate::scrub::safe::head_with(&cuts, &sentence, NOTE_CHARS);
    let mut at = cut.rfind(' ').filter(|i| *i > 0).unwrap_or(cut.len());
    loop {
        let trimmed = sentence[..at].trim_end_matches([',', ';', ':', ' ']).len();
        let end = cuts.before(&sentence, trimmed);
        if end == at {
            break;
        }
        at = end;
    }
    Some(format!("{}…", &sentence[..at]))
}

/// For each cleared result among `nodes` (in order), by the result's own node
/// id (not its call's id, which a provider may repeat between exchanges,
/// issue 44), the note of the next answer of its own exchange after it, when that answer has text. A cleared
/// result's next answer exists when it is cleared (the newest exchange's
/// results stay whole) and never changes, so the note is stable.
pub(in crate::compiler) fn notes<'n>(
    nodes: impl Iterator<Item = &'n Node>,
    cleared: &HashSet<&str>,
    replaced: &HashSet<&str>,
) -> HashMap<String, String> {
    let mut out = HashMap::new();
    if cleared.is_empty() {
        return out;
    }
    // Each cleared result waits for the next answer of its OWN exchange (its session and turn): a request lays
    // many exchanges, and another exchange's answer is not what was written after it.
    let mut waiting: Vec<(&str, (&str, Option<&str>))> = Vec::new();
    for n in nodes {
        let exchange = (n.session_id.as_str(), n.turn_id.as_deref());
        match &n.body {
            Body::ToolResult { late: false, .. } if cleared.contains(n.id.as_str()) => {
                waiting.push((n.id.as_str(), exchange))
            }
            Body::AssistantMessage { blocks, .. }
                if !waiting.is_empty() && !replaced.contains(n.id.as_str()) =>
            {
                let note = note_of(blocks);
                waiting.retain(|(id, of)| {
                    if *of != exchange {
                        return true;
                    }
                    out.extend(note.clone().map(|note| (id.to_string(), note)));
                    false
                });
            }
            _ => {}
        }
    }
    out
}

/// The lines a read's result holds: its first and last row numbers, and the
/// file's length when the result says it (its `[showing lines a-b of N; …]`
/// footer, else the tool's own `lines_total`). None when the result has no
/// numbered rows.
pub fn read_lines(content: &str, meta: &Value) -> Option<(u64, u64, Option<u64>)> {
    let row = |l: &str| {
        let (n, _) = l.split_once('\t')?;
        (n.len() >= 6 && n.trim().bytes().all(|b| b.is_ascii_digit()))
            .then(|| n.trim().parse::<u64>().ok())
            .flatten()
    };
    let first = content.lines().find_map(row)?;
    let last = content.lines().rev().find_map(row)?;
    let footer = content.rfind("[showing lines ").and_then(|i| {
        let rest = &content[i + "[showing lines ".len()..];
        let (_, after) = rest.split_once(" of ")?;
        let digits: String = after.chars().take_while(char::is_ascii_digit).collect();
        digits.parse::<u64>().ok()
    });
    let total = footer.or_else(|| meta.get("lines_total").and_then(Value::as_u64));
    Some((first, last, total))
}
