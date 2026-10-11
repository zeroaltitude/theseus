//! The server's two memory tools (theseus-02i1h): `memory_search` and
//! `memory_get`, reads of the owner's private memory with no model call.
//! The server maps each to the core's one method, `memory.lookup`, which
//! answers only while `[mcp_server] memory` is on and never returns text
//! from a shared place; a refusal is an `isError` result the client reads.

use serde_json::{json, Map, Value};

/// The tools' definitions, for `tools/list`.
pub fn definitions() -> Vec<Value> {
    vec![
        json!({
            "name": "memory_search",
            "title": "Search what the owner did",
            "description": "Search Theseus's memory of the owner's past conversations and imported history by words, with an optional span (since, until), book or topic. Use it for a past decision, what was done when, or what a session held; quote each hit's citation (node_id, position, session_id, as_of) back when you rely on it.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "words": { "type": "string", "description": "What to find, as you would say it." },
                    "since": { "type": "string", "description": "First day or month of a span: \"2026-03-01\", \"March 2026\"." },
                    "until": { "type": "string", "description": "Last day or month of a span." },
                    "book": { "type": "string", "description": "An imported book." },
                    "topic": { "type": "string", "description": "An imported topic label." },
                    "limit": { "type": "integer", "minimum": 1, "maximum": 25, "description": "Hits a page, 8 by default." },
                    "cursor": { "type": "integer", "minimum": 0, "description": "Where the next page begins, as the last answer's next said." }
                },
                "required": ["words"]
            },
            "annotations": { "readOnlyHint": true, "idempotentHint": true, "openWorldHint": false }
        }),
        json!({
            "name": "memory_get",
            "title": "Read a remembered message in place",
            "description": "Read one remembered node by a citation's id with a few messages of its session before and after it, or one session's messages in order. Use it to see what surrounded a hit or what a whole session held; quote the citations it returns back.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "id": { "type": "string", "description": "A citation's node_id." },
                    "before": { "type": "integer", "minimum": 0, "maximum": 10, "description": "Messages before it, 2 by default." },
                    "after": { "type": "integer", "minimum": 0, "maximum": 10, "description": "Messages after it, 2 by default." },
                    "session": { "type": "string", "description": "A citation's session_id: read its messages in order instead of one node." },
                    "cursor": { "type": "integer", "minimum": 0, "description": "With session: where the next page begins." }
                }
            },
            "annotations": { "readOnlyHint": true, "idempotentHint": true, "openWorldHint": false }
        }),
    ]
}

/// A tool's arguments as the core's `memory.lookup` params: only the keys
/// the tool names, and the client's name for the ledger.
pub fn params(tool: &str, args: &Value, client: &str) -> Option<Value> {
    let (op, keys): (&str, &[&str]) = match tool {
        "memory_search" => (
            "search",
            &[
                "words", "since", "until", "book", "topic", "limit", "cursor",
            ],
        ),
        "memory_get" => ("get", &["id", "before", "after", "session", "cursor"]),
        _ => return None,
    };
    let mut p = Map::new();
    p.insert("op".into(), json!(op));
    for k in keys {
        if let Some(v) = args.get(*k).filter(|v| !v.is_null()) {
            p.insert((*k).into(), v.clone());
        }
    }
    p.insert("client".into(), json!(client));
    Some(Value::Object(p))
}
