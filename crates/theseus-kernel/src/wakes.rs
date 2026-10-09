//! Wakes a conversation sets for itself (DD8, theseus-cff; spec §3.3,
//! §3.15): `wake.at` asks for a turn in this session at a time, with a note
//! as its input.
//!
//! - **Where they live.** On the execution, in `wakes`, beside its one
//!   `wake`. The `wake` field says what the last turn parked on (input, a
//!   job, an approval, the budget). The pending wakes are a list of their
//!   own, because a conversation waits on its next input and on its due
//!   times at once, and one that waits on a job must still be woken: "check
//!   the build in ten minutes" while the build runs as its job.
//! - **When one fires.** Once it is due and its execution is free: waiting
//!   on input, a due time, a job, or another execution, where new input
//!   would start a turn too (`free`). The due scan queues the execution for
//!   the driver, as a confirm's answer does (`fire_due`); it runs on the
//!   driver's tick and in the heartbeat's reconciler. An execution that is
//!   busy (a turn running or queued, an approval or the budget waiting on
//!   the operator) keeps its wakes, and the turn that frees it queues it
//!   again in the frame that ends it (`end_turn_with`).
//! - **Once.** The execution's next turn takes every due wake in one frame
//!   that removes them and writes their nodes (`take_wakes`). A crash before
//!   that frame leaves them pending; a crash after it leaves them taken. A
//!   wake's id comes from the call that set it, so the call, run again after
//!   a restart, finds its wake instead of setting a second.
//! - **The cap.** At most `MAX_PENDING` per execution. A repeating series
//!   is one wake, and counts once.
//! - **A repeating wake** (37a, theseus-d4pt; `repeat.rs`). The frame that
//!   takes it puts its next occurrence back on the list, with the same id and
//!   the next occurrence's number, so a crash either side of that frame
//!   leaves one wake. Occurrences that fell due while it waited are counted
//!   (`missed`) and never run; past `until` it is not put back, and a
//!   `wake.ended` row says so. A cancel removes it, and ends the series.
//! - **A task's wakes** (37b, theseus-7kg). A task parks on its own wakes,
//!   waiting on input beside them. One left waiting with none (its last
//!   cancelled) is queued in that frame (`task_unparked`), so it goes on to
//!   end and report; its end or cancel drops what is left.

use std::sync::atomic::Ordering;
use theseus_protocol::LedgerKind;

use anyhow::Result;
use serde_json::json;
use theseus_store::NewRecord;

use crate::kernel::{exec_record, Kernel, KernelError, TurnGuard};
use crate::repeat::Repeat;
use crate::types::*;

/// The most wakes one execution may hold at once.
pub const MAX_PENDING: usize = 5;

/// A wake's id, from the id of the call that sets it: `act_X` gives `wak_X`.
pub fn wake_id(correlation_id: &str) -> String {
    let tail = correlation_id
        .split_once('_')
        .map_or(correlation_id, |(_, t)| t);
    format!("wak_{tail}")
}

/// Whether a due wake may start a turn on `e` now: it waits on something new
/// input would also end (§3.15), and no approval or budget question of the
/// operator's is open.
pub fn free(e: &Execution) -> bool {
    e.state == ExecState::Waiting
        && matches!(
            e.wake,
            Some(Wake::Input)
                | Some(Wake::DueAt { .. })
                | Some(Wake::Actions { .. })
                | Some(Wake::Execution { .. })
        )
}

/// Whether `e` is a task left waiting on input with no wake of its own (37b,
/// theseus-7kg): no one gives a task input, so nothing would ever wake it.
/// It is queued instead, in the frame that left it so (`end_turn`, when a
/// cancel took its last wake while its turn ended; `cancel_wake`), and its
/// next turn, finding nothing new, ends it, and it reports.
pub fn task_unparked(e: &Execution) -> bool {
    e.parent.is_some()
        && e.state == ExecState::Waiting
        && matches!(e.wake, Some(Wake::Input))
        && e.wakes.is_empty()
}

/// Whether `e`'s soonest wake is due at `now_ms`.
pub fn wake_due(e: &Execution, now_ms: u64) -> bool {
    e.wakes.first().is_some_and(|w| w.due_at_ms <= now_ms)
}

/// Whether the due scan queues `e` at `now_ms`: it waits on a due time that
/// has come, or it is free and one of its wakes is due, or a task's report
/// asked for a turn (W1; the task's end queues a free parent itself, so this
/// catches one that was freed without a turn, such as by a stop).
pub fn due_now(e: &Execution, now_ms: u64) -> bool {
    matches!((e.state, &e.wake), (ExecState::Waiting, Some(Wake::DueAt { at_ms })) if *at_ms <= now_ms)
        || (free(e) && (wake_due(e, now_ms) || !e.report_wakes.is_empty()))
}

