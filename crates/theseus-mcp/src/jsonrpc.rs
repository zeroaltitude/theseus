//! JSON-RPC 2.0 as MCP carries it: one message per line on stdio, one per
//! POST (or per server-sent event) over HTTP.
//!
//! Incoming messages are classified by their fields, not by a tagged parse:
//! a peer's request id may be any string or integer, and a message that is
//! none of the three kinds is reported, never guessed at. Since 2025-06-18
//! nothing is batched; an array from a server on an older revision is read
//! as its messages in order.

use serde::{Deserialize, Serialize};
use serde_json::{json, Map, Value};

pub const JSONRPC: &str = "2.0";

/// JSON-RPC's own error codes, and the ones MCP uses.
pub mod code {
    pub const PARSE: i64 = -32700;
    pub const INVALID_REQUEST: i64 = -32600;
    pub const METHOD_NOT_FOUND: i64 = -32601;
    pub const INVALID_PARAMS: i64 = -32602;
    pub const INTERNAL: i64 = -32603;
}

/// An error object, as a response carries it.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RpcError {
    pub code: i64,
    #[serde(default)]
    pub message: String,
    #[serde(default, skip_serializing_if = "Value::is_null")]
    pub data: Value,
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

/// Classify one parsed value: a message, or a batch of them.
pub fn classify(v: Value) -> Vec<Result<Incoming, String>> {
    match v {
        Value::Array(items) if items.is_empty() => vec![Err("an empty batch".into())],
        Value::Array(items) => items.into_iter().map(classify_one).collect(),
        v => vec![classify_one(v)],
    }
}

fn classify_one(v: Value) -> Result<Incoming, String> {
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
                    Some(e) => format!("an error answer with no id: {}", clip(&e.to_string(), 300)),
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

/// At most `max` characters of `s`, cut at a character boundary.
pub fn clip(s: &str, max: usize) -> String {
    match s.char_indices().nth(max) {
        Some((i, _)) => format!("{}…", &s[..i]),
        None => s.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_three_kinds_and_the_strays() {
        let got = classify(json!({"jsonrpc": "2.0", "id": "a-1", "method": "ping"}));
        assert_eq!(
            got,
            vec![Ok(Incoming::Request {
                id: json!("a-1"),
                method: "ping".into(),
                params: Value::Null
            })]
        );
        let got = classify(json!({"jsonrpc": "2.0", "method": "notifications/tools/list_changed"}));
        assert!(
            matches!(&got[0], Ok(Incoming::Notification { method, .. }) if method == "notifications/tools/list_changed")
        );
        let got = classify(json!({"jsonrpc": "2.0", "id": 7, "result": {"tools": []}}));
        assert_eq!(
            got,
            vec![Ok(Incoming::Response {
                id: json!(7),
                outcome: Ok(json!({"tools": []}))
            })]
        );
        let got = classify(
            json!({"jsonrpc": "2.0", "id": 8, "error": {"code": -32602, "message": "Unknown tool"}}),
        );
        assert!(
            matches!(&got[0], Ok(Incoming::Response { outcome: Err(e), .. }) if e.code == -32602)
        );
        // A batch, from a 2025-03-26 server: each in order.
        let got = classify(json!([
            {"jsonrpc": "2.0", "method": "notifications/progress", "params": {"progress": 1}},
            {"jsonrpc": "2.0", "id": 9, "result": {}}
        ]));
        assert_eq!(got.len(), 2);
        assert!(matches!(&got[1], Ok(Incoming::Response { .. })));
        // Strays: reported, never guessed at.
        assert!(classify(json!("hello"))[0].is_err());
        assert!(classify(json!([]))[0].is_err());
        assert!(classify(json!({"jsonrpc": "2.0"}))[0].is_err());
        assert!(
            classify(json!({"jsonrpc": "2.0", "id": null, "error": {"code": -32700}}))[0]
                .as_ref()
                .is_err_and(|e| e.contains("no id"))
        );
        assert!(classify(json!({"jsonrpc": "2.0", "id": 1, "method": 5}))[0].is_err());
    }

    #[test]
    fn ids_and_params_on_the_way_out() {
        assert_eq!(
            request(3, "tools/list", Value::Null),
            json!({"jsonrpc": "2.0", "id": 3, "method": "tools/list"})
        );
        assert_eq!(
            notification("notifications/initialized", Value::Null),
            json!({"jsonrpc": "2.0", "method": "notifications/initialized"})
        );
        assert_eq!(id_number(&json!(12)), Some(12));
        assert_eq!(id_number(&json!("12")), Some(12));
        assert_eq!(id_number(&json!("x")), None);
        assert_eq!(clip("abcdef", 3), "abc…");
        assert_eq!(clip("abc", 3), "abc");
        assert_eq!(clip("ééé", 2), "éé…");
    }
}
