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

/// A stored header as it is emitted (issue 44): one an older factory froze with its session's title
/// (`session <short id> "<title>"`, `one_line` keeping any quote mark out of the title) is cut after
/// `session <short id>`, the rest dropped. Any other header is as it was stored. No other parsing, and no
/// stored note is rewritten.
pub fn untitled<'a>(header: &'a str, session: &str) -> &'a str {
    let old = format!("session {} \"", short(session));
    match header.find(&old) {
        Some(at) => &header[..at + old.len() - " \"".len()],
        None => header,
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
    let (cut, _) = crate::scrub::safe::head(&flat, SHOWN - 1);
    format!("{}…", cut.trim_end())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A message names its session by its id alone: no title, so no words of
    /// any message (issue 44); a long or multi-line subject is cut to one
    /// line, its quote marks made plain; and nothing but the node is read.
    #[test]
    fn an_evidence_line_names_the_subject_and_the_session() {
        let said = Node::user(
            "ses_ab12cd34ef",
            None,
            "cli",
            "the staging password is hunter2",
        );
        assert_eq!(evidence(&said), "session cd34ef");
        let other = Node::user("ses_99ff00aa11", None, "cli", "Staging database");
        assert_eq!(evidence(&other), "session 00aa11");
        let call = |target: &str| {
            let mut n = Node::user("ses_ab12cd34ef", None, "cli", "");
            n.body = Body::ToolCall {
                tool_use_id: "tu1".into(),
                tool: "web.search".into(),
                wire_name: "web_search".into(),
                input: serde_json::json!({ "query": target }),
                assistant_node: "msg_a".into(),
                correlation_id: None,
                gate: None,
            };
            n
        };
        assert_eq!(
            evidence(&call("tide tables")),
            "about web.search tide tables, session cd34ef"
        );
        let long = evidence(&call(&"harbour\nwall ".repeat(20)));
        assert!(long.ends_with("…, session cd34ef"), "{long}");
        assert!(long.chars().count() < 110 && !long.contains('\n'), "{long}");
        assert_eq!(
            evidence(&call("the \"prod\" door")),
            "about web.search the 'prod' door, session cd34ef"
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
        assert_eq!(
            evidence(&call),
            "about fs.read /srv/weir/gate.toml, session cd34ef"
        );
        // The same inputs, the same bytes.
        assert_eq!(evidence(&call), evidence(&call));
    }

    /// A header frozen before the title went (issue 44) is emitted cut after its session's short id: the
    /// title's words are in no rendered text, what came before them is; a header of today is as stored.
    #[test]
    fn an_old_frozen_header_is_emitted_without_its_title() {
        let header = "[m:a1b2c3] a message from the owner in local, said Sun 2026-11-22 08:00 MST \
                      (as of @7), session cd34ef \"KESTREL-MARKER plans\", volatile: as of 2026-11-22";
        let r = crate::node::RecalledRef {
            node_id: "msg_a1b2c3".into(),
            session_id: "ses_ab12cd34ef".into(),
            position: 7,
            chunk: (0, 9),
            header: header.into(),
            tokens: 3,
        };
        let said = Node::user("ses_ab12cd34ef", None, "cli", "the words");
        let shown = crate::recall::render::item(&r, Some(&said));
        assert!(!shown.contains("KESTREL-MARKER"), "{shown}");
        assert!(
            shown.starts_with("[m:a1b2c3] a message from the owner in local, said Sun 2026-11-22 08:00 MST (as of @7), session cd34ef\n"),
            "{shown}"
        );
        let today = "[m:a1b2c3] a message, session cd34ef, volatile: as of 2026-11-22";
        assert_eq!(untitled(today, "ses_ab12cd34ef"), today);
        assert_eq!(
            untitled(header, "ses_99ff00aa11"),
            header,
            "another session's id cuts nothing"
        );
    }
}
