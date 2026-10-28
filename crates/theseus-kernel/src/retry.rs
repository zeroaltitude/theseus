//! A failure's dismissal and a failed task's retry (theseus-7zgsm), each one
//! frame.
//!
//! - **Dismiss.** The failed execution's own record carries the owner's word
//!   (`Execution::dismissed`), written with its `execution.dismissed` row, so
//!   it survives a restart and the push's seed. Dismissing again writes
//!   nothing and answers the first time.
//! - **Retry.** A new task under the failed one's parent, with the same
//!   reports and wake, `retry_of` the failed one. Its limit is carved from
//!   what the parent has left, never more than the failed one had, as
//!   `open_task_at` carves, and it honours the same depth bound
//!   (`Kernel::depth_refusal`) though it writes its own records rather than
//!   going through `carve_task`; the failed one is dismissed in the same frame, so
//!   a crash leaves a failure with no retry or both. The core's records for
//!   the new task (its session, its brief, its pieces) ride the frame
//!   (`extra`), as a task's do.

use anyhow::Result;
use serde_json::json;
use theseus_protocol::LedgerKind;
use theseus_store::NewRecord;

use crate::kernel::{exec_record, Kernel, KernelError};

/// Why a dismissal or a retry was refused (theseus-7zgsm); nothing is written.
#[derive(Debug, thiserror::Error)]
pub enum RetryError {
    /// Only a failure is dismissed or retried.
    #[error(
        "execution {id} is {state}, not a failure: only a failed, blocked or budget-exhausted \
         execution can be dismissed or retried"
    )]
    NotAFailure { id: ExecutionId, state: String },
    /// A retry opens a task under the failed one's parent.
    #[error("{0}")]
    Refused(String),
}
use crate::tasks::{carve_key, task_ids};
use crate::types::*;

/// What `dismiss_execution` did.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Dismissed {
    pub at_ms: u64,
    /// Written by this call; false when it was dismissed before.
    pub written: bool,
}

/// What `open_retry` did.
#[derive(Debug, Clone)]
pub struct RetryOpen {
    pub task: Execution,
    pub failed: Execution,
    /// What the parent had left before the carve.
    pub available_before: Micros,
}

impl Kernel {
    /// The owner dismisses the failure `execution_id` is in. Only a failed,
    /// blocked or budget-exhausted execution has one.
    pub fn dismiss_execution(&self, execution_id: &str) -> Result<Dismissed> {
        self.require_accepting()?;
        let _w = self.lock(&[execution_id]);
        let mut e = self
            .execution(execution_id)?
            .ok_or_else(|| KernelError::UnknownExecution(execution_id.into()))?;
        if !e.state.is_failure() {
            return Err(RetryError::NotAFailure {
                id: e.id,
                state: e.state.as_str().into(),
            }
            .into());
        }
        if let Some(at_ms) = e.dismissal() {
            return Ok(Dismissed {
                at_ms,
                written: false,
            });
        }
        let now = self.now_ms();
        e.dismissed = Some(Dismissal {
            at_ms: now,
            turns: e.turns,
        });
        e.updated_at_ms = now;
        self.commit(&[exec_record(&e)?, self.dismissed_row(&e, "dismiss", None)?])?;
        Ok(Dismissed {
            at_ms: now,
            written: true,
        })
    }

    fn dismissed_row(&self, e: &Execution, by: &str, retry: Option<&str>) -> Result<NewRecord> {
        self.ledger(
            LedgerKind::ExecutionDismissed,
            Some(&e.session_id),
            json!({"execution_id": e.id, "kind": e.kind, "state": e.state,
                   "reason": e.ended_reason, "turns": e.turns, "via": by,
                   "retried_as": retry}),
        )
    }

    /// The failed task `failed_id` and its parent, read under their locks:
    /// refused, nothing written, for an execution that is no task, no
    /// failure, or has no parent on the record.
    fn failed_and_parent(&self, failed_id: &str) -> Result<(Execution, Execution)> {
        let failed = self
            .execution(failed_id)?
            .ok_or_else(|| KernelError::UnknownExecution(failed_id.into()))?;
        if failed.kind != SessionKind::Task {
            return Err(RetryError::Refused(format!(
                "execution {failed_id} is not a task: only a failed task can be retried"
            ))
            .into());
        }
        if !failed.state.is_failure() {
            return Err(RetryError::NotAFailure {
                id: failed.id,
                state: failed.state.as_str().into(),
            }
            .into());
        }
        let Some(pid) = failed.parent.clone() else {
            return Err(RetryError::Refused(format!(
                "task {failed_id} has no parent to carve a retry from"
            ))
            .into());
        };
        let Some(parent) = self.execution(&pid)? else {
            return Err(RetryError::Refused(format!(
                "task {failed_id}'s parent {pid} is gone, so a retry has nothing to carve its \
                 budget from or to report to"
            ))
            .into());
        };
        Ok((failed, parent))
    }

