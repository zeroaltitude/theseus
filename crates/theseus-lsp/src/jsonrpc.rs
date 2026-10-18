//! JSON-RPC 2.0 as LSP carries it: one message per frame, never batched. The
//! builders and the reader are theseus-protocol's, shared with MCP's; this
//! file adds LSP's own error codes.

pub use theseus_protocol::jsonrpc::{
    classify, error_response, id_number, notification, request, response, Incoming, JSONRPC,
};
pub use theseus_protocol::term::words::cut_marked as clip;
pub use theseus_protocol::RpcError;

#[cfg(test)]
use serde_json::{json, Value};

/// JSON-RPC's error codes, and LSP's own.
pub mod code {
    pub use theseus_protocol::jsonrpc::code::*;
    /// The document changed under the request: asking again may succeed.
    pub const CONTENT_MODIFIED: i64 = -32801;
    /// The server cancelled the request itself (pull diagnostics: ask again
    /// when `retriggerRequest` is true).
    pub const SERVER_CANCELLED: i64 = -32802;
    /// The client cancelled it with `$/cancelRequest`.
    pub const REQUEST_CANCELLED: i64 = -32800;
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
