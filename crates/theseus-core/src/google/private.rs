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
//! The mark follows recalled nodes (theseus-39dj0): a request that recall
//! fills with a node of a marked session holds private content, so the
//! asking session is marked too ([`followed`]), at the one point recall's
//! notes join a turn (`hold_node`, which every recall path of a turn and a
//! compaction ends in), and on `memory.lookup`'s answer, a tool result that
//! [`record`] marks.
//!
//! **The sensitive level.** A calendar the config marks `sensitive` adds
//! `"sensitive": true` to its result's meta, and [`record`] a second META
//! record, `google.sensitive.<session>`, in the same frame. The level follows
//! what the private mark follows (recall, a task's parent) and never lowers.
//! [`level`] is the one question the rest of the code asks of a session. The
//! content of a sensitive session never goes to a shared place (`places`,
//! `rpc/publish.rs`: sensitive is a subset of private) and no event text is in
//! any ledger row (`Google::call`'s row holds none).
//!
//! With no `[google]` nothing here reads the store.

use std::collections::HashSet;
use std::sync::Arc;

use serde_json::Value;
use theseus_store::{kinds, NewRecord};

use crate::node::{Body, Node};
use crate::provider::{
    CountFuture, DeltaSink, Provider, ProviderFuture, ProviderRequest, WarmFuture,
};
use crate::store::Store;

/// How much a session's content is held back. `Private`: Workspace content,
/// which only a model cleared for it may read. `Sensitive`: content of a
/// calendar the config marks `sensitive`, which also never reaches a shared
/// place or the ledger. Both floors read US origin alone today; the level is
/// what the page shows and what a stricter rule would read.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Default)]
pub enum Level {
    #[default]
    Open,
    Private,
    Sensitive,
}

impl From<bool> for Level {
    /// A caller that knows only "private" keeps that meaning.
    fn from(private: bool) -> Self {
        if private {
            Level::Private
        } else {
            Level::Open
        }
    }
}

tokio::task_local! {
    /// The provider call being made is for a session that read a Google
    /// result: `data_gate` refuses it to a model not cleared for the level.
    static SESSION_LEVEL: Level;
}

/// Run `f`, the making of a provider call, for a session at `level` (a bool
/// is private or open). The floor reads it at the call; the future `f`
/// returns is not scoped, since the floor decides before any byte is sent.
pub fn scoped<R>(level: impl Into<Level>, f: impl FnOnce() -> R) -> R {
    SESSION_LEVEL.sync_scope(level.into(), f)
}

/// Await `f`, a step whose provider calls are for a session at `level`: the
/// level is set while `f` runs, across its awaits.
pub async fn within<F: std::future::Future>(level: impl Into<Level>, f: F) -> F::Output {
    SESSION_LEVEL.scope(level.into(), f).await
}

/// The level of the call being made.
pub fn level_in_scope() -> Level {
    SESSION_LEVEL.try_with(|l| *l).unwrap_or_default()
}

/// Whether the call being made is a private (or sensitive) session's.
pub fn in_scope() -> bool {
    level_in_scope() != Level::Open
}

/// The `ToolOutput.meta` key that marks a result private.
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

/// The META key of a session's mark.
pub fn meta_key(session_id: &str) -> String {
    format!("google.private.{session_id}")
}

/// The META key of a session's sensitive mark: a second record, so a later
/// private mark never lowers it.
pub fn sensitive_key(session_id: &str) -> String {
    format!("google.sensitive.{session_id}")
}

/// The records a result node adds to its frame when it is marked: the session
/// now holds private content, and sensitive content when the result says so.
/// None for any other node.
pub fn record(node: &Node, session_id: &str) -> Vec<NewRecord> {
    let Body::ToolResult { meta, .. } = &node.body else {
        return Vec::new();
    };
    let write = |key: String| NewRecord::json(kinds::META, Some(&key), &node.id).ok();
    let private = marked(meta) || marked_sensitive(meta);
    [
        private.then(|| write(meta_key(session_id))).flatten(),
        marked_sensitive(meta)
            .then(|| write(sensitive_key(session_id)))
            .flatten(),
    ]
    .into_iter()
    .flatten()
    .collect()
}

/// Whether `session_id` has read a Google result. Nothing is read when the
/// config binds no account. A mark that cannot be read is a mark: the
/// session is held as private until it can be.
pub fn session(store: &Store, cfg: &crate::Config, session_id: &str) -> bool {
    session_on(store, &cfg.google, session_id)
}

/// [`session`], for a caller that holds only the `[google]` table (a tool's
/// call, theseus-a1wvq.6).
pub fn session_on(store: &Store, google: &crate::config::GoogleConfig, session_id: &str) -> bool {
    session_level_on(store, google, session_id) != Level::Open
}

