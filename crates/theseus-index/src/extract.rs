//! The extractor (M6 §2.2's table): what of a node is indexed, and what
//! never is.
//!
//! | Source | Indexed text | Never |
//! |---|---|---|
//! | `UserMessage` | text, and text attachments (and every attachment's name) | image bytes |
//! | `AssistantMessage` | text blocks | thinking blocks, and the `tool_use` blocks (their `ToolCall` nodes say what is indexed of them) |
//! | `ToolResult` | the content the model saw (already scrubbed and capped), with its `external` flag | the spool's full output, an image |
//! | `ToolCall` | `proc.run`'s argv, and the paths and queries of reads and searches | other inputs |
//! | `Summary` | its text | its testimony header |
//! | `Synthesis`, `Lesson` | text (once they exist) | |
//! | `Recall` | | always: recalled text is never indexed again (§5.2) |
//!
//! It reads a node's record as a JSON value, so the tender never depends on
//! `theseus-core`, and a change there never rebuilds the search engine. This
//! crate's tests hold it to every `Body` variant with an exhaustive match, so
//! a new variant fails the build until it is given a row here.
//!
//! **Versioned.** A change to what is indexed bumps [`EXTRACTOR_VERSION`];
//! an index written by another version is rebuilt from the WAL.

use serde_json::Value;

/// Bumped when what is indexed of a node changes.
pub const EXTRACTOR_VERSION: u32 = 1;

/// What a node gives the index, before chunking.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Extracted {
    pub node_id: String,
    pub session_id: String,
    /// The body's kind: `user_message`, `assistant_message`, `tool_call`,
    /// `tool_result`, or a later one with text.
    pub kind: String,
    /// `operator`, `agent`, `tool`, or `harness`.
    pub origin: String,
    pub author: Option<String>,
    pub created_at_ms: u64,
    /// The text came from outside (`ToolResult.external`, DD5).
    pub external: bool,
    /// The tool of a call or a result.
    pub tool: Option<String>,
    pub text: String,
}

/// The extractor's answer for one node record.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Extract {
    Index(Extracted),
    /// Nothing of it is indexed, and why. A re-ingest still deletes any
    /// earlier copy by its id.
    Skip {
        node_id: String,
        why: &'static str,
    },
}

#[derive(Debug, thiserror::Error)]
pub enum ExtractError {
    #[error("not a node: {0}")]
    NotJson(#[from] serde_json::Error),
    #[error("not a node: no {0}")]
    Missing(&'static str),
}

/// The tools whose inputs are reads and searches, and the fields of each
/// that are indexed. Every other tool's input is never indexed.
const READS_AND_SEARCHES: &[(&str, &[&str])] = &[
    ("fs.read", &["path"]),
    ("fs.list", &["path"]),
    ("fs.glob", &["pattern", "path"]),
    ("fs.grep", &["pattern", "path", "glob"]),
    ("git.log", &["path", "rev", "file"]),
    ("git.diff", &["path", "rev", "paths"]),
    ("web.search", &["query"]),
    ("http.fetch", &["url"]),
];

fn str_of<'a>(v: &'a Value, key: &str) -> Option<&'a str> {
    v.get(key).and_then(Value::as_str)
}

/// Extract a node record's payload (kind `NODE`).
pub fn extract(payload: &[u8]) -> Result<Extract, ExtractError> {
    let node: Value = serde_json::from_slice(payload)?;
    let node_id = str_of(&node, "id").ok_or(ExtractError::Missing("id"))?;
    let session_id = str_of(&node, "session_id").ok_or(ExtractError::Missing("session_id"))?;
    let body = node.get("body").ok_or(ExtractError::Missing("body"))?;
    let kind = str_of(body, "kind").ok_or(ExtractError::Missing("body.kind"))?;
    let skip = |why| {
        Ok(Extract::Skip {
            node_id: node_id.to_string(),
            why,
        })
    };
    let mut external = false;
    let mut tool = None;
    let text = match kind {
        "user_message" => user_text(body),
        "assistant_message" => assistant_text(body),
        "tool_call" => {
            let name = str_of(body, "tool").unwrap_or_default();
            tool = Some(name.to_string());
            let Some(text) = call_text(name, body.get("input").unwrap_or(&Value::Null)) else {
                return skip("a tool call whose input is never indexed");
            };
            text
        }
        "tool_result" => {
            tool = str_of(body, "tool").map(str::to_string);
            external = body.get("external").is_some_and(|e| !e.is_null());
            str_of(body, "content").unwrap_or_default().to_string()
        }
        "recall" => return skip("recalled text is never indexed again"),
        // A later body (`summary`, `synthesis`, `lesson`): its text.
        _ => match str_of(body, "text") {
            Some(t) => t.to_string(),
            None => return skip("a body kind this extractor does not know, with no text"),
        },
    };
    if text.trim().is_empty() {
        return skip("no text");
    }
    Ok(Extract::Index(Extracted {
        node_id: node_id.to_string(),
        session_id: session_id.to_string(),
        kind: kind.to_string(),
        origin: str_of(&node, "origin").unwrap_or_default().to_string(),
        author: str_of(&node, "author").map(str::to_string),
        created_at_ms: node
            .get("created_at_ms")
            .and_then(Value::as_u64)
            .unwrap_or(0),
        external,
        tool,
        text,
    }))
}

