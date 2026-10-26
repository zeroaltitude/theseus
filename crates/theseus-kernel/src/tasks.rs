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
//! - **Depth one.** A task cannot open tasks. The bound lives in one place,
//!   `depth_refusal`, and `carve_task` — the only way a task is ever opened —
//!   checks it, so no caller can skip it (theseus-rrqmr: a routine's fire
//!   did).
//! - **The spend carries all the way up.** What a task settles is the spend
//!   of every execution above it, not only its parent's, and each carve above
//!   it shrinks to what the execution under it can still spend
//!   (`Kernel::carry_to_parent`, theseus-deaqt). At depth one that is one
//!   level; deeper, it is the whole chain to the conversation, whose own
//!   spend-limit notice reads the `spent_micros` it raises.
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
//! - **Locks.** Every transition that writes a task and the executions above
//!   it takes all of their locks, in id order, in one call (`lock_family`,
//!   whose doc argues why that cannot deadlock however long the chain), so
//!   that a conversation's own writers and its tasks' never lose each other's
//!   update.

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

/// How a task opens: where it reports, whether its report wakes the parent,
/// and, for a deferred task, when it starts (theseus-rcgph).
#[derive(Debug, Clone, Default)]
pub struct Opening {
    pub reports_to: Option<String>,
    pub wake_parent: bool,
    /// Asleep until then: `Waiting{DueAt}`, queued by the due scan.
    pub start_at_ms: Option<u64>,
    /// The fan-out call (its correlation id) that opened this task as one of
    /// its helpers: see `Execution::helper_of`. None for every other task.
    pub helper: Option<String>,
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
        let how = Opening {
            reports_to,
            wake_parent,
            start_at_ms: None,
            helper: None,
        };
        self.open_task_at(guard, correlation_id, want_micros, how, extra)
    }

    /// `open_task`, asleep when `start_at_ms` is given (theseus-rcgph): the
    /// task and its carve are written as they are for any task, in the same
    /// one frame, but the execution is `Waiting{DueAt}` instead of queued and
    /// asks the driver for nothing. The due scan queues it when the time
    /// comes (`wakes::due_now`), and its first turn is its ordinary first
    /// turn. The carve is held from now, so the parent cannot spend it
    /// meanwhile; a cancel before the start releases it as any task's does.
    pub fn open_task_at(
        &self,
        guard: &TurnGuard,
        correlation_id: &str,
        want_micros: Micros,
        how: Opening,
        extra: impl FnOnce(&Execution) -> Result<Vec<NewRecord>>,
    ) -> Result<TaskOpen> {
        self.open_task_in(
            &guard.execution_id,
            true,
            correlation_id,
            want_micros,
            how,
            extra,
        )
    }

    /// `open_task_at` for the owner's own start (`task.start`, theseus-7oq0w): the
    /// parent is a conversation `parent_id` names, and no turn of it need run.
    /// Everything else is the model's open, in the same one frame: the carve,
    /// the depth rule, the rows, and a call (here, `correlation_id`) run again
    /// finds its task and writes nothing.
    pub fn open_task_owned(
        &self,
        parent_id: &str,
        correlation_id: &str,
        want_micros: Micros,
        how: Opening,
        extra: impl FnOnce(&Execution) -> Result<Vec<NewRecord>>,
    ) -> Result<TaskOpen> {
        self.open_task_in(parent_id, false, correlation_id, want_micros, how, extra)
    }

    /// The one open both callers share. `needs_turn`: the parent must be
    /// running the turn whose tool call opens the task (the model's side); the
    /// owner's start needs only a parent that has not ended.
    fn open_task_in(
        &self,
        parent_id: &str,
        needs_turn: bool,
        correlation_id: &str,
        want_micros: Micros,
        how: Opening,
        extra: impl FnOnce(&Execution) -> Result<Vec<NewRecord>>,
    ) -> Result<TaskOpen> {
        self.require_accepting()?;
        let Opening {
            reports_to,
            wake_parent,
            start_at_ms,
            helper,
        } = how;
        let (task_id, session_id) = task_ids(correlation_id);
        let _w = self.lock(&[parent_id, &task_id]);
        let mut parent = self
            .execution(parent_id)?
            .ok_or_else(|| KernelError::UnknownExecution(parent_id.to_string()))?;
        let available_before = parent.budget.available();
        if let Some(task) = self.execution(&task_id)? {
            return Ok(TaskOpen {
                task,
                opened: false,
                available_before,
            });
        }
        // Early, so that a parent that is itself a task is refused as
        // `TaskDepth` and not as `NothingToCarve`; `carve_task` checks it
        // again, and that is the check no caller can skip (`depth_refusal`).
        if let Some(e) = depth_refusal(&parent) {
            return Err(e.into());
        }
        if needs_turn {
            crate::kernel::require_turn(&parent)?;
        } else if parent.state.is_terminal() {
            return Err(KernelError::NoTurn {
                id: parent.id,
                state: parent.state.as_str(),
            }
            .into());
        }
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
        let (task, frame) = self.carve_task(
            &mut parent,
            CarveFor {
                task_id,
                session_id,
                limit,
                want_micros,
                available_before,
                by: correlation_id,
                how: Opening {
                    reports_to,
                    wake_parent,
                    start_at_ms,
                    helper,
                },
            },
            extra,
        )?;
        self.commit(&frame)?;
        Ok(TaskOpen {
            task,
            opened: true,
            available_before,
        })
    }

    /// The row `execution.opened` of a new task.
    pub(crate) fn opened_row(
        &self,
        task: &Execution,
        parent: &str,
        limit: Micros,
        by: &str,
    ) -> Result<NewRecord> {
        let mut row = json!({"execution_id": task.id, "kind": task.kind,
            "limit_usd": micros_to_usd(limit), "parent": parent, "reports_to": task.reports_to,
            "by": by, "wake_parent": task.wake_parent});
        if let Some(Wake::DueAt { at_ms }) = task.wake {
            row["starts_at_ms"] = json!(at_ms);
        }
        self.ledger(LedgerKind::ExecutionOpened, Some(&task.session_id), row)
    }

    /// The row that says how a new task begins: queued for the driver, or,
    /// deferred, waiting for its start (theseus-rcgph).
    pub(crate) fn start_row(
        &self,
        task: &Execution,
        start_at_ms: Option<u64>,
    ) -> Result<NewRecord> {
        match start_at_ms {
            Some(at) => self.ledger(
                LedgerKind::ExecutionWaiting,
                Some(&task.session_id),
                json!({"execution_id": task.id, "why": "deferred", "wake": task.wake,
                       "starts_at_ms": at}),
            ),
            None => self.ledger(
                LedgerKind::ExecutionQueued,
                Some(&task.session_id),
                json!({"execution_id": task.id, "why": "task"}),
            ),
        }
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
        self.take_reports_where((&guard.execution_id, guard.turn), |_| true, None, extra)
    }

    /// `take_reports` for the named tasks only (the in-turn wait, theseus-z9mze):
    /// the reports among `ids` that are on the parent's `reports`, oldest
    /// first, in one frame with the records `extra` builds from them and a
    /// `task.reports_read` row whose detail adds `"via": "wait"`. A named task
    /// that has not ended is not taken and not an error: `TakenReports::ids`
    /// says which were taken, and the caller tells the running ones by what is
    /// missing. When none of them has ended, nothing is written. Every other
    /// report stays for the parent's next turn, and a taken one is never
    /// delivered again.
    ///
    /// The taken tasks leave `report_wakes` too: the turn that takes them is
    /// reading them now, so a wake kept for them would queue the parent for a
    /// turn with nothing to read. A wake of a task not named stays, and with
    /// it the parent's next turn.
    pub fn take_reports_of(
        &self,
        guard: &TurnGuard,
        ids: &[ExecutionId],
        extra: impl FnOnce(&[ExecutionId]) -> Result<Vec<NewRecord>>,
    ) -> Result<TakenReports> {
        self.take_reports_in_turn(&guard.execution_id, guard.turn, ids, extra)
    }

    /// `take_reports_of` for a caller that cannot hold the turn's guard: a
    /// tool that waits in its own task (`task.wait`) names the turn it runs in
    /// by its execution and number instead. The turn is still running, as the
    /// guard's is, so the frame and its locks are the same.
    pub fn take_reports_in_turn(
        &self,
        execution_id: &ExecutionId,
        turn: u64,
        ids: &[ExecutionId],
        extra: impl FnOnce(&[ExecutionId]) -> Result<Vec<NewRecord>>,
    ) -> Result<TakenReports> {
        self.take_reports_where(
            (execution_id, turn),
            |id| ids.contains(id),
            Some("wait"),
            extra,
        )
    }

    /// What `take_reports` and `take_reports_of` share: the reports `wanted`
    /// takes, with their wakes, in one frame. `via` names the way in the row.
    fn take_reports_where(
        &self,
        (execution_id, turn): (&ExecutionId, u64),
        wanted: impl Fn(&ExecutionId) -> bool,
        via: Option<&str>,
        extra: impl FnOnce(&[ExecutionId]) -> Result<Vec<NewRecord>>,
    ) -> Result<TakenReports> {
        let _w = self.lock(&[execution_id]);
        let mut e = self
            .execution(execution_id)?
            .ok_or_else(|| KernelError::UnknownExecution(execution_id.clone()))?;
        // A wait names its turn by number, not by guard: it takes nothing for
        // a turn that is no longer this execution's running one.
        if via.is_some() && e.turns != turn {
            return Err(KernelError::NoTurn {
                id: e.id.clone(),
                state: e.state.as_str(),
            }
            .into());
        }
        if e.reports.is_empty() && e.report_wakes.is_empty() {
            return Ok(TakenReports::default());
        }
        let (ids, kept): (Vec<_>, Vec<_>) = std::mem::take(&mut e.reports)
            .into_iter()
            .partition(|id| wanted(id));
        if via.is_some() && ids.is_empty() {
            return Ok(TakenReports::default());
        }
        let (woke, kept_wakes): (Vec<_>, Vec<_>) = std::mem::take(&mut e.report_wakes)
            .into_iter()
            .partition(|id| wanted(id));
        e.reports = kept;
        e.report_wakes = kept_wakes;
        e.updated_at_ms = self.now_ms();
        let mut frame = vec![exec_record(&e)?];
        frame.extend(extra(&ids)?);
        let mut detail = json!({"execution_id": e.id, "tasks": ids, "woke": woke, "turn": turn});
        if let Some(via) = via {
            detail["via"] = json!(via);
        }
        frame.push(self.ledger(LedgerKind::TaskReportsRead, Some(&e.session_id), detail)?);
        self.commit(&frame)?;
        Ok(TakenReports { ids, woke })
    }

    /// A task's change reaches every execution above it (DD7, theseus-deaqt):
    /// under the whole family's locks, which the caller holds (`lock_family`,
    /// or `Kernel::frame` inside a transaction), `carry` applies to the
    /// parent, then to that one's parent, up to the conversation, and each
    /// record that changed joins `frame`. So the spend a task settled is the
    /// spend of every execution above it, and each carve shrinks to what the
    /// execution under it can still spend; a conversation, whose own
    /// spend-limit notice reads `spent_micros`, therefore sees what a
    /// grandchild spent. `spent_before` is the task's spend before the change;
    /// at each level above, what that level had spent before its own carry is
    /// what the level above carries from — the same money, one level up.
    ///
    /// The walk stops at the first level `carry` leaves unchanged: a level
    /// that did not change passes nothing on. Returns the immediate parent
    /// when it changed. Nothing for an execution with no parent.
    pub(crate) fn carry_to_parent(
        &self,
        child: &Execution,
        mut spent_before: Micros,
        frame: &mut Vec<NewRecord>,
    ) -> Result<Option<Execution>> {
        let now = self.now_ms();
        let mut below = std::borrow::Cow::Borrowed(child);
        let mut first = None;
        // Only a corrupt store could hold a chain that names an id twice; it
        // stops the walk rather than looping (`family`).
        let mut seen: std::collections::HashSet<String> = std::collections::HashSet::new();
        seen.insert(child.id.clone());
        while let Some(pid) = below.parent.clone() {
            let Some(mut parent) = self.execution(&pid)?.filter(|_| seen.insert(pid.clone()))
            else {
                break;
            };
            let was = parent.budget.spent_micros;
            if !carry(&mut parent, &below, spent_before) {
                break;
            }
            parent.updated_at_ms = now;
            frame.push(exec_record(&parent)?);
            if first.is_none() {
                first = Some(parent.clone());
            }
            spent_before = was;
            below = std::borrow::Cow::Owned(parent);
        }
        Ok(first)
    }

    /// A task ended in the frame being built: the parent's carve keeps only
    /// what the task still has in flight or unknown, and the task joins the
    /// parent's `reports`, with a `task.ended` row. The caller holds the whole
    /// family's locks (`lock_family`). Nothing for an execution with no parent.
    ///
    /// **And the release goes up the chain** (theseus-deaqt). A parent that is
    /// itself a task, and has ended, can still spend only what it has in
    /// flight or unknown — and this task's carve was part of that — so
    /// releasing it changes what the execution above the parent must keep
    /// carved for the parent, and that one in turn. At depth one the parent is
    /// a conversation and there is nothing above it. Without this, a tree of
    /// three levels left the conversation holding a carve for a task that had
    /// ended and could spend nothing, which is the same one-level bug
    /// `carry_to_parent` had: kernel-sim's carve invariant catches it in a few
    /// hundred steps once the bound allows a grandchild.
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
        // The release changed what the PARENT can still spend, when the
        // parent is itself a task that has ended: its `still` is what it has
        // in flight or unknown, and this task's carve was part of that. So
        // the change goes up the chain as a settle's does (theseus-deaqt;
        // `carry_to_parent` with no spend of its own, the carve alone). At
        // depth one the parent is a conversation and this does nothing.
        self.carry_to_parent(&parent, parent.budget.spent_micros, frame)?;
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

    /// The tasks below `root`, children before the task they belong to (depth first, oldest first among
    /// siblings), whatever their state: a task that has ended is walked through, since a task below it is still
    /// the root's (theseus-cdcwr). One index lookup per task, never a scan; a task met twice is named once.
    pub fn task_tree(&self, root: &str) -> Result<Vec<Execution>> {
        fn below(
            k: &Kernel,
            id: &str,
            seen: &mut std::collections::HashSet<String>,
            out: &mut Vec<Execution>,
        ) -> Result<()> {
            for t in k.tasks(Some(id))? {
                if seen.insert(t.id.clone()) {
                    below(k, &t.id.clone(), seen, out)?;
                    out.push(t);
                }
            }
            Ok(())
        }
        let mut out = Vec::new();
        below(self, root, &mut std::collections::HashSet::new(), &mut out)?;
        Ok(out)
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

/// How deep a task tree may go (DD7: depth one — a task cannot open tasks).
/// `Some` is the refusal for opening a task under `parent`.
///
/// **One place, and one place only.** `carve_task` checks this, and
/// `carve_task` is the only way a task is ever opened, so no caller can open
/// one past the bound. The shared open (`open_task_in`) checks it as well,
/// early, so that its refusal reads `TaskDepth` rather than `NothingToCarve`;
/// a caller that does not go through the shared open is refused all the same.
/// A routine's fire was exactly such a caller (theseus-rrqmr): it called
/// `carve_task` with nothing to say about depth, so a routine's series set on
/// a task carved a task under a task, which the bound could say nothing
/// about. A step that moves the bound — `theseus-tecj5` raises it to ten —
/// changes this function, and the `a_task` predicate kernel-sim uses to
/// expect the refusal, and nothing else.
pub fn depth_refusal(parent: &Execution) -> Option<KernelError> {
    (parent.parent.is_some() || parent.kind == SessionKind::Task).then(|| KernelError::TaskDepth {
        id: parent.id.clone(),
    })
}

/// What `carve_task` opens: the ids, the limit, and how it begins.
pub(crate) struct CarveFor<'a> {
    pub task_id: ExecutionId,
    pub session_id: SessionId,
    pub limit: Micros,
    pub want_micros: Micros,
    pub available_before: Micros,
    pub by: &'a str,
    pub how: Opening,
}

