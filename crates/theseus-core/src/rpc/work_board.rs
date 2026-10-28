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
            method::JOB_STOP => reply(self.job_stop(parse(params)?, conn).await?),
            method::WORK_AGENDA => reply(self.work_agenda_rpc(parse(params)?, conn).await?),
            method::WORK_DIGEST => {
                let params = if params.is_null() { json!({}) } else { params };
                reply(self.work_digest_for(parse(params)?, conn.surface).await?)
            }
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
        let scope = self.work_scope(&p, guarded, conn.surface)?;
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
        let (position, views, total) =
            self.work_snapshot_marked(p.limit.unwrap_or(DEFAULT_LIMIT) as usize, &scope);
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

    /// Whether a guarded connection may watch `sid`: a shared place's
    /// conversation always; and, for the Discord binding, a conversation that
    /// speaks in a Discord place of any class (theseus-2j2ka). The binding
    /// shows a place its own session's tasks in that place alone, as it shows
    /// its cards, so a private DM's board is the owner's own DM; the views it
    /// hears are still scrubbed and offer no answer (`may_answer` is false:
    /// an answer is judged when it is pressed). The Reminders Home, which no
    /// place runs on, is watched too: the binding shows its tasks on the
    /// owner's DM (theseus-31pgz), with the same scrubbing.
    fn may_watch(&self, sid: &str, surface: crate::approval::Surface) -> bool {
        if matches!(self.runner.place_of(sid), Place::Shared(_)) {
            return true;
        }
        surface == crate::approval::Surface::Discord
            && (self
                .outbox
                .try_target(sid)
                .ok()
                .flatten()
                .is_some_and(|t| t.starts_with("discord:"))
                || self
                    .store
                    .get_meta::<String>(crate::wake_home::HOME_SESSION)
                    .ok()
                    .flatten()
                    .as_deref()
                    == Some(sid))
    }

    /// What a watch may see. `roots` are conversations (session ids); a
    /// guarded connection (one that reads no private text) must name them,
    /// and a root that does not speak in a shared place is left out without
    /// a word: asking is no way to learn that a private session exists.
    pub(super) fn work_scope(
        &self,
        p: &WorkWatchParams,
        guarded: bool,
        surface: crate::approval::Surface,
    ) -> Result<WorkScope, RpcFailure> {
        let Some(named) = p.roots.as_ref() else {
            if guarded {
                return Err(RpcFailure::new(
                    error_code::INVALID_PARAMS,
                    format!(
                        "{} reads no private text: name the conversations (roots) to watch",
                        surface.name()
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
            if guarded && !self.may_watch(sid, surface) {
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

impl Core {
    /// The work board's snapshot (`Push::work_snapshot`), its views stamped.
    pub(crate) fn work_snapshot_marked(
        &self,
        limit: usize,
        scope: &crate::push::WorkScope,
    ) -> (u64, Vec<theseus_protocol::work::WorkView>, u64) {
        let (position, mut views, total) = self.push.work_snapshot(limit, scope);
        self.stamp_data(&mut views);
        (position, views, total)
    }

    /// Stamp each conversation's and task's view with its data mark, read from
    /// its labels (`ContentFlags::mark`). The work board holds none (it reads no
    /// store): a snapshot and each frame it sends are stamped here as they go
    /// out.
    pub(crate) fn stamp_data(&self, views: &mut [theseus_protocol::work::WorkView]) {
        use theseus_protocol::work::WorkKind;
        for v in views {
            if let (WorkKind::Conversation | WorkKind::Task, Some(sid)) = (v.kind, &v.session_id) {
                v.data = Some(crate::content_wait::mark_of(&self.store, sid));
            }
        }
    }
}
