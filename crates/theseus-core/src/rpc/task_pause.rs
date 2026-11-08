//! `task.pause` and `task.resume`: the owner holds a running task at its next
//! loop and lets it go on.
//!
//! - **The hold** is the META key `task.paused.<session>` (`true`, or `false`
//!   once released) and a set in memory (`Pauses`), read back from the keys at
//!   the first ask after a start. Each change is one frame with its
//!   `task.paused`/`task.resumed` row, which the work board reads to mark the
//!   task `paused`.
//! - **A held task parks** at its next loop boundary (`TurnRunner::paused_at`,
//!   beside `steer_in`): its turn ends waiting on input and `task_end` keeps
//!   that wait, so it never reports. A model call in flight is never cut.
//! - **Resume** releases the hold and, when the task is parked, writes one
//!   owner node, `(resumed by the owner)`, and wakes its execution; the
//!   driver's next turn reads it and goes on. A task still mid-turn needs no
//!   wake: the hold was all that stopped it.
//! - Both are the owner's, judged as `session.retire` is (`Act::Session`).

use std::sync::Arc;

use serde_json::Value;
use theseus_kernel::Execution;
use theseus_protocol::task_pause::{TaskPauseParams, TaskPauseResult};
use theseus_protocol::{error_code, method};
use theseus_store::{kinds, NewRecord, Store as _};

use super::confirms::Act;
use super::server::{route, Conn, RpcFailure};
use super::Core;
use crate::approval::Answerer;
use crate::fact;
use crate::node::{Node, Origin};
use crate::turn::TurnRunner;

const KEY: &str = "task.paused.";

/// What the owner's wake says to the task.
pub const RESUMED: &str = "(resumed by the owner)";

impl TurnRunner {
    /// Whether the task session `session` is held. One atomic load while no
    /// task is; the first ask after a start reads the stored holds.
    pub(crate) fn task_paused(&self, session: &str) -> bool {
        let pauses = &self.tools.job_waits.steers.pauses;
        if !pauses.warmed() {
            self.read_pauses();
        }
        pauses.is(session)
    }

    fn read_pauses(&self) {
        let read =
            theseus_store::blocking(|| self.store.inner().latest_with_prefix(kinds::META, KEY));
        match read {
            Ok(records) => {
                let held = records.into_iter().filter_map(|r| {
                    let session = r.key.as_deref()?.strip_prefix(KEY)?.to_string();
                    r.decode::<bool>().ok().filter(|on| *on).map(|_| session)
                });
                self.tools.job_waits.steers.pauses.warm(held);
            }
            Err(e) => tracing::warn!(error = %format!("{e:#}"), "the paused tasks were not read"),
        }
    }
}

impl Core {
    pub(super) fn rpc_task_pause(
        self: &Arc<Self>,
        m: &str,
        params: Value,
        conn: Conn<'_>,
    ) -> Result<Value, RpcFailure> {
        route(params, |p: TaskPauseParams| {
            let who = conn.answerer(p.author.clone(), p.discord.clone());
            self.task_hold(&p.task, m == method::TASK_PAUSE, &who)
                .map_err(|e| match e.downcast::<crate::task::NoSuchTask>() {
                    Ok(n) => RpcFailure::new(error_code::NOT_FOUND, n.0),
                    Err(e) => RpcFailure::invalid(e),
                })
        })
    }

