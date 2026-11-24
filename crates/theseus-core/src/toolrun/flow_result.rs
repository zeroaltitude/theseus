//! One flow step's settled node identity, captured at the writer, not discovered in history.
use crate::node::{Body, Node};
use std::cell::RefCell;

tokio::task_local! {
    static RESULT: RefCell<(String, String, Option<String>)>;
}

pub(super) async fn capture<F, T>(session: &str, call: &str, run: F) -> (T, Option<String>)
where
    F: std::future::Future<Output = T>,
{
    RESULT
        .scope(RefCell::new((session.into(), call.into(), None)), async {
            let result = run.await;
            (result, RESULT.with(|r| r.borrow_mut().2.take()))
        })
        .await
}

/// Invoked only after the result's frame is written. No other session or call can fill this slot.
pub(super) fn written(node: &Node) {
    let Body::ToolResult {
        tool_use_id,
        late: false,
        ..
    } = &node.body
    else {
        return;
    };
    let _ = RESULT.try_with(|r| {
        let mut r = r.borrow_mut();
        if r.0 == node.session_id && r.1 == *tool_use_id {
            r.2 = Some(node.id.clone());
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    fn result(session: &str, call: &str) -> Node {
        let body = serde_json::from_value(serde_json::json!({
            "kind": "tool_result", "tool_use_id": call, "tool": "fs.read",
            "status": "ok", "is_error": false, "content": "settled"
        }))
        .unwrap();
        Node::tool_result(session, Some("turn"), None, body)
    }
    #[tokio::test]
    async fn only_the_scoped_steps_settled_identity_is_returned() {
        let expected = result("session", "step");
        let (_, id) = capture("session", "step", async {
            written(&expected);
            written(&result("other", "step"));
            written(&result("session", "other"));
        })
        .await;
        assert_eq!(id.as_deref(), Some(expected.id.as_str()));
        let (_, absent) = capture("session", "step", async {}).await;
        assert!(
            absent.is_none(),
            "the preceding step cannot fill a new scope"
        );
    }
}
