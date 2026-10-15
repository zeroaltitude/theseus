//! `work.watch` and `work.unwatch` (theseus-hkx30): the work board on the
//! wire. The watch subscribes this connection before it reads the board, so
//! no change falls between the snapshot and the events (the position rule
//! settles the overlap, as `executions.watch`'s does).
//!
//! The place rule: a connection that reads no private text must name the
//! conversations it asks for, and hears about those of a shared place alone;
//! what it hears passes the scrubber and offers no answer (`push/work/place.rs`).

use std::collections::HashSet;
use std::sync::Arc;

use serde_json::json;
use theseus_protocol::{error_code, method, WorkUnwatchResult, WorkWatchParams, WorkWatchResult};

use super::server::{parse, reply, Conn, RpcFailure};
use super::Core;
use crate::push::WorkScope;
use theseus_memory::recall::Place;

/// The roots a snapshot carries besides those that need you or work.
const DEFAULT_LIMIT: u32 = 200;

impl Core {
    /// The work board's methods, from the prefixed arm of `dispatch`.
    pub(super) async fn rpc_work(
        self: Arc<Self>,
        name: &str,
        params: serde_json::Value,
        conn: Conn<'_>,
    ) -> Result<serde_json::Value, RpcFailure> {
        match name {
            method::WORK_WATCH => {
                // Its params are optional: none watches every root.
                let params = if params.is_null() { json!({}) } else { params };
                reply(self.work_watch(parse(params)?, conn).await?)
            }
            method::WORK_UNWATCH => reply(self.work_unwatch(conn)),
            method::WORK_PEEK => reply(self.work_peek(parse(params)?, conn).await?),
            other => Err(RpcFailure::new(
                error_code::METHOD_NOT_FOUND,
                format!("no method {other}"),
            )),
        }
    }

    async fn work_watch(
        self: &Arc<Self>,
        p: WorkWatchParams,
        conn: Conn<'_>,
    ) -> Result<WorkWatchResult, RpcFailure> {
        let guarded = !conn.surface.reads_private();
        let scope = self.work_scope(&p, guarded, conn.surface.name())?;
        self.push.ensure(self).await?;
        self.bus.watch_work(
            conn.client,
            conn.tx.clone(),
            scope.clone(),
            p.interactive.unwrap_or(false),
        );
        // The threads are the owner's: a connection that reads no private
        // text is told of none. The watch is subscribed already, so the read
        // of their stats reaches it as an update too.
        let threads = match guarded {
            true => Vec::new(),
            false => {
                self.install_thread_feed();
                self.push.set_threads(self, self.thread_groups());
                self.push.work_threads()
            }
        };
        let (position, views, total) = self
            .push
            .work_snapshot(p.limit.unwrap_or(DEFAULT_LIMIT) as usize, &scope);
        Ok(WorkWatchResult {
            position,
            views: scope.cut(&views, &self.tools.scrubber),
            total,
            threads,
        })
    }

    fn work_unwatch(&self, conn: Conn<'_>) -> WorkUnwatchResult {
        self.bus.unwatch_work(conn.client);
        WorkUnwatchResult { watching: false }
    }

    /// What a watch may see. `roots` are conversations (session ids); a
    /// guarded connection (one that reads no private text) must name them,
    /// and a root that does not speak in a shared place is left out without
    /// a word: asking is no way to learn that a private session exists.
    fn work_scope(
        &self,
        p: &WorkWatchParams,
        guarded: bool,
        surface: &str,
    ) -> Result<WorkScope, RpcFailure> {
        let Some(named) = p.roots.as_ref() else {
            if guarded {
                return Err(RpcFailure::new(
                    error_code::INVALID_PARAMS,
                    format!(
                        "{surface} reads no private text: name the conversations (roots) to watch"
                    ),
                ));
            }
            return Ok(WorkScope::default());
        };
        let mut roots: HashSet<String> = HashSet::new();
        for sid in named {
            // The daemon's own work, for a surface that reads private text.
            if sid == theseus_protocol::work_system::ROOT {
                if !guarded {
                    roots.insert(sid.clone());
                }
                continue;
            }
            let Ok(rec) = self.session(sid) else { continue };
            if guarded && !matches!(self.runner.place_of(sid), Place::Shared(_)) {
                continue;
            }
            if let Some(exe) = rec.execution_id {
                roots.insert(exe);
            }
        }
        Ok(WorkScope {
            roots: Some(roots),
            guarded,
        })
    }
}
