//! Task executions (DD7, theseus-qn2; spec §3.2a): a conversation opens a
//! task, a child execution that works on its own, and hears back when it ends.
//!
//! - **The ids.** One tool call opens a task, and the task's execution and
//!   session take that call's id (`act_X` opens `exe_X` in `ses_X`), so a call
//!   run again after a crash finds its task instead of opening another
//!   (`task_ids`).
//! - **The carve.** The task's limit is carved from what the parent has left,
//!   as a reservation of the parent's (`task:<task>`), so the parent cannot
//!   promise the same dollars twice. As the task's costs settle they become the
//!   parent's spend too, and the carve shrinks to what the task can still
//!   spend. When the task ends the carve is released, but for what it still
//!   has in flight or unknown, which stays reserved until that settles. So no
//!   session outspends its limit through its tasks, and the only way past a
//!   carve is an approved reset of the task's own spend, which the parent's
//!   spend still counts.
//! - **Depth one.** A task cannot open tasks.
//! - **The end.** The frame that ends a task (its last turn, a failed turn, a
//!   cancel) puts it on the parent's `reports`, and the parent's next turn
//!   reads them (`take_reports`).
//! - **The report's wake** (W1, theseus-lji). A task opened with
//!   `wake_parent` that finishes or fails also asks for that turn: the same
//!   frame puts it on the parent's `report_wakes`, and queues the parent for
//!   the driver when it is free, as a due wake does (`wakes::free`). A busy
//!   parent keeps the ask, and the frame that frees it queues it. Reports
//!   that land together start one turn, which reads them all. A cancelled
//!   task wakes nothing: whoever cancelled it is already there, and its
//!   report says who did.
//! - **Locks.** Every transition that writes both takes both locks, in id
//!   order (`Kernel::lock`), so the parent's own writers and its task's never lose
//!   each other's update.

use anyhow::Result;
use serde_json::json;
use theseus_protocol::LedgerKind;
use theseus_store::NewRecord;

use crate::kernel::{exec_record, Kernel, KernelError, TurnGuard};
use crate::types::*;

/// The parent's reservation that holds a task's carve.
pub fn carve_key(task_execution: &str) -> String {
    format!("task:{task_execution}")
}

/// A task's execution and session ids, from the id of the call that opens it:
/// `act_X` gives `exe_X` and `ses_X`.
pub fn task_ids(correlation_id: &str) -> (ExecutionId, SessionId) {
    let tail = correlation_id
        .split_once('_')
        .map_or(correlation_id, |(_, t)| t);
    (format!("exe_{tail}"), format!("ses_{tail}"))
}

/// What `open_task` did.
#[derive(Debug, Clone)]
pub struct TaskOpen {
    pub task: Execution,
    /// Opened by this call. False when the same tool call had opened it
    /// before (a call run again after a crash): nothing was written.
    pub opened: bool,
    /// What the parent had left before the carve.
    pub available_before: Micros,
}

/// What `take_reports` took.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct TakenReports {
    /// The tasks whose reports the turn reads, oldest first.
    pub ids: Vec<ExecutionId>,
    /// Those among them that asked for this turn (W1, `wake_parent`).
    pub woke: Vec<ExecutionId>,
}