/// What `set_wake` did.
#[derive(Debug, Clone)]
pub struct WakeSet {
    pub wake: PendingWake,
    /// Set by this call. False when the same call had set it before (a call
    /// run again after a crash): nothing was written.
    pub set: bool,
    /// The execution's pending wakes, this one included.
    pub pending: usize,
}

/// A wake a turn took (`take_wakes`).
#[derive(Debug, Clone)]
pub struct FiredWake {
    pub wake: PendingWake,
    /// How long after its due time the turn took it.
    pub late_ms: u64,
    /// It fell due before this process started: the daemon was not running.
    pub while_down: bool,
    /// A repeating wake's occurrences that fell due after this one and by
    /// the time the turn took it: passed over, never run (37a).
    pub missed: u64,
    /// A repeating wake's next due time, put back on the list in the frame
    /// that took this one; none for a one-shot wake, and for a series that
    /// `until` ended.
    pub next_due_at_ms: Option<u64>,
}

impl FiredWake {
    /// A repeating wake whose series ended with this occurrence.
    pub fn ended(&self) -> bool {
        self.wake.repeat.is_some() && self.next_due_at_ms.is_none()
    }
}

impl Kernel {
    /// Set a wake for the tool call `correlation_id` of the turn `guard`
    /// holds, in one frame with the row `wake.set`. The execution must hold
    /// its turn. `TooManyWakes` when it holds `MAX_PENDING` already, and
    /// nothing is written. A call that set its wake before gets it back, and
    /// writes nothing. With `repeat`, `due_at_ms` is its first occurrence.
    pub fn set_wake(
        &self,
        guard: &TurnGuard,
        correlation_id: &str,
        due_at_ms: u64,
        note: &str,
        target: Option<String>,
        repeat: Option<Repeat>,
    ) -> Result<WakeSet> {
        self.require_accepting()?;
        let id = wake_id(correlation_id);
        let _w = self.lock(&[&guard.execution_id]);
        let mut e = self
            .execution(&guard.execution_id)?
            .ok_or_else(|| KernelError::UnknownExecution(guard.execution_id.clone()))?;
        if let Some(w) = e.wakes.iter().find(|w| w.id == id) {
            return Ok(WakeSet {
                wake: w.clone(),
                set: false,
                pending: e.wakes.len(),
            });
        }
        crate::kernel::require_turn(&e)?;
        if e.wakes.len() >= MAX_PENDING {
            return Err(KernelError::TooManyWakes { max: MAX_PENDING }.into());
        }
        if let Some(r) = &repeat {
            let floor = self.config().min_repeat_ms;
            anyhow::ensure!(
                r.every.nominal_ms() >= floor,
                "a wake may repeat every {} minutes at the most often, not every {}",
                floor / 60_000,
                r.every
            );
        }
        let now = self.now_ms();
        let wake = PendingWake {
            id,
            due_at_ms,
            note: note.to_string(),
            set_at_ms: now,
            by: correlation_id.to_string(),
            target,
            occurrence: u32::from(repeat.is_some()),
            repeat,
        };
        e.wakes.push(wake.clone());
        sort(&mut e.wakes);
        e.updated_at_ms = now;
        let mut row = json!({"execution_id": e.id, "wake_id": wake.id, "due_at_ms": due_at_ms,
                       "in_ms": due_at_ms.saturating_sub(now), "note": clip(&wake.note),
                       "by": correlation_id, "pending": e.wakes.len()});
        if let Some(r) = &wake.repeat {
            row["every"] = json!(r.every.to_string());
            if !r.days.is_empty() {
                row["days"] = json!(r.days);
            }
            if let Some(u) = r.until_ms {
                row["until_ms"] = json!(u);
            }
        }
        self.commit(&[
            exec_record(&e)?,
            self.ledger(LedgerKind::WakeSet, Some(&e.session_id), row)?,
        ])?;
        Ok(WakeSet {
            wake,
            set: true,
            pending: e.wakes.len(),
        })
    }