    /// Hold (`on`) or release the task `name` for `who`, the owner.
    pub fn task_hold(
        &self,
        name: &str,
        on: bool,
        who: &Answerer,
    ) -> anyhow::Result<TaskPauseResult> {
        let (verb, m) = match on {
            true => ("Pausing", method::TASK_PAUSE),
            false => ("Resuming", method::TASK_RESUME),
        };
        let what = format!("{verb} the task {name}");
        self.judge_act(
            who,
            Act::Session {
                method: m,
                what: &what,
            },
        )?;
        let tasks = self.kernel.tasks(None)?;
        let found = crate::task::resolve(&tasks, name).map_err(crate::task::NoSuchTask)?;
        let (id, session_id) = (found.id.clone(), found.session_id.clone());
        let by = who.label.as_str();
        // The park is read under the execution's lock, the one its turn's end
        // commits the park under, and the hold is let go under it: a resume
        // that read the task before it parked, cleared the hold, and woke
        // nothing was this race. A resume landing first is seen by the turn's
        // end (`end_and_wake`), which reads the hold in the same lock.
        let changed = self
            .kernel
            .frame(&[&id], |k| self.hold_in_frame(k, &id, on, by))?;
        if changed {
            let rec = self.rec_for(&session_id);
            match on {
                true => rec.announce(&fact::task_pause::TaskPaused {
                    task: &id,
                    session_id: &session_id,
                    by,
                }),
                false => rec.announce(&fact::task_pause::TaskResumed {
                    task: &id,
                    session_id: &session_id,
                    by,
                }),
            }
        }
        Ok(TaskPauseResult {
            task_id: session_id,
            paused: on,
            changed,
        })
    }

    /// `task_hold`'s read and writes, inside the execution's frame: whether it
    /// changed anything.
    fn hold_in_frame(
        &self,
        k: &theseus_kernel::Kernel,
        id: &str,
        on: bool,
        by: &str,
    ) -> anyhow::Result<bool> {
        let e = k
            .execution(id)?
            .ok_or_else(|| theseus_kernel::KernelError::UnknownExecution(id.to_string()))?;
        if e.state.is_terminal() {
            anyhow::bail!("Task {} has ended ({}).", e.session_id, e.state.as_str());
        }
        // Parked on the hold: a resume wakes it even when the hold is already
        // gone (a resume that raced the park found it running).
        let parked = crate::task::parked_on_hold(&e);
        if self.runner.task_paused(&e.session_id) == on && (on || !parked) {
            return Ok(false);
        }
        let key = format!("{KEY}{}", e.session_id);
        let meta = NewRecord::json(kinds::META, Some(&key), &on)?;
        let row = match on {
            true => fact::row(
                &fact::task_pause::TaskPaused {
                    task: &e.id,
                    session_id: &e.session_id,
                    by,
                },
                Some(&e.session_id),
                None,
            )?,
            false => fact::row(
                &fact::task_pause::TaskResumed {
                    task: &e.id,
                    session_id: &e.session_id,
                    by,
                },
                Some(&e.session_id),
                None,
            )?,
        };
        let mut records = vec![meta, row];
        let wake = !on && parked;
        if wake {
            records
                .push(Node::relayed(&e.session_id, None, Origin::Operator, by, RESUMED).record()?);
        }
        self.store.append(&records)?;
        self.runner
            .tools
            .job_waits
            .steers
            .pauses
            .set(&e.session_id, on);
        if wake {
            k.wake(&e.id, "resumed")?;
        }
        Ok(true)
    }

    /// The backstop for a task parked on a hold that is gone (a resume that
    /// raced the park and woke nothing): wake it, as a resume would. The
    /// driver's tick calls it for each such task it reads; `None` when the
    /// task is not one (it parked after the read, or is held).
    pub(crate) fn wake_stranded(&self, id: &str) -> anyhow::Result<Option<Execution>> {
        self.kernel.frame(&[id], |k| {
            let Some(e) = k.execution(id)? else {
                return Ok(None);
            };
            if !crate::task::parked_on_hold(&e) || self.runner.task_paused(&e.session_id) {
                return Ok(None);
            }
            let node = Node::relayed(&e.session_id, None, Origin::Operator, "the owner", RESUMED);
            self.store.append(&[node.record()?])?;
            tracing::warn!(execution_id = %e.id, "a task was parked on a hold already let go; woken");
            Ok(Some(k.wake(id, "resumed")?))
        })
    }
}

#[cfg(test)]
#[path = "tests_task_pause.rs"]
mod tests;
