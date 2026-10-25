//! Routines' firing (theseus-pvm5c; the horizon report's §5.1, §5.4): a
//! routine is a conversation's execution that holds one pending wake with
//! `purpose: Routine` and `run: Task(TaskRun)`. The wake is its schedule; each
//! occurrence that fires opens a fresh task under the routine's execution, so
//! a run's context never grows with the runs before it, and its cost is the
//! routine's month (the execution's own limit), each run carved from what the
//! month has left.
//!
//! - **One frame per fire.** `fire_routines` takes every due, unpaused task wake
//!   of an execution under its lock and, in one frame: re-arms the series (the
//!   next occurrence, numbered past those passed over), and either opens the
//!   run's task with `open_task`'s records (the carve in the routine's
//!   reservations, the task, the core's records for it) and a `wake.fired` row,
//!   or skips it with a `wake.skipped { why }` row. A month's reset, when the
//!   core says the month turned, is a `budget.reset { why: "month" }` row in the
//!   same frame. Like a notice, it fires in any state of the execution, and the
//!   due scan finds the executions by the `nw` term: no new timer.
//! - **The decision is the core's.** It knows the routine's record, whether the
//!   last run still works, the month, and the catch-up rule; the kernel gives it
//!   what the wake and the budget say (`RoutineDue`) and applies its plan
//!   (`RoutinePlan`) in the frame. The plan runs under the execution's lock, so it
//!   reads and never calls a kernel transition on that execution.
//! - **Run now** (`run_routine_now`) opens one task from the routine's wake without
//!   touching the series, whether the wake is paused or not.
//! - **A task's report** is not read by anyone in the routine's own execution, so
//!   each fire clears the `reports` the earlier runs left there.

use anyhow::Result;
use serde_json::json;
use std::sync::atomic::Ordering;
use theseus_protocol::LedgerKind;
use theseus_store::NewRecord;

use crate::kernel::{exec_record, Kernel, KernelError};
use crate::outbox::Post;
use crate::tasks::{CarveFor, Opening};
use crate::types::*;
use crate::wake_kinds::{Purpose, TaskRun, WakeRun};
use crate::wakes::FiredWake;

/// What a fire knows, for the core to decide on.
#[derive(Debug, Clone)]
pub struct RoutineDue {
    /// The wake that came due, as taken: how late, whether the daemon was
    /// down, the occurrences passed over, and the next time.
    pub fired: FiredWake,
    /// The brief and the run's grant.
    pub task: TaskRun,
    /// A person asked for this one (`run_routine_now`): the series is untouched.
    pub manual: bool,
    pub now_ms: u64,
    /// The execution's limit (the month's) and what it has spent.
    pub limit_micros: Micros,
    pub spent_micros: Micros,
    /// What a run may carve now, and what it could after a reset of the spend.
    pub available_micros: Micros,
    pub available_after_reset_micros: Micros,
    /// The ids the run's task will have: the core builds its session's
    /// records from them.
    pub task_id: ExecutionId,
    pub session_id: SessionId,
}

/// What the core decided.
pub enum RoutinePlan {
    /// Not this time: the occurrence is passed, the series goes on.
    Skip {
        /// `overlap`, `budget`, `paused`, or `catch_up`.
        why: &'static str,
        /// A notice for the owner, when the skip is worth saying.
        post: Option<Post>,
        /// The core's own records (the routine's record, noting a month told).
        extra: Vec<NewRecord>,
    },
    /// Open the run.
    Run(RunPlan),
}

/// A run to open.
pub struct RunPlan {
    /// The per-run limit asked for; it is carved from what the month has left.
    pub want_micros: Micros,
    /// The month turned: spend starts again at zero.
    pub reset_month: bool,
    /// Where the run's report goes (`discord:dm:<user>`), when somewhere.
    pub reports_to: Option<String>,
    /// The core's records for the task: its session, its brief, its binding.
    pub records: Vec<NewRecord>,
    /// And any beside it: the routine's own record (run count, last run).
    pub extra: Vec<NewRecord>,
}