    /// The wakes of the turn `guard` holds that are due now, soonest first.
    /// They are removed in one frame with the records `extra` builds from
    /// them (their nodes in the session) and a `wake.fired` row each, which
    /// says how late each ran. A repeating one goes back on the list in the
    /// same frame, at its next occurrence after now, and its row says which
    /// occurrence ran, how many it passed over, and when the next is due; one
    /// that `until` ends gets a `wake.ended` row instead. None due, and
    /// nothing is written: a plain turn pays one read.
    pub fn take_wakes(
        &self,
        guard: &TurnGuard,
        extra: impl FnOnce(&[FiredWake]) -> Result<Vec<NewRecord>>,
    ) -> Result<Vec<FiredWake>> {
        let _w = self.lock(&[&guard.execution_id]);
        let mut e = self
            .execution(&guard.execution_id)?
            .ok_or_else(|| KernelError::UnknownExecution(guard.execution_id.clone()))?;
        let now = self.now_ms();
        if !wake_due(&e, now) {
            return Ok(vec![]);
        }
        let started = self.started_at_ms.load(Ordering::Relaxed);
        let (due, later): (Vec<PendingWake>, Vec<PendingWake>) = std::mem::take(&mut e.wakes)
            .into_iter()
            .partition(|w| w.due_at_ms <= now);
        e.wakes = later;
        e.updated_at_ms = now;
        let zone = &self.config().zone;
        let fired: Vec<FiredWake> = due
            .into_iter()
            .map(|w| {
                let (missed, next) = match &w.repeat {
                    Some(r) => (r.between(zone, w.due_at_ms, now), r.next_after(zone, now)),
                    None => (0, None),
                };
                FiredWake {
                    late_ms: now.saturating_sub(w.due_at_ms),
                    while_down: started > 0 && w.due_at_ms < started,
                    missed,
                    next_due_at_ms: next,
                    wake: w,
                }
            })
            .collect();
        // Each series' next occurrence, numbered past those passed over.
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
        let mut frame = vec![exec_record(&e)?];
        frame.extend(extra(&fired)?);
        for f in &fired {
            let mut row = json!({"execution_id": e.id, "wake_id": f.wake.id, "due_at_ms": f.wake.due_at_ms,
                       "set_at_ms": f.wake.set_at_ms, "late_ms": f.late_ms,
                       "while_down": f.while_down, "turn": guard.turn});
            if let Some(r) = &f.wake.repeat {
                row["every"] = json!(r.every.to_string());
                row["occurrence"] = json!(f.wake.occurrence);
                row["missed"] = json!(f.missed);
                row["next_due_at_ms"] = json!(f.next_due_at_ms);
            }
            frame.push(self.ledger(LedgerKind::WakeFired, Some(&e.session_id), row)?);
            if f.ended() {
                frame.push(self.ledger(
                    LedgerKind::WakeEnded,
                    Some(&e.session_id),
                    json!({"execution_id": e.id, "wake_id": f.wake.id,
                           "occurrence": f.wake.occurrence,
                           "until_ms": f.wake.repeat.as_ref().and_then(|r| r.until_ms),
                           "why": "until"}),
                )?);
            }
        }
        self.commit(&frame)?;
        Ok(fired)
    }

    /// Cancel the wake `wake_id` of `execution_id`, for `by`, in one frame
    /// with the row `wake.cancelled`. `None` when it is not pending (it ran,
    /// or was cancelled before): nothing is written.
    pub fn cancel_wake(
        &self,
        execution_id: &str,
        wake_id: &str,
        by: &str,
    ) -> Result<Option<(Execution, PendingWake)>> {
        let _w = self.lock(&[execution_id]);
        let mut e = self
            .execution(execution_id)?
            .ok_or_else(|| KernelError::UnknownExecution(execution_id.into()))?;
        let Some(i) = e.wakes.iter().position(|w| w.id == wake_id) else {
            return Ok(None);
        };
        let wake = e.wakes.remove(i);
        e.updated_at_ms = self.now_ms();
        let mut rows = vec![self.ledger(
            LedgerKind::WakeCancelled,
            Some(&e.session_id),
            json!({"execution_id": e.id, "wake_id": wake.id, "due_at_ms": wake.due_at_ms, "by": by}),
        )?];
        // A task parked on its last wake goes on, to end and report (37b).
        if task_unparked(&e) {
            e.state = ExecState::Queued;
            e.wake = None;
            e.resume_pending = true;
            rows.push(self.ledger(
                LedgerKind::ExecutionQueued,
                Some(&e.session_id),
                json!({"execution_id": e.id, "why": "wake_cancelled", "wakes": [wake.id]}),
            )?);
        }
        let mut frame = vec![exec_record(&e)?];
        frame.extend(rows);
        self.commit(&frame)?;
        Ok(Some((e, wake)))
    }