/// The level of `session_id`'s own marks, ignoring its ancestors. Open when
/// the config binds no account (nothing is read). An unreadable mark is held:
/// private, and sensitive when the sensitive record cannot be read.
pub fn session_level(store: &Store, cfg: &crate::Config, session_id: &str) -> Level {
    session_level_on(store, &cfg.google, session_id)
}

/// [`session_level`], for a caller that holds only the `[google]` table.
pub fn session_level_on(
    store: &Store,
    google: &crate::config::GoogleConfig,
    session_id: &str,
) -> Level {
    if google.is_empty() {
        return Level::Open;
    }
    match store.get_meta::<String>(&sensitive_key(session_id)) {
        Ok(None) => {}
        _ => return Level::Sensitive,
    }
    match store.get_meta::<String>(&meta_key(session_id)) {
        Ok(None) => Level::Open,
        _ => Level::Private,
    }
}

/// Whether `session_id` itself has read a sensitive calendar.
pub fn sensitive_session(store: &Store, cfg: &crate::Config, session_id: &str) -> bool {
    session_level(store, cfg, session_id) == Level::Sensitive
}

/// The level of `session_id` and of every task above it: the highest of them
/// (a task under a sensitive session is sensitive, however late the parent
/// took the mark, theseus-y8k3u). **The one function the rest of the code asks
/// for a session's data level**: the cockpit's data mark reads it (ops-verbs),
/// and the route step's floor agrees with it. A record the store cannot read,
/// or a chain deeper than 16, counts as sensitive.
pub(crate) fn level(store: &Store, cfg: &crate::Config, session_id: &str) -> Level {
    if cfg.google.is_empty() {
        return Level::Open;
    }
    let mut at = session_id.to_string();
    let mut top = Level::Open;
    for _ in 0..16 {
        top = top.max(session_level(store, cfg, &at));
        if top == Level::Sensitive {
            return top;
        }
        match store.get_session::<crate::session::SessionRecord>(&at) {
            Ok(Some(r)) => match r.task {
                Some(k) => at = k.parent_session,
                None => return top,
            },
            Ok(None) => return top,
            Err(_) => return Level::Sensitive,
        }
    }
    Level::Sensitive
}

/// Whether `session_id` or any task above it has read a Google result.
pub fn lineage(store: &Store, cfg: &crate::Config, session_id: &str) -> bool {
    level(store, cfg, session_id) != Level::Open
}

/// Whether `session_id` or any task above it has read a sensitive calendar.
pub fn sensitive_lineage(store: &Store, cfg: &crate::Config, session_id: &str) -> bool {
    level(store, cfg, session_id) == Level::Sensitive
}

/// The highest level among `sessions`: the mark of a request that holds
/// content from several of them (fail closed: one is enough).
pub fn over<S: AsRef<str>>(
    store: &Store,
    cfg: &crate::Config,
    sessions: impl IntoIterator<Item = S>,
) -> Level {
    sessions
        .into_iter()
        .map(|s| session_level(store, cfg, s.as_ref()))
        .max()
        .unwrap_or_default()
}

/// Whether any of `sessions` has read a Google result.
pub fn any_session<S: AsRef<str>>(
    store: &Store,
    cfg: &crate::Config,
    sessions: impl IntoIterator<Item = S>,
) -> bool {
    over(store, cfg, sessions) != Level::Open
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

/// The ids of `records` a session holding the mark authored: a record kept
/// from a message (`source` `capture:<node>`) whose node's session holds it.
/// Nothing is read with no `[google]`; a node the store cannot read counts
/// as private (`any_node`).
pub fn authored<'a>(
    store: &Store,
    cfg: &crate::Config,
    records: impl IntoIterator<Item = &'a theseus_protocol::StandingRecord>,
) -> HashSet<String> {
    if cfg.google.is_empty() {
        return HashSet::new();
    }
    // Any other source is a surface the owner typed on: the owner's own words.
    let mut private_node: std::collections::HashMap<&str, bool> = Default::default();
    records
        .into_iter()
        .filter_map(|r| Some((r, r.source.strip_prefix("capture:")?)))
        .filter(|(_, node)| {
            *private_node
                .entry(node)
                .or_insert_with(|| any_node(store, cfg, [node]))
        })
        .map(|(r, _)| r.id.clone())
        .collect()
}

