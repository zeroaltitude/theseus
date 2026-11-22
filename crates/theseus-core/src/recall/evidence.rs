//! A recalled note's evidence line (theseus-55hlu): what the note is about
//! and which conversation it came from, so the model can tell two notes of
//! one kind apart and does not take a near one for the answer (the recall
//! bench's abstention probes: it answered with the distractor's value in 35
//! of 60).
//!
//! Computed from the node and its session's record alone, never the clock
//! or the question, so a header rendered twice is the same bytes (the
//! header is frozen with its item, and a cached prefix holds).
//!
//! - **Its subject**, when the node names one: a tool call's target (its
//!   path, command, address, query or name). A node has no fact key or
//!   entity of its own (the memory pass keeps entities in `memory.labeled`
//!   rows, which a header does not read).
//! - **Its session**: the last six characters of its id. Never its title or
//!   label (issue 44, the conductor under the owner's privacy word): a
//!   title is a message's own words (the first prompt, or the re-title's),
//!   and a message moved into a sealed thread would ride in the header of
//!   every other item recalled from its session, frozen in the note. The
//!   header's place and time name the session too.

use serde_json::Value;

use crate::node::{Body, Node};

/// The longest a subject runs in a header, in characters.
const SHOWN: usize = 64;

/// The input fields a tool call's subject is read from, in order.
const TARGETS: [&str; 8] = [
    "path", "file", "command", "cmd", "url", "query", "name", "key",
];

/// `about fs.read src/relay.toml, session 7f3a21`; a node with no subject
/// gives its session alone.
pub fn evidence(n: &Node) -> String {
    let session = format!("session {}", short(&n.session_id));
    match subject(n) {
        Some(subject) => format!("about {subject}, {session}"),
        None => session,
    }
}

/// The last six characters of a session's id, without its `ses_`.
pub fn short(id: &str) -> &str {
    let id = id.strip_prefix("ses_").unwrap_or(id);
    let from = id.char_indices().rev().nth(5).map_or(0, |(i, _)| i);
    &id[from..]
}

/// What a tool call is about: its tool and the first target its input names.
fn subject(n: &Node) -> Option<String> {
    let Body::ToolCall { tool, input, .. } = &n.body else {
        return None;
    };
    let target = TARGETS
        .iter()
        .find_map(|k| input.get(k).and_then(Value::as_str))
        .map(one_line)?;
    Some(format!("{tool} {target}"))
}

/// The session's title, on one line and cut.
/// `text` as one line of at most `SHOWN` characters, `…` where it was cut,
/// and no quote mark to end the header's own early.
fn one_line(text: &str) -> String {
    let flat: String = text
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .replace('"', "'");
    if flat.chars().count() <= SHOWN {
        return flat;
    }
    let cut: String = flat.chars().take(SHOWN - 1).collect();
    format!("{}…", cut.trim_end())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A message names its session by its id alone: no title, so no words of
    /// any message (issue 44); and nothing but the node is read.
    #[test]
    fn an_evidence_line_names_the_subject_and_the_session() {
        let said = Node::user(
            "ses_ab12cd34ef",
            None,
            "cli",
            "the staging password is hunter2",
        );
        assert_eq!(evidence(&said), "session cd34ef");
    }

    #[test]
    fn a_tool_call_is_about_its_target() {
        let mut call = Node::user("ses_ab12cd34ef", None, "cli", "");
        call.body = Body::ToolCall {
            tool_use_id: "tu1".into(),
            tool: "fs.read".into(),
            wire_name: "fs_read".into(),
            input: serde_json::json!({"path": "/srv/weir/gate.toml", "limit": 40}),
            assistant_node: "msg_a".into(),
            correlation_id: None,
            gate: None,
        };
        assert_eq!(
            evidence(&call),
            "about fs.read /srv/weir/gate.toml, session cd34ef"
        );
        // The same inputs, the same bytes.
        assert_eq!(evidence(&call), evidence(&call));
    }
}
