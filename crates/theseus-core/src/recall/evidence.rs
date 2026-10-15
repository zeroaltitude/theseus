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
//!   rows, which a header does not read), so a message, a reply or a result
//!   gives its subject through its session's title, in the session part.
//! - **Its session**: the last six characters of its id, and its title (the
//!   first words of its first prompt, or the re-title), cut short.

use serde_json::Value;

use crate::node::{Body, Node};
use crate::session::SessionRecord;

/// The longest a subject or a title runs in a header, in characters.
const SHOWN: usize = 64;

/// The input fields a tool call's subject is read from, in order.
const TARGETS: [&str; 8] = [
    "path", "file", "command", "cmd", "url", "query", "name", "key",
];

/// `about fs.read src/relay.toml, session 7f3a21 "Relay migration"`; the
/// parts the node and its session do not have are left out, and a note with
/// neither has none.
pub fn evidence(n: &Node, session: Option<&SessionRecord>) -> Option<String> {
    let mut parts = Vec::new();
    if let Some(subject) = subject(n) {
        parts.push(format!("about {subject}"));
    }
    parts.push(match session.and_then(title) {
        Some(t) => format!("session {} \"{t}\"", short(&n.session_id)),
        None => format!("session {}", short(&n.session_id)),
    });
    Some(parts.join(", "))
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
fn title(s: &SessionRecord) -> Option<String> {
    let t = one_line(s.title.as_deref().or(s.label.as_deref())?);
    (!t.is_empty()).then_some(t)
}

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
    use theseus_protocol::SessionKind;

    fn session(id: &str, title: Option<&str>) -> SessionRecord {
        let mut s = SessionRecord::with_id(id.into(), SessionKind::Conversation, None);
        s.title = title.map(str::to_string);
        s
    }

    /// A message names its session; a tool call names its target too; a
    /// session with no title gives its id alone; a long or multi-line title
    /// is cut to one line; and nothing but the node and its session is read.
    #[test]
    fn an_evidence_line_names_the_subject_and_the_session() {
        let said = Node::user(
            "ses_ab12cd34ef",
            None,
            "cli",
            "the staging password is hunter2",
        );
        let s = session(
            "ses_ab12cd34ef",
            Some("Staging database\nsetup for the tide service"),
        );
        assert_eq!(
            evidence(&said, Some(&s)).as_deref(),
            Some("session cd34ef \"Staging database setup for the tide service\"")
        );
        let untitled = session("ses_ab12cd34ef", None);
        assert_eq!(
            evidence(&said, Some(&untitled)).as_deref(),
            Some("session cd34ef")
        );
        assert_eq!(evidence(&said, None).as_deref(), Some("session cd34ef"));
        let long = session("ses_ab12cd34ef", Some(&"harbour ".repeat(20)));
        let line = evidence(&said, Some(&long)).unwrap();
        assert!(line.ends_with("…\""), "{line}");
        assert!(line.chars().count() < 90, "{line}");
        let quoted = session("ses_ab12cd34ef", Some("the \"prod\" door"));
        assert_eq!(
            evidence(&said, Some(&quoted)).as_deref(),
            Some("session cd34ef \"the 'prod' door\"")
        );
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
        let s = session("ses_ab12cd34ef", Some("Weir gate"));
        assert_eq!(
            evidence(&call, Some(&s)).as_deref(),
            Some("about fs.read /srv/weir/gate.toml, session cd34ef \"Weir gate\"")
        );
        // The same inputs, the same bytes.
        assert_eq!(evidence(&call, Some(&s)), evidence(&call, Some(&s)));
    }
}
