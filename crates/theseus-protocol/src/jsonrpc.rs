//! JSON-RPC 2.0 framing for the peers Theseus talks to (LSP servers, MCP
//! servers), once: the builders for the four message kinds and the reader that
//! classifies what comes back by its fields. A peer's request id may be a
//! string or an integer, and a message that is none of the three kinds is
//! reported, never guessed at. Theseus's own protocol is typed (`Request`,
//! `Response`); this is for the two that are not.

use crate::RpcError;
use serde_json::{json, Map, Value};

pub const JSONRPC: &str = "2.0";

/// JSON-RPC's own error codes.
pub mod code {
    pub const PARSE: i64 = -32700;
    pub const INVALID_REQUEST: i64 = -32600;
    pub const METHOD_NOT_FOUND: i64 = -32601;
    pub const INVALID_PARAMS: i64 = -32602;
    pub const INTERNAL: i64 = -32603;
}

/// One message from the peer, classified.
#[derive(Debug, Clone, PartialEq)]
pub enum Incoming {
    /// A request to us: its answer carries the same `id`.
    Request {
        id: Value,
        method: String,
        params: Value,
    },
    Notification {
        method: String,
        params: Value,
    },
    /// An answer to one of our requests.
    Response {
        id: Value,
        outcome: Result<Value, RpcError>,
    },
}

/// Classify one parsed value: a message, or (MCP before 2025-06-18) a batch of
/// them, each in order.
pub fn classify_batch(v: Value) -> Vec<Result<Incoming, String>> {
    match v {
        Value::Array(items) if items.is_empty() => vec![Err("an empty batch".into())],
        Value::Array(items) => items.into_iter().map(classify).collect(),
        v => vec![classify(v)],
    }
}

/// Classify one parsed message.
pub fn classify(v: Value) -> Result<Incoming, String> {
    let Value::Object(mut m) = v else {
        return Err("a message that is not a JSON object".into());
    };
    let id = m.remove("id").filter(|id| id.is_string() || id.is_number());
    match m.remove("method") {
        Some(Value::String(method)) => {
            let params = m.remove("params").unwrap_or(Value::Null);
            Ok(match id {
                Some(id) => Incoming::Request { id, method, params },
                None => Incoming::Notification { method, params },
            })
        }
        Some(_) => Err("a message whose method is not a string".into()),
        None => {
            // A response to a request the peer could not read carries a null
            // id: there is no call to give it to.
            let Some(id) = id else {
                return Err(match m.get("error") {
                    Some(e) => format!(
                        "an error answer with no id: {}",
                        crate::term::words::cut_marked(&e.to_string(), 300)
                    ),
                    None => "a message with neither a method nor an id".into(),
                });
            };
            let outcome = match m.remove("error") {
                Some(e) => {
                    Err(
                        serde_json::from_value::<RpcError>(e).unwrap_or_else(|e| RpcError {
                            code: code::INTERNAL,
                            message: format!("an unreadable error object: {e}"),
                            data: Value::Null,
                        }),
                    )
                }
                None => Ok(m.remove("result").unwrap_or(Value::Null)),
            };
            Ok(Incoming::Response { id, outcome })
        }
    }
}

/// A request. `params` is left out when it is null.
pub fn request(id: impl Into<Value>, method: &str, params: Value) -> Value {
    let mut m = Map::new();
    m.insert("jsonrpc".into(), JSONRPC.into());
    m.insert("id".into(), id.into());
    m.insert("method".into(), method.into());
    if !params.is_null() {
        m.insert("params".into(), params);
    }
    Value::Object(m)
}

/// A notification. `params` is left out when it is null.
pub fn notification(method: &str, params: Value) -> Value {
    let mut m = Map::new();
    m.insert("jsonrpc".into(), JSONRPC.into());
    m.insert("method".into(), method.into());
    if !params.is_null() {
        m.insert("params".into(), params);
    }
    Value::Object(m)
}

/// A success answer. A `null` result is sent as `null`, not left out.
pub fn response(id: Value, result: Value) -> Value {
    json!({ "jsonrpc": JSONRPC, "id": id, "result": result })
}

pub fn error_response(id: Value, code: i64, message: &str) -> Value {
    json!({ "jsonrpc": JSONRPC, "id": id, "error": { "code": code, "message": message } })
}

/// Our own request ids are integers; a peer that echoes one as a string of
/// digits is still matched.
pub fn id_number(id: &Value) -> Option<i64> {
    match id {
        Value::Number(n) => n.as_i64(),
        Value::String(s) => s.parse().ok(),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_builders_write_the_bytes_the_lsp_and_mcp_copies_wrote() {
        let wire = |v: Value| serde_json::to_string(&v).unwrap();
        assert_eq!(
            wire(request(3, "shutdown", Value::Null)),
            r#"{"id":3,"jsonrpc":"2.0","method":"shutdown"}"#
        );
        assert_eq!(
            wire(request("r-1", "tools/call", json!({"name": "x"}))),
            r#"{"id":"r-1","jsonrpc":"2.0","method":"tools/call","params":{"name":"x"}}"#
        );
        assert_eq!(
            wire(notification("exit", Value::Null)),
            r#"{"jsonrpc":"2.0","method":"exit"}"#
        );
        assert_eq!(
            wire(notification("initialized", json!({}))),
            r#"{"jsonrpc":"2.0","method":"initialized","params":{}}"#
        );
        assert_eq!(
            wire(response(json!("s-1"), Value::Null)),
            r#"{"id":"s-1","jsonrpc":"2.0","result":null}"#
        );
        assert_eq!(
            wire(error_response(
                json!(7),
                code::METHOD_NOT_FOUND,
                "no such method"
            )),
            r#"{"error":{"code":-32601,"message":"no such method"},"id":7,"jsonrpc":"2.0"}"#
        );
        let batch = json!([
            request(1, "ping", Value::Null),
            notification("notifications/progress", json!({"progress": 1}))
        ]);
        assert_eq!(
            wire(batch),
            r#"[{"id":1,"jsonrpc":"2.0","method":"ping"},{"jsonrpc":"2.0","method":"notifications/progress","params":{"progress":1}}]"#
        );
    }

    #[test]
    fn a_batch_is_read_in_order_and_an_empty_one_is_reported() {
        let got = classify_batch(json!([
            {"jsonrpc": "2.0", "method": "notifications/progress", "params": {"progress": 1}},
            {"jsonrpc": "2.0", "id": 9, "result": {}}
        ]));
        assert_eq!(got.len(), 2);
        assert!(matches!(&got[0], Ok(Incoming::Notification { .. })));
        assert!(matches!(&got[1], Ok(Incoming::Response { .. })));
        assert!(classify_batch(json!([]))[0].is_err());
        assert_eq!(classify_batch(json!({"id": 1, "method": "ping"})).len(), 1);
    }

    #[test]
    fn an_error_object_with_no_message_still_reads() {
        let got = classify(json!({"jsonrpc": "2.0", "id": 9, "error": {"code": -32000}}));
        assert!(
            matches!(&got, Ok(Incoming::Response { outcome: Err(e), .. }) if e.code == -32000 && e.message.is_empty())
        );
    }
}
