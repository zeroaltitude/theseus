//! The task graph in a place (M7 step 39b, theseus-ext.14): the board and
//! `/tasks`. A child of `runtime`, apart from it for the shape budget's file
//! ceiling.
//!
//! - **The route.** A `task.changed` goes to the place of its home session
//!   (`theseus_core::task_graph::home`: its root task's origin session, a
//!   task session's own parent's), not to the session whose call made it, and
//!   draws nothing else. No place routes a task session, so its tasks' board
//!   is its parent's.
//! - **The board.** The place reads its tasks as they read now and sends the
//!   tree to its lane as one live upsert (`render::BOARD_KEY`): coalesced,
//!   never replayed, made at the place's first change. The lane pins it once,
//!   and finds it again after a restart (`courier/board.rs`).
//! - **`/tasks`.** The records' tree (states, owners, claims, versions), then
//!   the task sessions of today, from `task.list`.

use theseus_protocol::tasks::TaskChanged;

use super::{Place, PlaceMsg, Shared};
use crate::courier::LaneMsg;
use crate::render::{self, Buttons, Op};

/// Hand a change to its home's place, if one routes it.
pub(super) fn route(shared: &Shared, c: &TaskChanged) {
    let records = theseus_core::task_graph::all(&shared.core.store);
    let records = match records {
        Ok(r) => r,
        Err(e) => {
            tracing::warn!(error = %format!("{e:#}"), "the task board was not routed");
            return;
        }
    };
    let home = theseus_core::task_graph::home(&records, &c.task);
    let tx = shared.routes.lock().unwrap().by_session.get(&home).cloned();
    if let Some(tx) = tx {
        let _ = tx.send(PlaceMsg::Board);
    }
}

impl Place {
    /// The board's latest state, to the lane.
    pub(super) fn board(&mut self) {
        let core = &self.shared.core;
        let records = theseus_core::task_graph::all_shown(&core.store, &core.kernel);
        let records = match records {
            Ok(r) => r,
            Err(e) => {
                tracing::warn!(place = %self.label, error = %format!("{e:#}"), "the task board was not read");
                return;
            }
        };
        let now = core.kernel.now_ms();
        if let Some(content) = render::board(&records, &self.session_id, now) {
            let _ = self.lane.send(LaneMsg::Live(Op::Upsert {
                key: render::BOARD_KEY.into(),
                content,
                buttons: Buttons::Keep,
            }));
        }
    }

    /// `/tasks`: the place's records' tree, then its task sessions of today.
    pub(super) async fn tasks_here(&self) -> String {
        match self
            .shared
            .rpc
            .call::<_, theseus_protocol::TaskListResult>(
                theseus_protocol::method::TASK_LIST,
                theseus_protocol::TaskListParams {
                    session_id: None,
                    target: Some(self.target.clone()),
                },
            )
            .await
        {
            Ok(l) => render::tasks_here(
                &l.records,
                &self.session_id,
                &l.tasks,
                theseus_protocol::now_unix_ms(),
            ),
            Err(e) => format!("⚠️ Could not list the tasks: {e}."),
        }
    }
}