/// One fire's outcome.
#[derive(Debug, Clone)]
pub struct RoutineFired {
    pub wake: FiredWake,
    /// The run's task, when one was opened.
    pub task: Option<Execution>,
    /// Why it was skipped, when it was.
    pub skipped: Option<&'static str>,
    /// The post the skip staged.
    pub post: Option<Action>,
    pub reset_month: bool,
    pub manual: bool,
}

impl Kernel {
    /// The routine executions whose wake is due: the due scan reads the `nw`
    /// term (`notice_executions`) and filters by `task_due`.
    pub fn routine_due(e: &Execution, now_ms: u64) -> bool {
        !e.state.is_terminal()
            && e.wakes
                .iter()
                .any(|w| w.purpose == Purpose::Routine && w.task_due(now_ms))
    }

    /// Fire `execution_id`'s due routine wakes, each in the one frame this
    /// module's doc says. `plan` decides each from what it is given.
    pub fn fire_routines(
        &self,
        execution_id: &str,
        mut plan: impl FnMut(&Execution, &RoutineDue) -> Result<RoutinePlan>,
    ) -> Result<Vec<RoutineFired>> {
        let _w = self.lock(&[execution_id]);
        let Some(mut e) = self.execution(execution_id)? else {
            return Ok(vec![]);
        };
        let now = self.now_ms();
        if !Self::routine_due(&e, now) {
            return Ok(vec![]);
        }
        let started = self.started_at_ms.load(Ordering::Relaxed);
        let (due, later): (Vec<PendingWake>, Vec<PendingWake>) = std::mem::take(&mut e.wakes)
            .into_iter()
            .partition(|w| w.purpose == Purpose::Routine && w.task_due(now));
        e.wakes = later;
        let fired: Vec<FiredWake> = due
            .into_iter()
            .map(|w| self.fired_of(w, now, started))
            .collect();
        // The series goes on: each next occurrence, numbered past those passed
        // over, is back on the list in this frame.
        for f in &fired {
            if let Some(next) = f.next_due_at_ms {
                let skipped = u32::try_from(f.missed).unwrap_or(u32::MAX);
                e.wakes.push(PendingWake {
                    due_at_ms: next,
                    occurrence: f.wake.occurrence.saturating_add(1).saturating_add(skipped),
                    ..f.wake.clone()
                });
            }
        }
        sort(&mut e.wakes);
        let mut frame = Vec::new();
        let mut out = Vec::new();
        for f in fired {
            let one = self.fire_one(&mut e, f, false, &mut plan, &mut frame)?;
            out.push(one);
        }
        e.updated_at_ms = now;
        // The carve staged the routine's record as it was then: the last copy
        // of a key is the one that stands.
        frame.push(exec_record(&e)?);
        self.commit(&dedup(frame))?;
        Ok(out)
    }

    /// Open one run from `wake_id` now, without touching the series (a
    /// paused one too). The same plan decides; a refusal to run is an error
    /// saying why (a run is going, the month is spent).
    pub fn run_routine_now(
        &self,
        execution_id: &str,
        wake_id: &str,
        mut plan: impl FnMut(&Execution, &RoutineDue) -> Result<RoutinePlan>,
    ) -> Result<RoutineFired> {
        self.require_accepting()?;
        let _w = self.lock(&[execution_id]);
        let mut e = self
            .execution(execution_id)?
            .ok_or_else(|| KernelError::UnknownExecution(execution_id.into()))?;
        anyhow::ensure!(
            !e.state.is_terminal(),
            "routine execution {execution_id} has ended"
        );
        let wake = e
            .wakes
            .iter()
            .find(|w| w.id == wake_id && w.purpose == Purpose::Routine)
            .cloned()
            .ok_or_else(|| anyhow::anyhow!("no routine wake {wake_id} is pending"))?;
        let now = self.now_ms();
        let f = FiredWake {
            late_ms: 0,
            while_down: false,
            missed: 0,
            next_due_at_ms: None,
            wake,
        };
        let mut frame = Vec::new();
        let one = self.fire_one(&mut e, f, true, &mut plan, &mut frame)?;
        e.updated_at_ms = now;
        frame.push(exec_record(&e)?);
        self.commit(&dedup(frame))?;
        Ok(one)
    }

