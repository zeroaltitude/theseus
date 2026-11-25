//! `task.undo` (issue 57): the owner takes back a change to their task that
//! ran without asking them (`task_graph::undo`), from their terminal. Judged
//! as `task.pause` is (`Act::Session`): the owner, from a private place. The
//! note's Undo button goes through `told.undo`, which judges the press and
//! calls [`Core::task_undo_as`]. One frame: the record as it was before the
//! change, as the task's next version, and its `task.updated` row naming the
//! version it undid.

use std::sync::Arc;

use serde_json::{json, Value};
use theseus_protocol::task_undo::{TaskUndoParams, TaskUndoResult};
use theseus_protocol::{error_code, method};

use super::confirms::Act;
use super::server::{route, Conn, RpcFailure};
use super::Core;
use crate::approval::{Answerer, Refusal};
use crate::task_graph::{self as graph, undo};

impl Core {
    pub(super) fn rpc_task_undo(
        self: &Arc<Self>,
        params: Value,
        conn: Conn<'_>,
    ) -> Result<Value, RpcFailure> {
        route(params, |p: TaskUndoParams| {
            let who = conn.answerer(p.author.clone(), p.discord.clone());
            self.task_undo(&p, &who)
        })
    }

    /// Undo the told change that made `p.version` of the task `p.id`, for
    /// `who`, the owner.
    pub fn task_undo(
        &self,
        p: &TaskUndoParams,
        who: &Answerer,
    ) -> Result<TaskUndoResult, RpcFailure> {
        let what = format!("undoing the change to task {}", p.id);
        self.judge_act(
            who,
            Act::Session {
                method: method::TASK_UNDO,
                what: &what,
            },
        )
        .map_err(refused)?;
        self.task_undo_as(&p.id, p.version, who)
    }

    /// The undo itself, for `who`, already judged: by `task.undo` above, or by
    /// `told.undo` for a press of the note's Undo.
    pub(super) fn task_undo_as(
        &self,
        id: &str,
        version: u64,
        who: &Answerer,
    ) -> Result<TaskUndoResult, RpcFailure> {
        let tasks = graph::all(&self.store)?;
        let id = graph::resolve(&tasks, id)
            .map_err(|e| RpcFailure::new(error_code::NOT_FOUND, e))?
            .id
            .clone();
        let _lock = self.store.lock_task(&id);
        let now = graph::get(&self.store, &id)?
            .ok_or_else(|| RpcFailure::new(error_code::NOT_FOUND, "no such task"))?;
        let (task, restored) = undo::restored(&self.store, &now, version, self.kernel.now_ms())
            .map_err(|e| RpcFailure::new(error_code::INVALID_PARAMS, e))?;
        let home = self.home().map_err(RpcFailure::from)?;
        let rec = self.session_rec(&home.session_id);
        let change = crate::fact::task_graph::Change {
            session_id: home.session_id.clone(),
            task: task.clone(),
            from: Some(now.version),
            detail: json!({"fields": "undo", "undid": version, "by": who.label}),
        };
        self.store.append(&[
            graph::record(&task)?,
            crate::fact::task_graph::row_of(&rec, "updated", &change)?,
        ])?;
        crate::fact::task_graph::announce(&rec, "updated", &change);
        Ok(TaskUndoResult { task, restored })
    }
}

/// A refusal on the wire: who, through what, and why; nothing was undone.
fn refused(e: anyhow::Error) -> RpcFailure {
    match e.downcast::<Refusal>() {
        Ok(r) => RpcFailure {
            code: error_code::REFUSED,
            message: format!(
                "an undo from {} does not count: {}. Nothing was undone.",
                r.who, r.why
            ),
            data: json!({"who": r.who, "via": r.via, "why": r.why}),
        },
        Err(e) => RpcFailure::invalid(e),
    }
}