impl Kernel {
    /// Open a task for the tool call `correlation_id` of the turn `guard`
    /// holds (DD7), in one frame under both locks:
    /// - the task: queued for the driver, its limit pinned at `want_micros`
    ///   capped at what the parent has left, the parent's authority, `parent`,
    ///   `reports_to`, and `wake_parent` (W1);
    /// - the parent's carve, a reservation of that limit;
    /// - `extra`, the core's records for the task (its session record, its
    ///   brief as the first node, where it reports);
    /// - the rows `execution.opened`, `budget.carved`, and `execution.queued`.
    ///
    /// A task cannot open tasks (`TaskDepth`), and a parent with nothing left
    /// has nothing to carve (`NothingToCarve`); either way nothing is written.
    /// A call that opened its task before gets that task back, and writes
    /// nothing.
    pub fn open_task(
        &self,
        guard: &TurnGuard,
        correlation_id: &str,
        want_micros: Micros,
        reports_to: Option<String>,
        wake_parent: bool,
        extra: impl FnOnce(&Execution) -> Result<Vec<NewRecord>>,
    ) -> Result<TaskOpen> {
        self.require_accepting()?;
        let (task_id, session_id) = task_ids(correlation_id);
        let _w = self.lock(&[&guard.execution_id, &task_id]);
        let mut parent = self
            .execution(&guard.execution_id)?
            .ok_or_else(|| KernelError::UnknownExecution(guard.execution_id.clone()))?;
        let available_before = parent.budget.available();
        if let Some(task) = self.execution(&task_id)? {
            return Ok(TaskOpen {
                task,
                opened: false,
                available_before,
            });
        }
        if parent.parent.is_some() || parent.kind == SessionKind::Task {
            return Err(KernelError::TaskDepth { id: parent.id }.into());
        }
        crate::kernel::require_turn(&parent)?;
        // Under a limit that notifies, the carve is what the task asked for
        // up to the parent's whole limit, even past what the parent has left,
        // so the task notifies at its own limit (theseus-usei).
        let limit = match self.overdraws(&parent) {
            true => want_micros.min(parent.budget.limit_micros),
            false => want_micros.min(available_before),
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
        let task = Execution {
            id: task_id,
            schema: SCHEMA,
            session_id,
            kind: SessionKind::Task,
            // The driver takes its first turn: its brief is waiting for it.
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
            reports_to,
            reports: vec![],
            wakes: vec![],
            wake_parent,
            report_wakes: vec![],
            stopped: None,
            turns: 0,
            interrupted: 0,
            resume_pending: true,
            cancel: None,
            ended_reason: None,
            created_at_ms: now,
            updated_at_ms: now,
        };
        parent.budget.reserved_micros = parent.budget.reserved_micros.saturating_add(limit);
        parent
            .budget
            .reservations
            .insert(carve_key(&task.id), limit);
        parent.updated_at_ms = now;
        let mut frame = vec![exec_record(&parent)?, exec_record(&task)?];
        frame.extend(extra(&task)?);
        frame.push(self.ledger(
            LedgerKind::ExecutionOpened,
            Some(&task.session_id),
            json!({"execution_id": task.id, "kind": task.kind, "limit_usd": micros_to_usd(limit),
                   "parent": parent.id, "reports_to": task.reports_to, "by": correlation_id,
                   "wake_parent": wake_parent}),
        )?);
        frame.push(self.ledger(
            LedgerKind::BudgetCarved,
            Some(&parent.session_id),
            json!({"execution_id": parent.id, "task": task.id, "carved_usd": micros_to_usd(limit),
                   "asked_usd": micros_to_usd(want_micros),
                   "available_before_usd": micros_to_usd(available_before),
                   "available_after_usd": micros_to_usd(parent.budget.available())}),
        )?);
        frame.push(self.ledger(
            LedgerKind::ExecutionQueued,
            Some(&task.session_id),
            json!({"execution_id": task.id, "why": "task"}),
        )?);
        self.commit(&frame)?;
        Ok(TaskOpen {
            task,
            opened: true,
            available_before,
        })
    }

    /// The reports of the tasks that ended since the turn `guard` holds last
    /// read them, oldest first (DD7). They are cleared in one frame with the
    /// records `extra` builds from them (their nodes in the session), and the
    /// row `task.reports_read`, and so are the reports' wakes (W1): this turn
    /// reads every report, whichever asked for it. When there are none,
    /// nothing is written: a plain turn pays one read.
    pub fn take_reports(
        &self,
        guard: &TurnGuard,
        extra: impl FnOnce(&[ExecutionId]) -> Result<Vec<NewRecord>>,
    ) -> Result<TakenReports> {
        let _w = self.lock(&[&guard.execution_id]);
        let mut e = self
            .execution(&guard.execution_id)?
            .ok_or_else(|| KernelError::UnknownExecution(guard.execution_id.clone()))?;
        if e.reports.is_empty() && e.report_wakes.is_empty() {
            return Ok(TakenReports::default());
        }
        let ids = std::mem::take(&mut e.reports);
        let woke = std::mem::take(&mut e.report_wakes);
        e.updated_at_ms = self.now_ms();
        let mut frame = vec![exec_record(&e)?];
        frame.extend(extra(&ids)?);
        frame.push(self.ledger(
            LedgerKind::TaskReportsRead,
            Some(&e.session_id),
            json!({"execution_id": e.id, "tasks": ids, "woke": woke, "turn": guard.turn}),
        )?);
        self.commit(&frame)?;
        Ok(TakenReports { ids, woke })
    }

    /// A task ended in the frame being built: the parent's carve keeps only
    /// what the task still has in flight or unknown, and the task joins the
    /// parent's `reports`, with a `task.ended` row. The caller holds both
    /// locks (`lock_family`). Nothing for an execution with no parent.
    ///
    /// A task opened with `wake_parent` that did not end by a cancel also
    /// asks for the parent's next turn (W1): it joins `report_wakes`, with a
    /// `task.report_wake` row, and a parent that is free (`wakes::free`) is
    /// queued for the driver in this frame, `why: report`. A busy one keeps
    /// the ask for the frame that frees it (`end_turn_with`).
    pub(crate) fn task_ended(&self, e: &Execution, frame: &mut Vec<NewRecord>) -> Result<()> {
        let Some(pid) = &e.parent else {
            return Ok(());
        };
        let Some(mut parent) = self.execution(pid)? else {
            return Ok(());
        };
        let key = carve_key(&e.id);
        let carved = parent.budget.reservations.get(&key).copied().unwrap_or(0);
        carry(&mut parent, e, e.budget.spent_micros);
        let kept = parent.budget.reservations.get(&key).copied().unwrap_or(0);
        if !parent.reports.contains(&e.id) {
            parent.reports.push(e.id.clone());
        }
        let now = self.now_ms();
        let mut woke = Vec::new();
        if e.wake_parent && e.state != ExecState::Cancelled && !parent.state.is_terminal() {
            if !parent.report_wakes.contains(&e.id) {
                parent.report_wakes.push(e.id.clone());
            }
            let queued = crate::wakes::free(&parent);
            woke.push(self.ledger(
                LedgerKind::TaskReportWake,
                Some(&parent.session_id),
                json!({"execution_id": parent.id, "task": e.id, "task_session": e.session_id,
                       "state": e.state, "parent_state": parent.state, "queued": queued}),
            )?);
            if queued {
                parent.state = ExecState::Queued;
                parent.wake = None;
                parent.resume_pending = true;
                woke.push(self.ledger(
                    LedgerKind::ExecutionQueued,
                    Some(&parent.session_id),
                    json!({"execution_id": parent.id, "why": "report", "tasks": parent.report_wakes}),
                )?);
            }
        }
        parent.updated_at_ms = now;
        frame.push(exec_record(&parent)?);
        frame.push(self.ledger(
            LedgerKind::TaskEnded,
            Some(&parent.session_id),
            json!({"execution_id": parent.id, "task": e.id, "state": e.state,
                   "reason": e.ended_reason, "spent_usd": micros_to_usd(e.budget.spent_micros),
                   "limit_usd": micros_to_usd(e.budget.limit_micros),
                   "released_usd": micros_to_usd(carved.saturating_sub(kept)),
                   "still_reserved_usd": micros_to_usd(kept)}),
        )?);
        frame.extend(woke);
        Ok(())
    }

    /// The tasks that have not ended, in id order, by their `ot` term
    /// (theseus-id8d): health's parked tasks read these, never every open
    /// execution.
    pub fn open_tasks(&self) -> Result<Vec<Execution>> {
        Ok(self
            .executions_by(&[crate::terms::one(crate::terms::OPEN_TASK)])?
            .into_iter()
            .filter(|e| e.kind == SessionKind::Task && !e.state.is_terminal())
            .collect())
    }

    /// Every task execution, oldest first; only `parent`'s when it is given.
    /// Read by their terms (theseus-lv2), never every execution.
    pub fn tasks(&self, parent: Option<&str>) -> Result<Vec<Execution>> {
        let wanted = match parent {
            Some(p) => crate::terms::one(&crate::terms::tasks_of(p)),
            None => crate::terms::prefix("t:"),
        };
        let mut v: Vec<Execution> = self
            .executions_by(&[wanted])?
            .into_iter()
            .filter(|e| match (&e.parent, parent) {
                (None, _) => false,
                (Some(_), None) => true,
                (Some(p), Some(want)) => p == want,
            })
            .collect();
        v.sort_by(|a, b| (a.created_at_ms, &a.id).cmp(&(b.created_at_ms, &b.id)));
        Ok(v)
    }
}

/// What a task's change means for its parent (DD7). The spend the task settled
/// since `spent_before` is the parent's spend too (a reset of the task's own
/// spend lowers nothing of the parent's), and the parent's carve for the task
/// shrinks to what the task can still spend: its room under its limit while it
/// runs, and what it has in flight or unknown once it has ended. A carve never
/// grows. Returns whether the parent changed.
pub(crate) fn carry(parent: &mut Execution, child: &Execution, spent_before: Micros) -> bool {
    let b = &mut parent.budget;
    let mut changed = false;
    let spent = child.budget.spent_micros.saturating_sub(spent_before);
    if spent > 0 {
        b.spent_micros = b.spent_micros.saturating_add(spent);
        changed = true;
    }
    let key = carve_key(&child.id);
    if let Some(&carved) = b.reservations.get(&key) {
        let c = &child.budget;
        let still = if child.state.is_terminal() {
            c.reserved_micros.saturating_add(c.held_unknown_micros)
        } else {
            c.limit_micros.saturating_sub(c.spent_micros)
        };
        let keep = carved.min(still);
        if keep != carved {
            b.reserved_micros = b.reserved_micros.saturating_sub(carved - keep);
            if keep == 0 {
                b.reservations.remove(&key);
            } else {
                b.reservations.insert(key, keep);
            }
            changed = true;
        }
    }
    changed
}