    /// One wake's step, staged into `frame`; `e` is changed in place.
    fn fire_one(
        &self,
        e: &mut Execution,
        f: FiredWake,
        manual: bool,
        plan: &mut impl FnMut(&Execution, &RoutineDue) -> Result<RoutinePlan>,
        frame: &mut Vec<NewRecord>,
    ) -> Result<RoutineFired> {
        let now = self.now_ms();
        let WakeRun::Task(task) = f.wake.run.clone() else {
            anyhow::bail!("wake {} is not a routine's", f.wake.id);
        };
        let (task_id, session_id) = crate::tasks::task_ids(&new_id("act"));
        let due = RoutineDue {
            fired: f.clone(),
            task,
            manual,
            now_ms: now,
            limit_micros: e.budget.limit_micros,
            spent_micros: e.budget.spent_micros,
            available_micros: e.budget.available(),
            available_after_reset_micros: e.budget.available_after_reset(),
            task_id: task_id.clone(),
            session_id: session_id.clone(),
        };
        // Nobody reads the reports of earlier runs in the routine's own
        // execution: they would only pile up.
        e.reports.clear();
        e.report_wakes.clear();
        let mut row = json!({"execution_id": e.id, "wake_id": f.wake.id,
            "due_at_ms": f.wake.due_at_ms, "set_at_ms": f.wake.set_at_ms,
            "late_ms": f.late_ms, "while_down": f.while_down, "purpose": "routine",
            "occurrence": f.wake.occurrence, "missed": f.missed,
            "next_due_at_ms": f.next_due_at_ms, "manual": manual});
        let outcome = match plan(e, &due)? {
            RoutinePlan::Skip { why, post, extra } => {
                row["why"] = json!(why);
                frame.push(self.ledger(LedgerKind::WakeSkipped, Some(&e.session_id), row)?);
                let post = match post {
                    Some(p) => {
                        let (a, recs) = self.outbox_stage(p)?;
                        frame.extend(recs);
                        Some(a)
                    }
                    None => None,
                };
                frame.extend(extra);
                RoutineFired {
                    wake: f.clone(),
                    task: None,
                    skipped: Some(why),
                    post,
                    reset_month: false,
                    manual,
                }
            }
            RoutinePlan::Run(run) => {
                let reset_month = run.reset_month;
                let opened = self.open_run(e, &f, (task_id, session_id), run, row, frame)?;
                RoutineFired {
                    wake: f.clone(),
                    task: Some(opened),
                    skipped: None,
                    post: None,
                    reset_month,
                    manual,
                }
            }
        };
        // A series that `until` ends with this occurrence says so.
        if f.ended() && !manual {
            frame.push(self.ledger(
                LedgerKind::WakeEnded,
                Some(&e.session_id),
                json!({"execution_id": e.id, "wake_id": f.wake.id,
                       "occurrence": f.wake.occurrence, "why": "until"}),
            )?);
        }
        Ok(outcome)
    }

    /// A run opened: the month's reset when the core says it turned, the
    /// carve and the task's records, and the `wake.fired` row.
    fn open_run(
        &self,
        e: &mut Execution,
        f: &FiredWake,
        (task_id, session_id): (ExecutionId, SessionId),
        run: RunPlan,
        mut row: serde_json::Value,
        frame: &mut Vec<NewRecord>,
    ) -> Result<Execution> {
        if run.reset_month {
            let before = e.budget.spent_micros;
            e.budget.spent_micros = 0;
            e.budget.resets += 1;
            frame.push(self.ledger(
                LedgerKind::BudgetReset,
                Some(&e.session_id),
                json!({"execution_id": e.id, "why": "month", "by": "routine",
                       "spent_before_usd": micros_to_usd(before),
                       "limit_usd": micros_to_usd(e.budget.limit_micros),
                       "reserved_usd": micros_to_usd(e.budget.reserved_micros),
                       "held_unknown_usd": micros_to_usd(e.budget.held_unknown_micros),
                       "resets": e.budget.resets}),
            )?);
        }
        let available = e.budget.available();
        let limit = run.want_micros.min(available);
        if limit == 0 {
            return Err(KernelError::NothingToCarve {
                available,
                spent: e.budget.spent_micros,
                limit: e.budget.limit_micros,
            }
            .into());
        }
        let records = run.records;
        let (opened, mut recs) = self.carve_task(
            e,
            CarveFor {
                task_id,
                session_id,
                limit,
                want_micros: run.want_micros,
                available_before: available,
                by: &f.wake.id,
                how: Opening {
                    reports_to: run.reports_to,
                    wake_parent: false,
                    start_at_ms: None,
                    helper: None,
                },
            },
            |_| Ok(records),
        )?;
        frame.append(&mut recs);
        row["task"] = json!(opened.id);
        row["task_session"] = json!(opened.session_id);
        row["limit_usd"] = json!(micros_to_usd(limit));
        frame.push(self.ledger(LedgerKind::WakeFired, Some(&e.session_id), row)?);
        frame.extend(run.extra);
        Ok(opened)
    }
}

