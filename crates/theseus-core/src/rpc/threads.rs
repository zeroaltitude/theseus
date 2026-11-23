//! `thread.list` and `.show`, read-only: threads were retired on 2026-11-22
//! (issue 44), and these read the stored records. A thread is private
//! always, so each read is the owner's, from a private place, judged as the
//! standing records' writes are (`judge_act(Act::Thread)`). Nothing writes a
//! thread record any more.

use std::collections::BTreeSet;

use serde_json::json;
use theseus_protocol::threads::{
    ThreadInfo, ThreadListParams, ThreadListResult, ThreadNameParams, ThreadRow, ThreadState,
};
use theseus_protocol::{error_code, method};
use theseus_store::{kinds, Page};

use super::confirms::Act;
use super::server::{route, Conn, RpcFailure};
use super::Core;
use crate::approval::{Answerer, Refusal};
use crate::ledger::LedgerRow;
use crate::session::SessionRecord;
use crate::threads::Board;

impl Core {
    /// The two read-only methods (threads were retired on 2026-11-22, issue
    /// 44): `thread.list` and `thread.show` read the stored records. Every
    /// other `thread.*` method is refused.
    pub(super) fn rpc_threads(
        &self,
        name: &str,
        params: serde_json::Value,
        conn: Conn<'_>,
    ) -> Result<serde_json::Value, RpcFailure> {
        match name {
            method::THREAD_LIST => route(params, |p: ThreadListParams| {
                self.thread_list(&p, conn.answerer(p.author.clone(), p.discord.clone()))
            }),
            method::THREAD_SHOW => route(params, |p: ThreadNameParams| {
                self.thread_show(&p.slug, conn.answerer(p.author.clone(), p.discord.clone()))
            }),
            // The other names stay in the protocol until the sweep takes them out: each is
            // refused, so a client that still sends one is told why.
            _ => Err(RpcFailure::new(
                error_code::REFUSED,
                format!(
                    "{name}: threads were retired on 2026-11-22; `thread.list` and `thread.show` \
                     read the stored records"
                ),
            )),
        }
    }

    /// The threads in memory (`Board`): built by two META scans the first
    /// time, after serving (`warm_threads`) or at the first reader. Never a
    /// store read on a turn's path once built. A store that cannot be read
    /// gives an empty board, warned of, and is tried again.
    pub fn threads(&self) -> std::sync::Arc<Board> {
        self.runner.threads.all(&self.store).unwrap_or_else(|e| {
            tracing::warn!(error = %format!("{e:#}"), "the threads cannot be read");
            Default::default()
        })
    }

    /// Read the threads now, off the start path: after serving, on the
    /// blocking pool. A reader that comes sooner reads them itself, once.
    pub fn warm_threads(self: &std::sync::Arc<Self>) {
        let core = std::sync::Arc::downgrade(self);
        tokio::task::spawn_blocking(move || {
            let Some(core) = core.upgrade() else { return };
            if let Err(e) = core.runner.threads.all(&core.store) {
                tracing::warn!(error = %format!("{e:#}"), "the threads cannot be read");
            }
        });
    }

    /// Judge an act on the threads: the owner, from a private place.
    pub(super) fn judge_thread(
        &self,
        who: &Answerer,
        method: &'static str,
        slug: &str,
    ) -> Result<(), RpcFailure> {
        let what = format!("the thread {slug}");
        self.judge_act(
            who,
            Act::Thread {
                method,
                what: &what,
            },
        )
        .map_err(failure)
    }

    /// `thread.list`: the active and paused threads (or those in `state`),
    /// newest activity first.
    pub fn thread_list(
        &self,
        p: &ThreadListParams,
        who: impl Into<Answerer>,
    ) -> Result<ThreadListResult, RpcFailure> {
        let who = who.into();
        self.judge_thread(&who, method::THREAD_LIST, "list")?;
        let board = self.threads();
        let mut shown: Vec<ThreadInfo> = board
            .all()
            .filter(|t| match p.state {
                Some(s) => t.state == s,
                None => t.state != ThreadState::Done,
            })
            .cloned()
            .collect();
        let rows = self.thread_rows(&mut shown);
        Ok(ThreadListResult {
            threads: rows,
            total: board.len() as u32,
        })
    }

    /// `thread.show`.
    pub fn thread_show(
        &self,
        slug: &str,
        who: impl Into<Answerer>,
    ) -> Result<ThreadRow, RpcFailure> {
        let who = who.into();
        self.judge_thread(&who, method::THREAD_SHOW, slug)?;
        let t = self.thread_named(slug)?;
        Ok(self
            .thread_rows(&mut [t])
            .pop()
            .expect("one row for one thread"))
    }

