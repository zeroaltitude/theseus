//! `work.watch` and `work.unwatch` (theseus-hkx30): the work board on the
//! wire. The watch subscribes this connection before it reads the board, so
//! no change falls between the snapshot and the events (the position rule
//! settles the overlap, as `executions.watch`'s does).
//!
//! Every connection may watch every conversation (2026-11-22: no surface is
//! withheld text). A connection that reads no private text is still `guarded`:
//! what it hears passes the secrets' scrubber and offers no answer
//! (`push/work/place.rs`), because a secret is a secret on any surface and an
//! answer is an act.

use std::collections::HashSet;
use std::sync::Arc;

use serde_json::json;
use theseus_protocol::{error_code, method, WorkUnwatchResult, WorkWatchParams, WorkWatchResult};

use super::server::{parse, reply, Conn, RpcFailure};
use super::Core;
use crate::push::WorkScope;

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
            method::WORK_LOOK => reply(self.work_look_for(parse(params)?, conn.surface).await?),
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
        // The secret and the act, not the surface: see the module's note.
        let guarded = !conn.surface.reads_private();
        let scope = self.work_scope(&p, guarded);
        self.push.ensure(self).await?;
        self.bus.watch_work(
            conn.client,
            conn.tx.clone(),
            scope.clone(),
            p.interactive.unwrap_or(false),
        );
        let (position, views, total) =
            self.work_snapshot_marked(p.limit.unwrap_or(DEFAULT_LIMIT) as usize, &scope);
        Ok(WorkWatchResult {
            position,
            views: scope.cut(&views, &self.tools.scrubber),
            total,
        })
    }

    fn work_unwatch(&self, conn: Conn<'_>) -> WorkUnwatchResult {
        self.bus.unwatch_work(conn.client);
        WorkUnwatchResult { watching: false }
    }

    /// What a watch may see: the conversations `roots` names (session ids),
    /// or every one. `guarded` rides the scope for the scrubber and the
    /// answer, and narrows nothing.
    pub(super) fn work_scope(&self, p: &WorkWatchParams, guarded: bool) -> WorkScope {
        let roots = p.roots.as_ref().map(|named| {
            named
                .iter()
                .filter_map(|sid| match sid == theseus_protocol::work_system::ROOT {
                    true => Some(sid.clone()),
                    false => self.session(sid).ok().and_then(|rec| rec.execution_id),
                })
                .collect::<HashSet<String>>()
        });
        WorkScope { roots, guarded }
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