/// The operator's text, then each attachment's name, and the text of a
/// text attachment. An image is its name only: its bytes live in the blobs,
/// and its digest is no text.
fn user_text(body: &Value) -> String {
    let mut parts = vec![str_of(body, "text").unwrap_or_default().to_string()];
    for a in body
        .get("attachments")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
    {
        if let Some(name) = str_of(a, "name") {
            parts.push(name.to_string());
        }
        let content = a.get("content").unwrap_or(&Value::Null);
        if str_of(content, "kind") == Some("text") {
            parts.push(str_of(content, "text").unwrap_or_default().to_string());
        }
    }
    join(parts)
}

/// The model's text blocks; never its thinking, and never its `tool_use`
/// blocks (each has its own `ToolCall` node).
fn assistant_text(body: &Value) -> String {
    join(
        body.get("blocks")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
            .filter(|b| str_of(b, "type") == Some("text"))
            .filter_map(|b| str_of(b, "text"))
            .map(str::to_string)
            .collect(),
    )
}

/// What of a tool call's input is indexed: `proc.run`'s argv, and the
/// paths and queries of reads and searches. `None` for any other tool.
fn call_text(tool: &str, input: &Value) -> Option<String> {
    if tool == "proc.run" {
        let argv: Vec<String> = input
            .get("argv")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
            .filter_map(Value::as_str)
            .map(str::to_string)
            .collect();
        return Some(argv.join(" "));
    }
    let (_, fields) = READS_AND_SEARCHES.iter().find(|(t, _)| *t == tool)?;
    let mut parts = Vec::new();
    for f in *fields {
        match input.get(*f) {
            Some(Value::String(s)) => parts.push(s.clone()),
            Some(Value::Array(a)) => {
                parts.extend(a.iter().filter_map(Value::as_str).map(str::to_string));
            }
            _ => {}
        }
    }
    Some(join(parts))
}

