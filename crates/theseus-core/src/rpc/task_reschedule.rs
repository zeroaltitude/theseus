//! `task.reschedule` (theseus-jid7t.2): the start of a deferred task moves.
//!
//! - **Only a task that has not started**: asleep in `Waiting{DueAt}` with no
//!   turn taken (`task_start::starts_at`), which is what `TaskInfo.starts_at_ms`
//!   shows. A running, ended or held task is refused with its state.
//! - **The time** is exactly one of the three `task.start` takes, parsed by
//!   the same function (`Core::start_time`) inside the same bounds, so a time
//!   in the past is refused.
//! - **What moves**: the execution's due wake (the due scan wakes it at the new
//!   time, never the old) and the META record of its times, whose `set_at_ms`
//!   stays, in one frame with the `task.rescheduled` row. The answer's
//!   `previous` is the undo: one more call with it as `start_at_ms`.
//! - **Authority** is `task.start`'s (`Act::Task`).

use std::sync::Arc;

use serde_json::Value;
use theseus_kernel::Wake;
use theseus_protocol::method;
use theseus_protocol::ops::{TaskRescheduleParams, TaskRescheduleResult};
use theseus_store::NewRecord;

use super::confirms::Act;
use super::server::{route, Conn, RpcFailure};
use super::task_rehome::task_failure;
use super::Core;
use crate::approval::Answerer;
use crate::fact;
use crate::task_start::Deferred;

impl Core {
    pub(super) fn rpc_task_reschedule(
        self: &Arc<Self>,
        params: Value,
        conn: Conn<'_>,
    ) -> Result<Value, RpcFailure> {
        route(params, |p: TaskRescheduleParams| {
            let who = conn.answerer(p.author.clone(), p.discord.clone());
            self.task_reschedule(&p, &who)
                .map_err(task_failure("a task's start", method::TASK_RESCHEDULE))
        })
    }

    /// Move the start of the deferred task `p.task` for `who`.
    pub fn task_reschedule(
        &self,
        p: &TaskRescheduleParams,
        who: &Answerer,
    ) -> anyhow::Result<TaskRescheduleResult> {
        let what = format!("Moving the start of task {}", p.task);
        self.judge_act(who, Act::Task { what: &what })?;
        let tasks = self.kernel.tasks(None)?;
        let e = crate::task::resolve(&tasks, &p.task)
            .map_err(crate::task::NoSuchTask)?
            .clone();
        let Some(previous) = crate::task_start::starts_at(&e) else {
            anyhow::bail!(
                "Task {} is not waiting to start ({}): only a deferred task that has not started can move",
                e.session_id,
                e.state.as_str()
            );
        };
        anyhow::ensure!(
            !self.runner.task_paused(&e.session_id),
            "Task {} is paused; resume it first",
            e.session_id
        );
        let now = self.kernel.now_ms();
        let to = self
            .start_time(p.start_at_ms, p.start_after_ms, &p.start_when, now)
            .map_err(|f| anyhow::anyhow!("{}", f.message.replace("Not started", "Not moved")))?
            .ok_or_else(|| {
                anyhow::anyhow!("give one of `start_at_ms`, `start_after_ms` and `start_when`")
            })?;
        let sid = e.session_id.as_str();
        let by = who.label.as_str();
        let times = self
            .store
            .get_meta::<Deferred>(&crate::task_start::key(sid))?;
        let moved = fact::ops::TaskRescheduled {
            task: &e.id,
            session_id: sid,
            from: previous,
            to,
            by,
        };
        let row = fact::row(&moved, Some(sid), None)?;
        self.kernel.edit_task(&e.id, |ex| {
            // The task may have started between the read and the lock.
            anyhow::ensure!(
                crate::task_start::starts_at(ex) == Some(previous),
                "Task {sid} started or moved while it was being moved"
            );
            ex.wake = Some(Wake::DueAt { at_ms: to });
            let mut frame = vec![row];
            if let Some(d) = times {
                let d = Deferred { due_at_ms: to, ..d };
                frame.push(crate::task_start::record(sid, &d)?);
            }
            Ok(((), frame as Vec<NewRecord>))
        })?;
        self.rec_for(sid).announce(&moved);
        Ok(TaskRescheduleResult {
            task: sid.to_string(),
            starts_at_ms: to,
            previous,
        })
    }
}

#[cfg(test)]
#[path = "tests_task_reschedule.rs"]
mod tests;
