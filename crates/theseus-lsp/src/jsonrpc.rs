//! JSON-RPC 2.0 as LSP carries it: one message per frame, never batched.
//!
//! Incoming messages are classified by their fields, as theseus-mcp's are: a
//! server's own request ids may be strings or integers, and a message that is
//! none of the three kinds is reported, never guessed at.

use serde::{Deserialize, Serialize};
use serde_json::{json, Map, Value};

pub const JSONRPC: &str = "2.0";

/// JSON-RPC's error codes, and LSP's own.
pub mod code {
    pub const PARSE: i64 = -32700;
    pub const INVALID_REQUEST: i64 = -32600;
    pub const METHOD_NOT_FOUND: i64 = -32601;
    pub const INVALID_PARAMS: i64 = -32602;
    pub const INTERNAL: i64 = -32603;
    /// The server got a request before `initialize`.
    pub const SERVER_NOT_INITIALIZED: i64 = -32002;
    /// The document changed under the request: asking again may succeed.
    pub const CONTENT_MODIFIED: i64 = -32801;
    /// The server cancelled the request itself (pull diagnostics: ask again
    /// when `retriggerRequest` is true).
    pub const SERVER_CANCELLED: i64 = -32802;
    /// The client cancelled it with `$/cancelRequest`.
    pub const REQUEST_CANCELLED: i64 = -32800;
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

/// A success answer. LSP's `null` results are sent as `null`, not left out.
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
        let got = classify(
            json!({"jsonrpc": "2.0", "id": "c-1", "method": "workspace/configuration", "params": {"items": []}}),
        );
        assert!(
            matches!(&got, Ok(Incoming::Request { id, method, .. }) if id == "c-1" && method == "workspace/configuration")
        );
        let got =
            classify(json!({"jsonrpc": "2.0", "method": "$/progress", "params": {"token": 1}}));
        assert!(
            matches!(&got, Ok(Incoming::Notification { method, .. }) if method == "$/progress")
        );
        let got = classify(json!({"jsonrpc": "2.0", "id": 7, "result": null}));
        assert_eq!(
            got,
            Ok(Incoming::Response {
                id: json!(7),
                outcome: Ok(Value::Null)
            })
        );
        let got = classify(
            json!({"jsonrpc": "2.0", "id": 8, "error": {"code": -32801, "message": "modified"}}),
        );
        assert!(
            matches!(&got, Ok(Incoming::Response { outcome: Err(e), .. }) if e.code == code::CONTENT_MODIFIED)
        );
        assert!(classify(json!("hello")).is_err());
        assert!(classify(json!({"jsonrpc": "2.0"})).is_err());
        assert!(
            classify(json!({"jsonrpc": "2.0", "id": null, "error": {"code": -32700}}))
                .is_err_and(|e| e.contains("no id"))
        );
        assert!(classify(json!({"jsonrpc": "2.0", "id": 1, "method": 5})).is_err());
    }

    #[test]
    fn ids_and_params_on_the_way_out() {
        assert_eq!(
            request(3, "shutdown", Value::Null),
            json!({"jsonrpc": "2.0", "id": 3, "method": "shutdown"})
        );
        assert_eq!(
            notification("exit", Value::Null),
            json!({"jsonrpc": "2.0", "method": "exit"})
        );
        assert_eq!(
            response(json!("s-1"), Value::Null),
            json!({"jsonrpc": "2.0", "id": "s-1", "result": null})
        );
        assert_eq!(id_number(&json!(12)), Some(12));
        assert_eq!(id_number(&json!("12")), Some(12));
        assert_eq!(id_number(&json!("x")), None);
        assert_eq!(clip("ééé", 2), "éé…");
    }
}
