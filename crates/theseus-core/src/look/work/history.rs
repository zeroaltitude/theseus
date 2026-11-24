//! `look.work`'s last part: this conversation's own messages, and no other's.

use std::fmt::Write;

use crate::node::{Body, Node, Origin};
use crate::scrub::Cuts;
use crate::Core;

use super::{clip_kept, LINE_CHARS};

/// Who said a message, in words that are true of it. "The harness" is for its own words alone
/// (`content::harness_own`); a task's report, a wake's note and a retraction's gist carry words from
/// elsewhere and are named as what they are.
pub(super) fn who(origin: Origin, author: Option<&str>) -> &'static str {
    match origin {
        Origin::Operator => "the owner",
        Origin::Harness if crate::content::harness_own(origin, author) => "the harness",
        Origin::Harness => match author.unwrap_or("") {
            a if a.starts_with("task:") => "a task's report",
            a if a.starts_with("wake:") => "a wake's note",
            _ => "a note that carries other words",
        },
        _ => "a message",
    }
}

/// A message as the name it goes under and its words, clipped at a known value's edge (`cuts`), and whether
/// the clip kept any of its bytes; none for a node that is not a message.
pub(super) fn line_of(n: &Node, cuts: &Cuts) -> Option<(String, String, bool)> {
    let said = |name: String, text: &str| {
        let (words, kept) = clip_kept(text, LINE_CHARS, cuts);
        (name, words, !kept.is_empty())
    };
    match &n.body {
        Body::UserMessage { text, .. } if !text.trim().is_empty() => Some(said(
            format!("{}: ", who(n.origin, n.author.as_deref())),
            text.as_str(),
        )),
        Body::AssistantMessage { blocks, model, .. } => {
            let text: String = blocks.iter().filter_map(|b| b["text"].as_str()).collect();
            (!text.trim().is_empty()).then(|| said(format!("Theseus ({model}): "), text.as_str()))
        }
        // Another profile's answer, a message of its own (`profile.ask`).
        Body::ToolResult { meta, .. } => {
            theseus_protocol::SubagentAnswer::from_meta(meta).map(|a| {
                said(
                    format!("{} (a subagent, {}): ", a.profile, a.model),
                    a.text.as_str(),
                )
            })
        }
        _ => None,
    }
}

/// The part's words, and the messages they show (what its result copies, `copies`): each with where its words
/// are in the part, in characters, `from` to `to`. A message the clip keeps none of (control characters alone,
/// or a known value from its start) shows none, and is not one of them (D6 round 6).
pub(super) fn history(
    core: &Core,
    session: &str,
    messages: usize,
    cuts: &Cuts,
) -> (String, Vec<(String, usize, usize)>) {
    let nodes = match core.store.session_nodes(session) {
        Ok(n) => n,
        Err(_) => {
            return (
                "Messages: they could not be read just now.\n".into(),
                Vec::new(),
            )
        }
    };
    // Each line as the name it goes under and its words, apart, so where its words are is known.
    let mut lines: Vec<(String, String, bool, String)> = nodes
        .iter()
        .filter_map(|(_, n)| {
            line_of(n, cuts).map(|(name, words, kept)| (name, words, kept, n.id.clone()))
        })
        .collect();
    let total = lines.len();
    let kept = lines.split_off(total.saturating_sub(messages));
    if kept.is_empty() {
        return (
            "Messages: this conversation has none yet.\n".into(),
            Vec::new(),
        );
    }
    let mut out = format!(
        "The last {} of this conversation's {total} messages:\n",
        kept.len()
    );
    let mut shown = Vec::new();
    for (name, words, some, id) in kept {
        let _ = write!(out, "- {name}");
        let from = out.chars().count();
        out.push_str(&words);
        if some {
            shown.push((id, from, from + words.chars().count()));
        }
        out.push('\n');
    }
    (out, shown)
}