    /// Open a retry of the failed task `failed_id` as the task the call
    /// `correlation_id` (`act_X`) names: `exe_X` in `ses_X`. In one frame, under
    /// the failed task's, its parent's and the new task's locks: the new
    /// task (queued, the parent's authority, the failed one's reports and
    /// wake), the parent's carve, the failed task dismissed, `extra`'s
    /// records (the core's), and the rows `execution.opened`,
    /// `budget.carved`, `execution.queued` and `execution.dismissed`. A call
    /// run again finds its task and writes nothing.
    ///
    /// Refused, nothing written, when the execution is no failed task, or
    /// its parent is gone (missing, cancelled or complete) or has nothing
    /// left to carve.
    pub fn open_retry(
        &self,
        failed_id: &str,
        correlation_id: &str,
        extra: impl FnOnce(&Execution, &Execution) -> Result<Vec<NewRecord>>,
    ) -> Result<RetryOpen> {
        self.require_accepting()?;
        let (task_id, session_id) = task_ids(correlation_id);
        let parent_id = self
            .execution(failed_id)?
            .ok_or_else(|| KernelError::UnknownExecution(failed_id.into()))?
            .parent;
        let mut ids = vec![failed_id, task_id.as_str()];
        if let Some(p) = &parent_id {
            ids.push(p);
        }
        let _w = self.lock(&ids);
        let (mut failed, mut parent) = self.failed_and_parent(failed_id)?;
        let available_before = parent.budget.available();
        if let Some(task) = self.execution(&task_id)? {
            return Ok(RetryOpen {
                task,
                failed,
                available_before,
            });
        }
        if matches!(parent.state, ExecState::Cancelled | ExecState::Complete) {
            return Err(RetryError::Refused(format!(
                "task {failed_id}'s parent {} has ended ({}), so a retry has nothing to carve \
                 its budget from or to report to",
                parent.id,
                parent.state.as_str()
            ))
            .into());
        }
        // The depth bound, as every open of a task honours it (theseus-rrqmr).
        // This one writes the task's record and the carve itself rather than
        // through `carve_task`, so it asks separately. A retry re-opens under
        // the failed task's own parent, so it keeps the failed one's depth and
        // can pass a bound the failed one already passed; asking costs nothing
        // and leaves no open of a task that does not.
        if let Some(e) = self.depth_refusal(&parent)? {
            return Err(e.into());
        }
        // Never more than the failed one had, and no more than the parent has
        // left; under a limit that notifies, up to the parent's whole limit.
        let want = failed.budget.limit_micros;
        let limit = match self.overdraws(&parent) {
            true => want.min(parent.budget.limit_micros),
            false => want.min(available_before),
        };
        if limit == 0 {
            return Err(KernelError::NothingToCarve {
                available: available_before,
                spent: parent.budget.spent_micros,
                limit: parent.budget.limit_micros,
            }
            .into());
        }
        let now = self.now_ms();
        let task = new_task(task_id, session_id, &failed, &parent, limit, now);
        parent.budget.reserved_micros = parent.budget.reserved_micros.saturating_add(limit);
        parent
            .budget
            .reservations
            .insert(carve_key(&task.id), limit);
        parent.updated_at_ms = now;
        failed.dismissed = Some(Dismissal {
            at_ms: now,
            turns: failed.turns,
        });
        failed.updated_at_ms = now;
        let mut frame = vec![
            exec_record(&parent)?,
            exec_record(&task)?,
            exec_record(&failed)?,
        ];
        frame.extend(extra(&failed, &task)?);
        frame.push(self.opened_row(&task, &parent.id, limit, correlation_id)?);
        frame.push(self.ledger(
            LedgerKind::BudgetCarved,
            Some(&parent.session_id),
            json!({"execution_id": parent.id, "task": task.id, "carved_usd": micros_to_usd(limit),
                   "asked_usd": micros_to_usd(want), "retry_of": failed.id,
                   "available_before_usd": micros_to_usd(available_before),
                   "available_after_usd": micros_to_usd(parent.budget.available())}),
        )?);
        frame.push(self.start_row(&task, None)?);
        frame.push(self.dismissed_row(&failed, "retry", Some(&task.id))?);
        self.commit(&frame)?;
        Ok(RetryOpen {
            task,
            failed,
            available_before,
        })
    }
}

/// The queued task that retries `failed` under `parent`, with `limit` carved.
fn new_task(
    id: ExecutionId,
    session_id: SessionId,
    failed: &Execution,
    parent: &Execution,
    limit: Micros,
    now: u64,
) -> Execution {
    Execution {
        id,
        schema: SCHEMA,
        session_id,
        kind: SessionKind::Task,
        state: ExecState::Queued,
        authority: parent.authority.clone(),
        budget: Budget {
            pinned: true,
            ..Budget::new(limit)
        },
        wake: None,
        outstanding: vec![],
        queued_results: vec![],
        parent: Some(parent.id.clone()),
        reports_to: failed.reports_to.clone(),
        reports: vec![],
        wakes: vec![],
        wake_parent: failed.wake_parent,
        report_wakes: vec![],
        helper: None,
        stopped: None,
        turns: 0,
        interrupted: 0,
        resume_pending: true,
        cancel: None,
        ended_reason: None,
        dismissed: None,
        retry_of: Some(failed.id.clone()),
        created_at_ms: now,
        updated_at_ms: now,
    }
}
