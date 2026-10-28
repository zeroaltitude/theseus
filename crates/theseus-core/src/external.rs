//! The `external` marker (DD5): a tool result whose text came from outside Theseus (`http.fetch`, `web.search`,
//! an MCP server marked external, a job that connected beyond `[sandbox] egress`) carries `external` on its node.
//! The marker is all there is: it holds nothing and waits for no one. What reads it is the content pack (the
//! source of a label), Jev's security state (the first outside text a session read), the index (recall keeps
//! outside text out unless asked) and egress's "outside text". The `injection` label is the one place a read of
//! outside text can stop a later call, and it does that at the act bar, by the owner's design (R6, theseus-gyjyo.8).

use crate::node::{Body, Node};

/// Whether `node` is a result marked as outside text.
pub fn is_outside(node: &Node) -> bool {
    matches!(
        &node.body,
        Body::ToolResult {
            external: Some(_),
            ..
        }
    )
}

/// The first outside text a session read: the tool whose result it was, what the result is marked with (the
/// page's final URL, the search's request, the hosts a job reached), and when the node was written.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Outside {
    pub tool: String,
    pub url: String,
    pub at_ms: u64,
}

/// The first of `nodes` that is outside text, which is the one Jev's state names.
pub fn first(nodes: &[Node]) -> Option<Outside> {
    nodes.iter().find_map(|n| match &n.body {
        Body::ToolResult {
            external: Some(e),
            tool,
            ..
        } => Some(Outside {
            tool: tool.clone(),
            url: e.url.clone(),
            at_ms: n.created_at_ms,
        }),
        _ => None,
    })
}

/// Whether `session_id` has read outside text: any result node of it is marked. A session whose nodes cannot be
/// read counts as having, so a fetch or a search in it is judged (the gate's `judged`).
pub fn read_any(store: &crate::store::Store, session_id: &str) -> bool {
    match store.session_nodes(session_id) {
        Ok(nodes) => nodes.iter().any(|(_, n)| is_outside(n)),
        Err(_) => true,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::node::ResultStatus;

    fn result(id: &str, tool: &str, at_ms: u64, url: Option<&str>) -> Node {
        let mut n = Node::harness_body(
            "nod",
            "ses_a",
            "test",
            Body::ToolResult {
                tool_use_id: format!("tu_{id}"),
                tool: tool.into(),
                status: ResultStatus::Ok,
                is_error: false,
                content: "text".into(),
                correlation_id: None,
                bytes_total: 4,
                truncated: false,
                full_ref: None,
                duration_ms: None,
                late: false,
                meta: serde_json::Value::Null,
                image: None,
                external: url.map(|u| theseus_tools::External { url: u.into() }),
            },
        );
        n.id = id.into();
        n.created_at_ms = at_ms;
        n
    }

    /// The marker is what makes a result outside text, and the first one is what Jev's state names.
    #[test]
    fn the_marker_is_read_and_the_first_marked_result_is_named() {
        let local = result("n1", "fs.read", 10, None);
        let page = result("n2", "http.fetch", 20, Some("https://site.test/a"));
        let search = result("n3", "web.search", 30, Some("tides"));
        assert!(!is_outside(&local) && is_outside(&page) && is_outside(&search));
        assert_eq!(first(&[local.clone()]), None);
        assert_eq!(
            first(&[local, page, search]),
            Some(Outside {
                tool: "http.fetch".into(),
                url: "https://site.test/a".into(),
                at_ms: 20
            })
        );
    }
}
