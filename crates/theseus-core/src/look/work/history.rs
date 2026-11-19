//! `look.work`'s last part: this conversation's own messages, and no other's.

use std::fmt::Write;

use crate::node::{Body, Origin};
use crate::Core;

use super::{clip, LINE_CHARS};

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

pub(super) fn history(core: &Core, session: &str, messages: usize) -> String {
    let nodes = match core.store.session_nodes(session) {
        Ok(n) => n,
        Err(_) => return "Messages: they could not be read just now.\n".into(),
    };
    let mut lines: Vec<String> = nodes
        .iter()
        .filter_map(|(_, n)| match &n.body {
            Body::UserMessage { text, .. } if !text.trim().is_empty() => Some(format!(
                "{}: {}",
                who(n.origin, n.author.as_deref()),
                clip(text, LINE_CHARS)
            )),
            Body::AssistantMessage { blocks, model, .. } => {
                let text: String = blocks.iter().filter_map(|b| b["text"].as_str()).collect();
                (!text.trim().is_empty())
                    .then(|| format!("Theseus ({model}): {}", clip(&text, LINE_CHARS)))
            }
            // Another profile's answer, a message of its own (`profile.ask`).
            Body::ToolResult { meta, .. } => {
                theseus_protocol::SubagentAnswer::from_meta(meta).map(|a| {
                    format!(
                        "{} (a subagent, {}): {}",
                        a.profile,
                        a.model,
                        clip(&a.text, LINE_CHARS)
                    )
                })
            }
            _ => None,
        })
        .collect();
    let total = lines.len();
    let kept = lines.split_off(total.saturating_sub(messages));
    if kept.is_empty() {
        return "Messages: this conversation has none yet.\n".into();
    }
    let mut out = format!(
        "The last {} of this conversation's {total} messages:\n",
        kept.len()
    );
    for l in &kept {
        let _ = writeln!(out, "- {l}");
    }
    out
}
