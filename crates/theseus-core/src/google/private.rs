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
//! The mark is carried to the floor by [`scoped`] around the call being made;
//! a step that holds a provider and calls it later takes [`bound`] instead, so
//! the mark is set as each call is made. Every provider call that holds a
//! session's content takes one of the two: the turn's and compaction's by
//! `scoped`, the background steps' (keep-warm, the memory pass, learning, the
//! people judge, cards, naming, replays, the census) by `bound`.
//!
//! With no `[google]` nothing here reads the store.

use std::sync::Arc;

use serde_json::Value;
use theseus_store::{kinds, NewRecord};

use crate::node::{Body, Node};
use crate::provider::{
    CountFuture, DeltaSink, Provider, ProviderFuture, ProviderRequest, WarmFuture,
};
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

/// Await `f`, a step whose provider calls are for a session that is (or is not)
/// private: the mark is set while `f` runs, across its awaits.
pub async fn within<F: std::future::Future>(private: bool, f: F) -> F::Output {
    SESSION_PRIVATE.scope(private, f).await
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
/// config binds no account. A mark that cannot be read is a mark: the
/// session is held as private until it can be.
pub fn session(store: &Store, cfg: &crate::Config, session_id: &str) -> bool {
    !cfg.google.is_empty()
        && match store.get_meta::<String>(&meta_key(session_id)) {
            Ok(mark) => mark.is_some(),
            Err(_) => true,
        }
}

/// Whether any of `sessions` has read a Google result: the mark of a request
/// that holds content from several of them (fail closed: one is enough).
pub fn any_session<S: AsRef<str>>(
    store: &Store,
    cfg: &crate::Config,
    sessions: impl IntoIterator<Item = S>,
) -> bool {
    !cfg.google.is_empty()
        && sessions
            .into_iter()
            .any(|s| session(store, cfg, s.as_ref()))
}

/// Whether any of the nodes `ids` is in a session that has read a Google
/// result. A node the store cannot read leaves its session unknown, and an
/// unknown session cannot be cleared: it counts as private.
pub fn any_node<S: AsRef<str>>(
    store: &Store,
    cfg: &crate::Config,
    ids: impl IntoIterator<Item = S>,
) -> bool {
    !cfg.google.is_empty()
        && ids
            .into_iter()
            .any(|id| match store.get_node(id.as_ref()).ok().flatten() {
                Some((_, n)) => session(store, cfg, &n.session_id),
                None => true,
            })
}

/// The META record that makes `child` private because `parent` is: written in
/// the frame that opens a task, so the task reads as private from its first
/// call. None when the parent is not private.
pub fn inherited(
    store: &Store,
    cfg: &crate::Config,
    parent: &str,
    child: &str,
) -> anyhow::Result<Option<NewRecord>> {
    if !session(store, cfg, parent) {
        return Ok(None);
    }
    Ok(Some(NewRecord::json(
        kinds::META,
        Some(&meta_key(child)),
        &parent,
    )?))
}

/// `provider`, whose every call is made for `session` (a refusal's row names
/// it) and, when `private`, for a session that read a Google result. For the
/// steps that hold the provider and call it later, or on another task, where
/// a scope around the call's creation would not reach: the mark is set as
/// each call is made, inside this wrapper. A provider of no session and no
/// mark is returned as it is.
pub fn bound(
    provider: Arc<dyn Provider>,
    session: Option<&str>,
    private: bool,
) -> Arc<dyn Provider> {
    if session.is_none() && !private {
        return provider;
    }
    Arc::new(Bound {
        inner: provider,
        session: session.map(str::to_string),
        private,
    })
}

/// `provider` for a call made for `session` alone: its mark, read now, holds
/// the call, and a refusal's row names it.
pub fn for_session(
    provider: Arc<dyn Provider>,
    store: &Store,
    cfg: &crate::Config,
    session_id: &str,
) -> Arc<dyn Provider> {
    bound(provider, Some(session_id), session(store, cfg, session_id))
}

/// `provider` for a call whose request holds content from every one of
/// `sessions`: private when any one of them is.
pub fn over_sessions<S: AsRef<str>>(
    provider: Arc<dyn Provider>,
    store: &Store,
    cfg: &crate::Config,
    sessions: impl IntoIterator<Item = S>,
) -> Arc<dyn Provider> {
    bound(provider, None, any_session(store, cfg, sessions))
}

struct Bound {
    inner: Arc<dyn Provider>,
    session: Option<String>,
    private: bool,
}

impl Bound {
    fn scope<R>(&self, f: impl FnOnce() -> R) -> R {
        let marked = || {
            if self.private {
                scoped(true, f)
            } else {
                f()
            }
        };
        match &self.session {
            Some(s) => crate::data_gate::for_session(s, marked),
            None => marked(),
        }
    }
}

impl Provider for Bound {
    fn name(&self) -> &str {
        self.inner.name()
    }
    fn stream_message<'a>(
        &'a self,
        req: &'a ProviderRequest,
        on_delta: DeltaSink<'a>,
    ) -> ProviderFuture<'a> {
        self.scope(|| self.inner.stream_message(req, on_delta))
    }
    fn warm(&self) -> WarmFuture<'_> {
        self.inner.warm()
    }
    fn count_tokens<'a>(&'a self, req: &'a ProviderRequest) -> CountFuture<'a> {
        self.scope(|| self.inner.count_tokens(req))
    }
}