impl Kernel {
    /// The records of a task opening, shared by `open_task_at` (a turn's
    /// call) and a routine's fire (`routines.rs`): the task, the carve in the
    /// parent's reservations, and the frame's records, which the caller
    /// commits. The parent is changed in place.
    ///
    /// **The depth bound is checked here**, because this is the only place a
    /// task is ever opened, and a caller that opened one past the bound was
    /// the bug (theseus-rrqmr). `TaskDepth` leaves `parent` and the frame
    /// untouched, so a caller that stages into a frame of its own and commits
    /// only on `Ok` writes nothing.
    pub(crate) fn carve_task(
        &self,
        parent: &mut Execution,
        c: CarveFor<'_>,
        extra: impl FnOnce(&Execution) -> Result<Vec<NewRecord>>,
    ) -> Result<(Execution, Vec<NewRecord>)> {
        if let Some(e) = depth_refusal(parent) {
            return Err(e.into());
        }
        let CarveFor {
            task_id,
            session_id,
            limit,
            want_micros,
            available_before,
            by: correlation_id,
            how:
                Opening {
                    reports_to,
                    wake_parent,
                    start_at_ms,
                    helper,
                },
        } = c;
        let now = self.now_ms();
        let task = Execution {
            id: task_id,
            schema: SCHEMA,
            session_id,
            kind: SessionKind::Task,
            // The driver takes its first turn: its brief is waiting for it.
            // A deferred task sleeps until its start (the due scan).
            state: match start_at_ms {
                Some(_) => ExecState::Waiting,
                None => ExecState::Queued,
            },
            authority: parent.authority.clone(),
            budget: Budget {
                pinned: true,
                ..Budget::new(limit)
            },
            wake: start_at_ms.map(|at_ms| Wake::DueAt { at_ms }),
            outstanding: vec![],
            queued_results: vec![],
            parent: Some(parent.id.clone()),
            reports_to,
            reports: vec![],
            wakes: vec![],
            wake_parent,
            report_wakes: vec![],
            helper,
            stopped: None,
            turns: 0,
            interrupted: 0,
            resume_pending: start_at_ms.is_none(),
            cancel: None,
            ended_reason: None,
            dismissed: None,
            retry_of: None,
            created_at_ms: now,
            updated_at_ms: now,
        };
        parent.budget.reserved_micros = parent.budget.reserved_micros.saturating_add(limit);
        parent
            .budget
            .reservations
            .insert(carve_key(&task.id), limit);
        parent.updated_at_ms = now;
        let mut frame = vec![exec_record(parent)?, exec_record(&task)?];
        frame.extend(extra(&task)?);
        frame.push(self.opened_row(&task, &parent.id, limit, correlation_id)?);
        frame.push(self.ledger(
            LedgerKind::BudgetCarved,
            Some(&parent.session_id),
            json!({"execution_id": parent.id, "task": task.id, "carved_usd": micros_to_usd(limit),
                   "asked_usd": micros_to_usd(want_micros),
                   "available_before_usd": micros_to_usd(available_before),
                   "available_after_usd": micros_to_usd(parent.budget.available())}),
        )?);
        frame.push(self.start_row(&task, start_at_ms)?);
        Ok((task, frame))
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
