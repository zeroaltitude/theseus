//! What a tool call writes back (spec §3.16): a call's failure as its
//! result, the answer for a call that never ran, the `ResultNode` a call site
//! fills, the call input a toollet refused, and the reply's unanswered
//! calls. Split from `toolrun.rs`, which imports them so the call sites keep
//! their paths.

use std::collections::HashSet;

use serde_json::Value;
use theseus_kernel::Action;
use theseus_protocol::GateRecord;

use super::kept;
use crate::node::{Body, Node, ResultStatus};
use crate::provider::ToolUse;

/// An async call's failure, as its result: `unknown` when the tool says its
/// outcome is (a connection that ended while it waited), and outside text
/// when the tool says its words are (an MCP server's error result).
#[allow(clippy::type_complexity)]
pub(super) fn failure(
    message: String,
    meta: Value,
) -> (
    ResultStatus,
    String,
    Value,
    Option<theseus_tools::Media>,
    Option<theseus_tools::External>,
) {
    let status = match meta.get("outcome_unknown").and_then(Value::as_bool) {
        Some(true) => ResultStatus::Unknown,
        _ => ResultStatus::Error,
    };
    let external = meta
        .get("external")
        .and_then(Value::as_str)
        .map(|url| theseus_tools::External { url: url.into() });
    (status, message, meta, None, external)
}

/// What a call that never ran tells the model: who declined it, that nobody
/// answered its question in time, or why it was cancelled (its action's
/// resolution).
pub(super) fn not_run_answer(a: &Action) -> (ResultStatus, String) {
    // An expired question is no one's decline (theseus-830).
    if let Some(answer) = crate::rpc::expired_answer(a) {
        return answer;
    }
    // A decline records who declined; the model reads only the note.
    match a.declined_note() {
        Some(note) => (
            ResultStatus::Declined,
            format!("Not run: the operator declined this call ({note})."),
        ),
        None => (
            ResultStatus::Cancelled,
            format!(
                "Not run: {}.",
                a.resolution.as_deref().unwrap_or("cancelled")
            ),
        ),
    }
}

/// A tool result to write: the call it answers, its status and text, and
/// what else the call site knows. Name only what differs from `new`, as
/// `ResultNode { late: true, ..ResultNode::new(…) }`.
pub(super) struct ResultNode<'a> {
    pub(super) tool_use_id: &'a str,
    pub(super) tool: &'a str,
    pub(super) status: ResultStatus,
    /// Before it is scrubbed and capped.
    pub(super) text: String,
    pub(super) correlation_id: Option<&'a str>,
    pub(super) duration_ms: Option<u64>,
    pub(super) late: bool,
    /// Bytes of the whole output; the text's own length when None.
    pub(super) bytes_total: Option<u64>,
    pub(super) meta: Value,
    pub(super) image: Option<crate::node::Attachment>,
    /// Where its text came from, when that is outside Theseus (DD5).
    pub(super) external: Option<theseus_tools::External>,
    /// The line its text ends with, past the cap: the native call a job of
    /// the daemon's own CLI stands in for (`native::hint`, theseus-8c8s).
    pub(super) hint: Option<String>,
    /// Where the output in its text is, kept whole when cut (theseus-v73m).
    pub(super) kept: Option<kept::Source>,
}

impl<'a> ResultNode<'a> {
    pub(super) fn new(
        tool_use_id: &'a str,
        tool: &'a str,
        status: ResultStatus,
        text: impl Into<String>,
    ) -> Self {
        Self {
            tool_use_id,
            tool,
            status,
            text: text.into(),
            correlation_id: None,
            duration_ms: None,
            late: false,
            bytes_total: None,
            meta: Value::Null,
            image: None,
            external: None,
            hint: None,
            kept: None,
        }
    }
}

/// A call whose input the toollet refused, or that its place may not make
/// (`refused`, the place rule); its gate record is still stored.
pub(super) struct Invalid {
    pub(super) record: Box<GateRecord>,
    pub(super) error: String,
    pub(super) refused: bool,
}

/// The last assistant message, and its `tool_use`s with no result yet.
pub(super) fn unanswered(nodes: &[(u64, crate::stub::Stub)]) -> Option<(&Node, Vec<ToolUse>)> {
    use crate::stub::Kind;
    let at = nodes
        .iter()
        .rposition(|(_, n)| n.kind == Kind::AssistantMessage)?;
    let last = &nodes[at].1;
    // A call's result follows the reply that holds it: only what came after
    // is read, so a long session's earlier results stay stubs (step 33).
    let answered: HashSet<&str> = nodes[at + 1..]
        .iter()
        .filter(|(_, n)| n.kind == Kind::ToolResult)
        .filter_map(|(_, n)| match &n.body {
            Body::ToolResult {
                tool_use_id,
                late: false,
                ..
            } => Some(tool_use_id.as_str()),
            _ => None,
        })
        .collect();
    let Body::AssistantMessage { blocks, .. } = &last.body else {
        return None;
    };
    let pending = crate::provider::tool_uses_in(blocks)
        .into_iter()
        .filter(|u| !answered.contains(u.id.as_str()))
        .collect();
    Some((&**last, pending))
}