fn join(parts: Vec<String>) -> String {
    parts
        .into_iter()
        .filter(|p| !p.trim().is_empty())
        .collect::<Vec<_>>()
        .join("\n\n")
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn node(body: Value) -> Vec<u8> {
        serde_json::to_vec(&json!({
            "id": "msg_1", "schema": 1, "session_id": "ses_1", "origin": "operator",
            "author": "cli", "created_at_ms": 7, "body": body,
        }))
        .unwrap()
    }

    fn text(body: Value) -> String {
        match extract(&node(body)).unwrap() {
            Extract::Index(e) => e.text,
            Extract::Skip { why, .. } => panic!("skipped: {why}"),
        }
    }

    #[test]
    fn a_user_message_gives_its_text_and_its_text_attachments_never_an_image() {
        let t = text(
            json!({"kind": "user_message", "text": "see these", "attachments": [
                {"name": "notes.txt", "media_type": "text/plain", "size": 5,
                 "content": {"kind": "text", "text": "port 7433"}},
                {"name": "shot.png", "media_type": "image/png", "size": 9,
                 "content": {"kind": "image", "digest": "sha256-abc", "width": 1, "height": 1}},
            ]}),
        );
        assert_eq!(t, "see these\n\nnotes.txt\n\nport 7433\n\nshot.png");
        assert!(!t.contains("sha256"));
    }

    #[test]
    fn an_assistant_message_gives_its_text_blocks_never_its_thinking() {
        let t = text(
            json!({"kind": "assistant_message", "model": "m", "provider": "p", "blocks": [
                {"type": "thinking", "thinking": "SECRET PLAN", "signature": "sig"},
                {"type": "redacted_thinking", "data": "xyz"},
                {"type": "text", "text": "Reading it."},
                {"type": "tool_use", "id": "t1", "name": "fs_read", "input": {"path": "/etc/hosts"}},
                {"type": "text", "text": "Done."},
            ]}),
        );
        assert_eq!(t, "Reading it.\n\nDone.");
    }

    #[test]
    fn a_tool_call_gives_argv_and_the_paths_and_queries_of_reads_never_other_inputs() {
        let call = |tool: &str, input: Value| {
            extract(&node(
                json!({"kind": "tool_call", "tool_use_id": "t", "tool": tool,
                "wire_name": "w", "input": input, "assistant_node": "msg_0"}),
            ))
            .unwrap()
        };
        let indexed = |e: Extract| match e {
            Extract::Index(e) => e.text,
            Extract::Skip { why, .. } => panic!("{why}"),
        };
        assert_eq!(
            indexed(call(
                "proc.run",
                json!({"argv": ["cargo", "test", "-p", "theseus-store"],
                "env": {"X": "never"}})
            )),
            "cargo test -p theseus-store"
        );
        assert_eq!(
            indexed(call("fs.read", json!({"path": "src/wal.rs", "offset": 3}))),
            "src/wal.rs"
        );
        assert_eq!(
            indexed(call(
                "fs.grep",
                json!({"pattern": "fn open", "path": "crates"})
            )),
            "fn open\n\ncrates"
        );
        assert_eq!(
            indexed(call("web.search", json!({"query": "tantivy as_of"}))),
            "tantivy as_of"
        );
        assert_eq!(
            indexed(call(
                "git.diff",
                json!({"rev": "HEAD~1", "paths": ["a.rs", "b.rs"]})
            )),
            "HEAD~1\n\na.rs\n\nb.rs"
        );
        // A write's content, an edit's strings: never.
        for (tool, input) in [
            ("fs.write", json!({"path": "a.txt", "content": "private"})),
            (
                "fs.edit",
                json!({"path": "a.txt", "old_string": "x", "new_string": "y"}),
            ),
            ("task.create", json!({"brief": "do it"})),
        ] {
            assert!(matches!(call(tool, input), Extract::Skip { .. }), "{tool}");
        }
    }

    #[test]
    fn a_tool_result_gives_its_content_and_its_external_flag() {
        let e = extract(&node(
            json!({"kind": "tool_result", "tool_use_id": "t", "tool": "http.fetch",
            "status": "ok", "is_error": false, "content": "fetched text",
            "external": {"url": "x", "host": "example.org"}, "full_ref": "spool/1"}),
        ))
        .unwrap();
        let Extract::Index(e) = e else { panic!() };
        assert_eq!(e.text, "fetched text");
        assert!(e.external);
        assert_eq!(e.tool.as_deref(), Some("http.fetch"));
        let e = extract(&node(
            json!({"kind": "tool_result", "tool_use_id": "t", "tool": "fs.read",
            "status": "ok", "is_error": false, "content": "local"}),
        ))
        .unwrap();
        let Extract::Index(e) = e else { panic!() };
        assert!(!e.external);
    }

    #[test]
    fn a_recall_is_never_indexed_and_a_later_body_gives_its_text() {
        assert!(matches!(
            extract(&node(json!({"kind": "recall", "text": "an old fact"}))).unwrap(),
            Extract::Skip { .. }
        ));
        assert_eq!(
            text(json!({"kind": "summary", "text": "the gist"})),
            "the gist"
        );
        assert!(matches!(
            extract(&node(json!({"kind": "someday", "items": [1]}))).unwrap(),
            Extract::Skip { .. }
        ));
    }

    #[test]
    fn a_node_without_text_is_skipped_and_garbage_is_an_error() {
        assert!(matches!(
            extract(&node(json!({"kind": "user_message", "text": "  "}))).unwrap(),
            Extract::Skip { .. }
        ));
        assert!(extract(b"not json").is_err());
        assert!(extract(br#"{"id": "x"}"#).is_err());
    }
}