/// A frame holds one record per key: the carve wrote the routine's own
/// execution record, and the fire's copy (which has the series put back, the
/// reset and the carve both) is the one to keep, so the last of equal keys wins
/// and the order of the rest stays.
fn dedup(records: Vec<NewRecord>) -> Vec<NewRecord> {
    let mut keep: Vec<NewRecord> = Vec::with_capacity(records.len());
    for r in records {
        if let Some(key) = r
            .key
            .as_deref()
            .filter(|_| r.kind == theseus_store::kinds::EXECUTION)
        {
            if let Some(at) = keep
                .iter()
                .position(|k| k.kind == r.kind && k.key.as_deref() == Some(key))
            {
                keep.remove(at);
            }
        }
        keep.push(r);
    }
    keep
}

fn sort(wakes: &mut [PendingWake]) {
    wakes.sort_by(|a, b| (a.due_at_ms, &a.id).cmp(&(b.due_at_ms, &b.id)));
}

/// What `reschedule_routine` changes in a routine's wake and execution.
#[derive(Debug, Clone, Default)]
pub struct Reschedule {
    /// A new series and its first time.
    pub schedule: Option<(u64, crate::repeat::Repeat)>,
    /// A new brief or per-run limit.
    pub task: Option<TaskRun>,
    /// A new month limit.
    pub limit_micros: Option<Micros>,
}

impl Kernel {
    /// Change a routine's wake (schedule, brief, per-run limit) and its
    /// execution's limit (the month) in one frame with a `wake.moved` row and
    /// the caller's records. The wake keeps its id and its pause.
    pub fn reschedule_routine(
        &self,
        execution_id: &str,
        wake_id: &str,
        change: Reschedule,
        by: &str,
        extra: Vec<NewRecord>,
    ) -> Result<PendingWake> {
        self.require_accepting()?;
        let _w = self.lock(&[execution_id]);
        let mut e = self
            .execution(execution_id)?
            .ok_or_else(|| KernelError::UnknownExecution(execution_id.into()))?;
        let i = e
            .wakes
            .iter()
            .position(|w| w.id == wake_id && w.purpose == Purpose::Routine)
            .ok_or_else(|| anyhow::anyhow!("no routine wake {wake_id} is pending"))?;
        let from = e.wakes[i].due_at_ms;
        if let Some((due, repeat)) = change.schedule {
            e.wakes[i].due_at_ms = due;
            e.wakes[i].repeat = Some(repeat);
        }
        if let Some(t) = change.task {
            e.wakes[i].run = WakeRun::Task(t);
        }
        if let Some(limit) = change.limit_micros {
            e.budget.limit_micros = limit;
            e.budget.pinned = true;
        }
        let wake = e.wakes[i].clone();
        sort(&mut e.wakes);
        e.updated_at_ms = self.now_ms();
        let row = self.ledger(
            LedgerKind::WakeMoved,
            Some(&e.session_id),
            json!({"execution_id": e.id, "wake_id": wake.id, "from": from,
                   "to": wake.due_at_ms, "by": by, "series": true, "routine": true}),
        )?;
        let mut frame = vec![exec_record(&e)?, row];
        frame.extend(extra);
        self.commit(&frame)?;
        Ok(wake)
    }
}
