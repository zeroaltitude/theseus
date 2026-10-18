//! JSON-RPC 2.0 as MCP carries it: one message per line on stdio, one per
//! POST (or per server-sent event) over HTTP. The builders and the reader are
//! theseus-protocol's, shared with LSP's. Since 2025-06-18 nothing is batched;
//! an array from a server on an older revision is read as its messages in
//! order.

pub use theseus_protocol::jsonrpc::{
    classify_batch as classify, error_response, id_number, notification, request, response,
    Incoming, JSONRPC,
};
pub use theseus_protocol::term::words::cut_marked as clip;
pub use theseus_protocol::RpcError;

#[cfg(test)]
use serde_json::{json, Value};

/// JSON-RPC's own error codes, and the ones MCP uses.
pub mod code {
    pub use theseus_protocol::jsonrpc::code::*;
    /// A server-defined code: the client answers too many of the server's
    /// own requests now (`client::SERVED_AT_ONCE`).
    pub const BUSY: i64 = -32000;
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
