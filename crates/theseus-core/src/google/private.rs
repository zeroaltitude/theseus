//! A Google result is a label source (theseus-gyjyo.5). Every Google tool's
//! result carries `"private": true` in its `meta` (`google::finish` sets it),
//! and a result of a calendar the config marks `sensitive` adds
//! `"sensitive": true`. `toolrun/complete.rs` reads the two where the result
//! is written and raises a label on its session ([`label`]): `confidential`,
//! or `personal` for a sensitive calendar, with source `google.<tool>`,
//! deterministically and with no model call (`content::deterministic`).
//!
//! From there it is any label: the operator sees it, removes it, and the same
//! switch (`[content_based_security] session_flags`) and the same chokepoint
//! (`data_gate`) read it. Nothing here marks a session, follows a recalled node
//! or a task's parent, or refuses a request. A calendar the config marks
//! `sensitive` still never reaches a shared place or the ledger
//! (`rpc/publish.rs`, `Google::call`'s row).
//!
//! With no `[google]` no tool runs and nothing here is read.

use serde_json::Value;

use crate::content::{deterministic, Flag, ItemKind, Label};
use crate::node::{Body, Node};

/// The `ToolOutput.meta` key that marks a result Workspace content.
pub const KEY: &str = "private";

/// The `ToolOutput.meta` key that marks a result sensitive, beside [`KEY`].
pub const SENSITIVE: &str = "sensitive";

/// Whether a result's meta carries the mark.
pub fn marked(meta: &Value) -> bool {
    meta.get(KEY).and_then(Value::as_bool).unwrap_or(false)
}

/// Whether a result's meta carries the sensitive mark.
pub fn marked_sensitive(meta: &Value) -> bool {
    meta.get(SENSITIVE)
        .and_then(Value::as_bool)
        .unwrap_or(false)
}

/// The source of a Google result's label: `google.<tool>`, the tool's name
/// without the `google.` it may already begin with (`google.call` reads
/// `google.call`, `calendar.events` reads `google.calendar.events`).
pub fn source(tool: &str) -> String {
    format!("google.{}", tool.strip_prefix("google.").unwrap_or(tool))
}

/// The label a result node raises, with the time the node entered (what a
/// Remove is compared against, `ContentFlags::raise_after`): none for any
/// other node. `personal` when the result is sensitive, else `confidential`.
pub fn label(node: &Node, now_ms: u64) -> Option<(Label, u64)> {
    let Body::ToolResult { tool, meta, .. } = &node.body else {
        return None;
    };
    let flag = match (marked_sensitive(meta), marked(meta)) {
        (true, _) => Flag::Personal,
        (false, true) => Flag::Confidential,
        (false, false) => return None,
    };
    let label = deterministic::label(flag, &node.id, ItemKind::ToolResult, source(tool), now_ms);
    Some((label, node.created_at_ms))
}