    pub(super) fn thread_named(&self, slug: &str) -> Result<ThreadInfo, RpcFailure> {
        self.threads().get(slug.trim()).cloned().ok_or_else(|| {
            RpcFailure::new(
                error_code::NOT_FOUND,
                format!("no thread {slug}: `theseus thread list` names them"),
            )
        })
    }

    /// The threads with what their sessions hold: the last activity, the
    /// context's tokens, and today's dollars. Reads the sessions, the
    /// compilations and today's spend rows: a read for a person, never a
    /// turn's path.
    pub(super) fn thread_rows(&self, threads: &mut [ThreadInfo]) -> Vec<ThreadRow> {
        let board = self.threads();
        let sessions: BTreeSet<String> = threads
            .iter()
            .filter_map(|t| t.session_id.clone())
            .collect();
        let today = self.spent_today_by(&sessions);
        let cache = self.thread_cache_for(&sessions.iter().cloned().collect::<Vec<_>>());
        let mut rows: Vec<ThreadRow> = threads
            .iter_mut()
            .map(|t| {
                let rec = t
                    .session_id
                    .as_deref()
                    .and_then(|s| self.store.get_session::<SessionRecord>(s).ok().flatten());
                let tokens = rec
                    .as_ref()
                    .and_then(|r| r.compilation_id.as_deref())
                    .and_then(|c| self.store.get_compilation(c).ok().flatten())
                    .and_then(|c| c.budget)
                    .map(|b| b.used_tokens);
                if let Some(r) = &rec {
                    t.last_active_ms = t.last_active_ms.max(r.last_active_ms);
                }
                let (merged_from, split_to) = board.links_of(&t.slug);
                ThreadRow {
                    cache_ttl_secs: rec
                        .as_ref()
                        .and_then(|r| r.last_target.as_ref())
                        .and_then(|t| self.runner.cfg.ttl_of(&t.profile))
                        .map(|(ttl, _)| ttl.secs()),
                    merged_from,
                    split_to,
                    today_usd: t
                        .session_id
                        .as_ref()
                        .and_then(|s| today.get(s))
                        .copied()
                        .unwrap_or(0.0),
                    cache: t.session_id.as_ref().and_then(|s| cache.get(s)).cloned(),
                    thread: t.clone(),
                    context_tokens: tokens,
                }
            })
            .collect();
        rows.sort_by(|a, b| {
            (b.thread.last_active_ms, &a.thread.slug)
                .cmp(&(a.thread.last_active_ms, &b.thread.slug))
        });
        rows
    }

    /// What `sessions` spent since local midnight, by session, from the
    /// ledger's spend rows: nothing while the index's shape is built.
    fn spent_today_by(
        &self,
        sessions: &BTreeSet<String>,
    ) -> std::collections::BTreeMap<String, f64> {
        let mut out = std::collections::BTreeMap::new();
        if sessions.is_empty() {
            return out;
        }
        let since = crate::learning::local_midnight(theseus_protocol::now_unix_ms());
        let tags: Vec<String> = crate::day_ceiling::counted_kinds()
            .map(theseus_store::pages::ledger_kind)
            .collect();
        let mut before = None;
        loop {
            let page = Page {
                kind: kinds::LEDGER,
                tags: tags.clone(),
                after: None,
                before,
                since_ms: Some(since),
                until_ms: None,
                limit: 512,
            };
            let Ok(Some(got)) = self.store.ledger_page(&page) else {
                return out;
            };
            for r in &got.records {
                let Ok(row) = r.decode::<LedgerRow>() else {
                    continue;
                };
                let Some(s) = row.session_id.as_deref().filter(|s| sessions.contains(*s)) else {
                    continue;
                };
                if row.at_unix_ms < since {
                    continue;
                }
                if let Some(m) = crate::day_ceiling::spend_of(&row) {
                    *out.entry(s.to_string()).or_insert(0.0) += m as f64 / 1_000_000.0;
                }
            }
            match (got.more, got.first) {
                (true, Some(first)) => before = Some(first),
                _ => return out,
            }
        }
    }
}

/// A write's error on the wire: a judged refusal is `REFUSED`.
pub(super) fn failure(e: anyhow::Error) -> RpcFailure {
    match e.downcast::<Refusal>() {
        Ok(r) => RpcFailure {
            code: error_code::REFUSED,
            message: format!(
                "a thread request from {} does not count: {}. Threads are the owner's, from a \
                 private place.",
                r.who, r.why
            ),
            data: json!({"who": r.who, "via": r.via, "why": r.why}),
        },
        Err(e) => RpcFailure::invalid(e),
    }
}