/// The first of `recalled` (node id, session id) whose session is at `least`
/// the level, other than `asker`'s: the node that makes a request of `asker`
/// hold content of that level (theseus-39dj0). Nothing is read with no
/// `[google]`; an unreadable mark is a mark.
fn recalled_at<'a>(
    store: &Store,
    google: &crate::config::GoogleConfig,
    asker: &str,
    least: Level,
    recalled: impl IntoIterator<Item = (&'a str, &'a str)>,
) -> Option<&'a str> {
    if google.is_empty() {
        return None;
    }
    recalled
        .into_iter()
        .find(|(_, s)| *s != asker && session_level_on(store, google, s) >= least)
        .map(|(node, _)| node)
}

/// The first recalled node of a session that holds the private mark.
pub fn recalled_from<'a>(
    store: &Store,
    google: &crate::config::GoogleConfig,
    asker: &str,
    recalled: impl IntoIterator<Item = (&'a str, &'a str)>,
) -> Option<&'a str> {
    recalled_at(store, google, asker, Level::Private, recalled)
}

/// The first recalled node of a session that holds the sensitive mark.
pub fn recalled_sensitive_from<'a>(
    store: &Store,
    google: &crate::config::GoogleConfig,
    asker: &str,
    recalled: impl IntoIterator<Item = (&'a str, &'a str)>,
) -> Option<&'a str> {
    recalled_at(store, google, asker, Level::Sensitive, recalled)
}

/// The META records that make `asker` private, and sensitive, because recall
/// put into its request a node of a session that holds the mark
/// (theseus-39dj0): the mark follows recalled nodes, so every later call of
/// `asker` carries it. Empty with no `[google]` (nothing is read), when
/// `asker` holds each mark already, or when no recalled node's session does.
pub fn followed<'a>(
    store: &Store,
    google: &crate::config::GoogleConfig,
    asker: &str,
    recalled: impl IntoIterator<Item = (&'a str, &'a str)> + Clone,
) -> Vec<NewRecord> {
    let have = session_level_on(store, google, asker);
    let mut out = Vec::new();
    for (level, key) in [
        (Level::Private, meta_key(asker)),
        (Level::Sensitive, sensitive_key(asker)),
    ] {
        if have >= level {
            continue;
        }
        let Some(node) = recalled_at(store, google, asker, level, recalled.clone()) else {
            continue;
        };
        out.extend(NewRecord::json(kinds::META, Some(&key), &node).ok());
    }
    out
}

/// The META records that make `child` as private as `parent` is: written in
/// the frame that opens a task, so the task reads as private (and sensitive)
/// from its first call. Empty when the parent is neither.
pub fn inherited(
    store: &Store,
    cfg: &crate::Config,
    parent: &str,
    child: &str,
) -> anyhow::Result<Vec<NewRecord>> {
    inherited_on(store, &cfg.google, parent, child)
}

/// [`inherited`], for a caller that holds only the `[google]` table.
pub fn inherited_on(
    store: &Store,
    google: &crate::config::GoogleConfig,
    parent: &str,
    child: &str,
) -> anyhow::Result<Vec<NewRecord>> {
    let level = session_level_on(store, google, parent);
    let mut out = Vec::new();
    if level >= Level::Private {
        out.push(NewRecord::json(
            kinds::META,
            Some(&meta_key(child)),
            &parent,
        )?);
    }
    if level == Level::Sensitive {
        out.push(NewRecord::json(
            kinds::META,
            Some(&sensitive_key(child)),
            &parent,
        )?);
    }
    Ok(out)
}

/// `provider`, whose every call is made for `session` (a refusal's row names
/// it) and, above `Open`, for a session that read a Google result. For the
/// steps that hold the provider and call it later, or on another task, where
/// a scope around the call's creation would not reach: the level is set as
/// each call is made, inside this wrapper. A provider of no session and no
/// level is returned as it is.
pub fn bound(
    provider: Arc<dyn Provider>,
    session: Option<&str>,
    level: impl Into<Level>,
) -> Arc<dyn Provider> {
    let level = level.into();
    if session.is_none() && level == Level::Open {
        return provider;
    }
    Arc::new(Bound {
        inner: provider,
        session: session.map(str::to_string),
        level,
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
    bound(
        provider,
        Some(session_id),
        session_level(store, cfg, session_id),
    )
}

/// `provider` for a call whose request holds content from every one of
/// `sessions`: at the highest level of them.
pub fn over_sessions<S: AsRef<str>>(
    provider: Arc<dyn Provider>,
    store: &Store,
    cfg: &crate::Config,
    sessions: impl IntoIterator<Item = S>,
) -> Arc<dyn Provider> {
    bound(provider, None, over(store, cfg, sessions))
}

struct Bound {
    inner: Arc<dyn Provider>,
    session: Option<String>,
    level: Level,
}

impl Bound {
    fn scope<R>(&self, f: impl FnOnce() -> R) -> R {
        let marked = || {
            if self.level != Level::Open {
                scoped(self.level, f)
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
