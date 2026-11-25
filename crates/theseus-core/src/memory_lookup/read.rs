//! What the MCP server's `memory_get` reads of a session's messages (`readable_at`), and how a time and a
//! message's words are said. `memory.lookup`'s own reads by session and its span listing went with that tool
//! (D6 step 6).

use super::day;
use crate::node::{Body, Node};
use crate::recall::when::When;

/// A message's text at most, in tokens, before it is cut.
pub(super) const MESSAGE_MAX: u64 = 1_500;

/// A session's messages a reader reads, in `when` when it is given, each with its WAL position (the MCP tools
/// cite it), and what it leaves out, said: tool calls and results, and an imported message of outside text.
pub(super) fn readable_at<'n>(
    nodes: &'n [(u64, Node)],
    when: Option<&When>,
) -> (Vec<(u64, &'n Node)>, Option<String>) {
    let (mut calls, mut outside) = (0u64, 0u64);
    let messages = nodes
        .iter()
        .map(|(p, n)| (*p, n))
        .filter(|(_, n)| when.is_none_or(|w| w.holds(n.created_at_ms)))
        .filter(|(_, n)| match &n.body {
            Body::UserMessage { .. } | Body::AssistantMessage { .. } => true,
            Body::Imported { integrity, .. } => {
                let out = *integrity == crate::import::Integrity::Outside;
                outside += u64::from(out);
                !out
            }
            Body::ToolCall { .. } | Body::ToolResult { .. } => {
                calls += 1;
                false
            }
            _ => false,
        })
        .filter(|(_, n)| !crate::recall::text_of(n).trim().is_empty())
        .collect();
    let mut left = Vec::new();
    if calls > 0 {
        left.push(crate::narrative::count(
            calls,
            "tool call or result",
            "tool calls and results",
        ));
    }
    if outside > 0 {
        left.push(crate::narrative::count(
            outside,
            "message of outside text",
            "messages of outside text",
        ));
    }
    (messages, (!left.is_empty()).then(|| left.join(" and ")))
}

/// A time as a page says it: `2026-03-14 09:30`, in the daemon's zone.
pub(super) fn moment(ms: u64) -> String {
    let off = crate::wake::local(ms).offset_secs;
    let secs = (ms / 1000) as i64 + off;
    let (h, m) = (secs.rem_euclid(86_400) / 3600, secs.rem_euclid(3600) / 60);
    format!("{} {h:02}:{m:02}", day(ms))
}

pub(super) fn one_line(s: &str) -> String {
    s.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// `s` cut to `max` bytes at a char's edge, saying how much more it held.
pub(super) fn clipped(s: &str, max: usize) -> String {
    if s.len() <= max {
        return s.to_string();
    }
    let (kept, _) = crate::scrub::safe::head_bytes(s, max);
    format!(
        "{kept} […{} more characters: ask with a larger budget_tokens, up to 6000, to read more of it]",
        s[kept.len()..].chars().count()
    )
}