    /// Queue `execution_id` for the driver if the due scan says so
    /// (`due_now`), deciding again under its lock: a due time it waited on,
    /// or a wake of its own that is due while it is free. Returns the queued
    /// execution, or `None` when it was not due by then.
    pub fn fire_due(&self, execution_id: &str) -> Result<Option<Execution>> {
        let _w = self.lock(&[execution_id]);
        let Some(mut e) = self.execution(execution_id)? else {
            return Ok(None);
        };
        let now = self.now_ms();
        if !due_now(&e, now) {
            return Ok(None);
        }
        let wakes: Vec<&str> = e
            .wakes
            .iter()
            .filter(|w| w.due_at_ms <= now)
            .map(|w| w.id.as_str())
            .collect();
        let why = if !wakes.is_empty() {
            "wake"
        } else if free(&e) && !e.report_wakes.is_empty() {
            "report"
        } else {
            "due"
        };
        let mut body = json!({"execution_id": e.id, "why": why, "wakes": wakes});
        if why == "report" {
            body["tasks"] = json!(e.report_wakes);
        }
        let row = self.ledger(LedgerKind::ExecutionQueued, Some(&e.session_id), body)?;
        e.state = ExecState::Queued;
        e.wake = None;
        e.resume_pending = true;
        e.updated_at_ms = now;
        self.commit(&[exec_record(&e)?, row])?;
        Ok(Some(e))
    }

    /// Every pending wake of every open execution, soonest first, each with
    /// its execution: those that hold wakes, by their term (theseus-lv2).
    pub fn pending_wakes(&self) -> Result<Vec<(Execution, PendingWake)>> {
        let mut out: Vec<(Execution, PendingWake)> = Vec::new();
        for e in self
            .executions_by(&[crate::terms::one("w")])?
            .into_iter()
            .filter(|e| !e.state.is_terminal())
        {
            for w in &e.wakes {
                out.push((e.clone(), w.clone()));
            }
        }
        out.sort_by(|a, b| (a.1.due_at_ms, &a.1.id).cmp(&(b.1.due_at_ms, &b.1.id)));
        Ok(out)
    }

    /// Why a turn that would park `e` queues it at once instead, if it
    /// does: a wake of its own came due while it ran, or a task's report
    /// asked for a turn (W1), and it would wait where a wake may fire (DD8);
    /// a task has no wake left to park on (37b); or the question it would
    /// wait on was answered while its turn still ran (theseus-q5af): the
    /// answer bound or declined it, and its wake found the turn running, so
    /// nothing would wake it again. The answer's frame and this one each hold
    /// the execution's lock, so an answer lands before the end, and is read
    /// here, or after it, and wakes it.
    pub(crate) fn queued_at_end(&self, e: &Execution, now: u64) -> Result<Option<&'static str>> {
        if let Some(Wake::Confirm { confirm_id }) = &e.wake {
            if self
                .action(confirm_id)?
                .is_some_and(|a| !a.awaits_confirm())
            {
                return Ok(Some("answered"));
            }
        }
        if !free(e) {
            return Ok(None);
        }
        Ok(if wake_due(e, now) {
            Some("wake")
        } else if !e.report_wakes.is_empty() {
            Some("report")
        } else if task_unparked(e) {
            Some("task_unparked")
        } else {
            None
        })
    }

    /// An execution that ends drops its wakes (a cancel, a task's end): each
    /// gets a `wake.cancelled` row in the frame being built, with `why`.
    /// Nothing for an execution with none. A `/stop` ends nothing, so it
    /// keeps them, as it keeps tasks (`stop_execution`).
    pub(crate) fn drop_wakes(
        &self,
        e: &mut Execution,
        by: &str,
        why: &str,
        frame: &mut Vec<NewRecord>,
    ) -> Result<()> {
        for w in std::mem::take(&mut e.wakes) {
            frame.push(self.ledger(
                LedgerKind::WakeCancelled,
                Some(&e.session_id),
                json!({"execution_id": e.id, "wake_id": w.id, "due_at_ms": w.due_at_ms, "by": by, "why": why}),
            )?);
        }
        Ok(())
    }
}

/// Soonest first, and by id at the same time.
fn sort(wakes: &mut [PendingWake]) {
    wakes.sort_by(|a, b| (a.due_at_ms, &a.id).cmp(&(b.due_at_ms, &b.id)));
}

/// A note as a ledger row carries it: its first 200 characters.
fn clip(note: &str) -> String {
    note.chars().take(200).collect()
}
