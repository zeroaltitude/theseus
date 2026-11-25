//! The undo of a change to the owner's task that ran without asking him
//! (issue 57, `task_graph::undo`). It has no method of its own: the note's
//! Undo (its button, or `theseus told undo`) is `told.undo`, which judges the
//! press and calls [`Core::task_undo_as`]. One frame: the record as it was
//! before the change, as the task's next version, and its `task.updated` row
//! naming the version it undid, under a key of its own so the door sees it
//! was done.

use serde_json::json;
use theseus_protocol::error_code;

use super::server::RpcFailure;
use super::Core;
use crate::approval::Answerer;
use crate::task_graph::{self as graph, undo};

impl Core {
    /// Undo the told change that made `version` of the task `id`, for `who`,
    /// already judged by `told.undo`.
    pub(super) fn task_undo_as(
        &self,
        id: &str,
        version: u64,
        who: &Answerer,
    ) -> Result<(), RpcFailure> {
        let tasks = graph::all(&self.store)?;
        let id = graph::resolve(&tasks, id)
            .map_err(|e| RpcFailure::new(error_code::NOT_FOUND, e))?
            .id
            .clone();
        let _lock = self.store.lock_task(&id);
        let now = graph::get(&self.store, &id)?
            .ok_or_else(|| RpcFailure::new(error_code::NOT_FOUND, "no such task"))?;
        let (task, _) = undo::restored(&self.store, &now, version, self.kernel.now_ms())
            .map_err(|e| RpcFailure::new(error_code::INVALID_PARAMS, e))?;
        let home = self.home().map_err(RpcFailure::from)?;
        let rec = self.session_rec(&home.session_id);
        let change = crate::fact::task_graph::Change {
            session_id: home.session_id.clone(),
            task: task.clone(),
            from: Some(now.version),
            detail: json!({"fields": "undo", "undid": version, "by": who.label}),
        };
        let mut row = crate::fact::task_graph::row_of(&rec, "updated", &change)?;
        row.key = Some(undo::undone_key(&task.id, version));
        self.store.append(&[graph::record(&task)?, row])?;
        crate::fact::task_graph::announce(&rec, "updated", &change);
        Ok(())
    }
}
