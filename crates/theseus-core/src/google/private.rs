//! The `private` marker (the Google design, §4.1): all Workspace content is
//! private. Every Google result carries `"private": true` in its `meta` (`google::finish`
//! sets it), and the frame that writes such a result also writes a META record
//! for its session. From then on the session's data answer is `private`, by
//! code, as a sensitive name makes it (theseus-oejh3), and the provider floor
//! (`data_gate`) refuses a request of that session to a model of restricted
//! origin. Fail closed: no judgment lifts it. The one question of origin, what
//! is restricted, is the data-routing lane's (`ModelOrigin::restricted`), never
//! asked here.
//!
//! With no `[google]` nothing here reads the store.

use serde_json::Value;
use theseus_store::{kinds, NewRecord};

use crate::node::{Body, Node};
use crate::store::Store;

tokio::task_local! {
    /// The provider call being made is for a session that read a Google
    /// result: `data_gate` refuses it to a model of restricted origin.
    static SESSION_PRIVATE: bool;
}

/// Run `f`, the making of a provider call, for a session that is (or is not)
/// private. The floor reads it at the call; the future `f` returns is not
/// scoped, since the floor decides before any byte is sent.
pub fn scoped<R>(private: bool, f: impl FnOnce() -> R) -> R {
    SESSION_PRIVATE.sync_scope(private, f)
}

/// Whether the call being made is a private session's.
pub fn in_scope() -> bool {
    SESSION_PRIVATE.try_with(|p| *p).unwrap_or(false)
}

/// The `ToolOutput.meta` key that marks a result private.
pub const KEY: &str = "private";

/// Whether a result's meta carries the mark.
pub fn marked(meta: &Value) -> bool {
    meta.get(KEY).and_then(Value::as_bool).unwrap_or(false)
}

/// The META key of a session's mark.
pub fn meta_key(session_id: &str) -> String {
    format!("google.private.{session_id}")
}

/// The record a result node adds to its frame when it is marked: the session
/// now holds private content. None for any other node.
pub fn record(node: &Node, session_id: &str) -> Option<NewRecord> {
    let Body::ToolResult { meta, .. } = &node.body else {
        return None;
    };
    marked(meta)
        .then(|| NewRecord::json(kinds::META, Some(&meta_key(session_id)), &node.id).ok())
        .flatten()
}

/// Whether `session_id` has read a Google result. Nothing is read when the
/// config binds no account.
pub fn session(store: &Store, cfg: &crate::Config, session_id: &str) -> bool {
    !cfg.google.is_empty()
        && store
            .get_meta::<String>(&meta_key(session_id))
            .ok()
            .flatten()
            .is_some()
}
