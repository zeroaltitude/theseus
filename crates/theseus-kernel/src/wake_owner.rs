//! The owner's verbs on wakes, and a notice's firing (theseus-8ux6x): what
//! needs no model turn and no turn guard.
//!
//! - **`create_wake`, `snooze_wake`, `pause_wake`, `resume_wake`**: each one
//!   frame under the execution's lock, with its row (`wake.set`,
//!   `wake.snoozed`, `wake.paused`, `wake.resumed`), whether or not a turn of
//!   that execution runs. `set_wake` needs the turn's guard, because the
//!   model sets its own through its turn; the owner does not.
//! - **`fire_notices`**: a `Notice` wake is taken the moment it is due, even
//!   while its conversation's turn runs: in one frame the wake leaves (a series
//!   puts its next occurrence back), a `wake.fired { notice: true }` row, the
//!   record a snooze reads, and the post of its note. No turn, no model call, no
//!   admission slot. The due scan finds the executions that hold one by the
//!   `nw` term (`notice_executions`), whatever their state.
//! - **A snooze of what already fired** (a one-shot reminder has left its
//!   execution) reads the fired wake's record (`FIRED_PREFIX`) and sets it
//!   again, for the new time, in the one frame.

use anyhow::Result;
use serde_json::json;
use std::sync::atomic::Ordering;
use theseus_protocol::term::words::cut;
use theseus_protocol::LedgerKind;
use theseus_store::{kinds, NewRecord};

use crate::kernel::{exec_record, Kernel, KernelError};
use crate::outbox::Post;
use crate::types::*;
use crate::wake_kinds::{NewWake, Purpose, WakeRun};
use crate::wakes::{wake_id, FiredWake, WakeMoved, WakeSet};

/// The META key a fired notice's wake is kept under (`<prefix><wake id>`), so
/// a snooze can set it again after it has left the execution.
pub const FIRED_PREFIX: &str = "wake.fired.";

/// A notice fired: the wake, and the post that carries its note.
#[derive(Debug, Clone)]
pub struct NoticeFired {
    pub fired: FiredWake,
    pub post: Option<Action>,
}

/// What the core adds to a notice's frame: where its note is posted, and any
/// records of its own (the note in a conversation that has no place).
#[derive(Debug, Default)]
pub struct NoticePost {
    pub post: Option<Post>,
    pub extra: Vec<NewRecord>,
}

impl Kernel {
    /// Open executions that hold a notice wake that is not paused, in any
    /// state: the due scan reads these, never every execution.
    pub fn notice_executions(&self) -> Result<Vec<Execution>> {
        self.executions_by(&[crate::terms::one(crate::terms::NOTICE)])
    }

    /// Take `execution_id`'s notices that are due now, in one frame. `make`
    /// builds each one's post (and any records beside it) from the execution
    /// and the fired wake. None due, and nothing is written. Returns the posts
    /// staged, for the caller to announce (`Outbox::posted`) once written.
    pub fn fire_notices(
        &self,
        execution_id: &str,
        mut make: impl FnMut(&Execution, &FiredWake) -> Result<NoticePost>,
    ) -> Result<Vec<NoticeFired>> {
        let _w = self.lock(&[execution_id]);
        let Some(mut e) = self.execution(execution_id)? else {
            return Ok(vec![]);
        };
        let now = self.now_ms();
        if e.state.is_terminal() || !e.wakes.iter().any(|w| w.notice_due(now)) {
            return Ok(vec![]);
        }
        let started = self.started_at_ms.load(Ordering::Relaxed);
        let (due, later): (Vec<PendingWake>, Vec<PendingWake>) = std::mem::take(&mut e.wakes)
            .into_iter()
            .partition(|w| w.notice_due(now));
        e.wakes = later;
        e.updated_at_ms = now;
        let fired: Vec<FiredWake> = due
            .into_iter()
            .map(|w| self.fired_of(w, now, started))
            .collect();
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
        let mut out = Vec::new();
        for f in fired {
            let np = make(&e, &f)?;
            let mut row = json!({"execution_id": e.id, "wake_id": f.wake.id,
                       "due_at_ms": f.wake.due_at_ms, "set_at_ms": f.wake.set_at_ms,
                       "late_ms": f.late_ms, "while_down": f.while_down, "notice": true,
                       "purpose": f.wake.purpose.as_str(), "note": cut(&f.wake.note, 200)});
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
            frame.push(NewRecord::json(
                kinds::META,
                Some(&format!("{FIRED_PREFIX}{}", f.wake.id)),
                &(&e.id, &f.wake),
            )?);
            let post = match np.post {
                Some(p) => {
                    let (a, recs) = self.outbox_stage(p)?;
                    frame.extend(recs);
                    Some(a)
                }
                None => None,
            };
            frame.extend(np.extra);
            out.push(NoticeFired { fired: f, post });
        }
        self.commit(&frame)?;
        Ok(out)
    }

    /// Set a wake for `execution_id` on the owner's word, in one frame with
    /// the row `wake.set` (`by` says who). No turn is needed, and none is
    /// taken. `id` makes it idempotent: a wake with that id is returned, not
    /// set again. The furthest it may be is `WakeLimits::max_ahead_ms`.
    pub fn create_wake(
        &self,
        execution_id: &str,
        id: Option<String>,
        new: NewWake,
        by: &str,
    ) -> Result<WakeSet> {
        self.create_wake_with(execution_id, id, new, by, vec![])
    }

    /// `create_wake` with the caller's own records in the one frame (a
    /// routine's record: `routines.rs`).
    pub fn create_wake_with(
        &self,
        execution_id: &str,
        id: Option<String>,
        new: NewWake,
        by: &str,
        extra: Vec<NewRecord>,
    ) -> Result<WakeSet> {
        self.require_accepting()?;
        let _w = self.lock(&[execution_id]);
        let mut e = self
            .execution(execution_id)?
            .ok_or_else(|| KernelError::UnknownExecution(execution_id.into()))?;
        let id = id.unwrap_or_else(|| wake_id(&new_id("act")));
        if let Some(w) = e.wakes.iter().find(|w| w.id == id) {
            return Ok(WakeSet {
                wake: w.clone(),
                set: false,
                pending: e.wakes.len(),
                list: e.wakes.clone(),
            });
        }
        anyhow::ensure!(
            !e.state.is_terminal(),
            "execution {execution_id} has ended: a wake cannot be set on it"
        );
        let now = self.now_ms();
        let max = self.config().wakes.max_ahead_ms;
        anyhow::ensure!(
            new.due_at_ms.saturating_sub(now) <= max,
            "a wake may be at most {} days from now",
            max / 86_400_000
        );
        self.insert_wake_with(&mut e, id, new, by, extra)
    }

    /// Move a wake's next time to `due_at_ms`, in one frame with the row
    /// `wake.snoozed`. A pending wake moves (a paused one is resumed by it);
    /// with `fired`, or when it is no longer pending, the wake that fired is set
    /// again from its record, a one-shot for the new time (a series' fired
    /// occurrence gets an id of its own, since the series goes on). `None` when
    /// there is neither a pending wake nor a fired one of that id.
    pub fn snooze_wake(
        &self,
        execution_id: &str,
        wake_id: &str,
        due_at_ms: u64,
        fired: bool,
        by: &str,
    ) -> Result<Option<WakeMoved>> {
        self.require_accepting()?;
        let _w = self.lock(&[execution_id]);
        let mut e = self
            .execution(execution_id)?
            .ok_or_else(|| KernelError::UnknownExecution(execution_id.into()))?;
        let now = self.now_ms();
        let max = self.config().wakes.max_ahead_ms;
        anyhow::ensure!(
            due_at_ms.saturating_sub(now) <= max,
            "a wake may be at most {} days from now",
            max / 86_400_000
        );
        let pending = e.wakes.iter().position(|w| w.id == wake_id);
        let record = if pending.is_none() || fired {
            match self
                .store()
                .latest_by_key(kinds::META, &format!("{FIRED_PREFIX}{wake_id}"))?
            {
                Some(r) => Some(r.decode::<(String, PendingWake)>()?),
                None => None,
            }
        } else {
            None
        };
        let (from, wake) = match (pending, record) {
            (Some(i), None) => {
                anyhow::ensure!(
                    e.wakes[i].probe().is_none(),
                    "a watch is not snoozed: it checks on its own interval. Cancel it and set another"
                );
                let from = e.wakes[i].clone();
                e.wakes[i].due_at_ms = due_at_ms;
                e.wakes[i].paused_at_ms = None;
                (from, e.wakes[i].clone())
            }
            (_, Some((_, was))) => {
                anyhow::ensure!(
                    !e.state.is_terminal(),
                    "execution {execution_id} has ended: a wake cannot be set on it"
                );
                let id = if pending.is_some() {
                    wake_id_of(&new_id("act"))
                } else {
                    was.id.clone()
                };
                if let Some(cap) = self.config().wakes.daemon_cap(was.purpose) {
                    let held = self.count_purpose(was.purpose)?;
                    anyhow::ensure!(
                        held < cap,
                        "the daemon already holds {held} pending {}s, the most it may ({cap})",
                        was.purpose.noun()
                    );
                }
                let again = PendingWake {
                    id,
                    due_at_ms,
                    set_at_ms: now,
                    by: by.to_string(),
                    repeat: None,
                    occurrence: 0,
                    paused_at_ms: None,
                    ..was.clone()
                };
                e.wakes.push(again.clone());
                (was, again)
            }
            (None, None) => return Ok(None),
        };
        sort(&mut e.wakes);
        e.updated_at_ms = now;
        let row = self.ledger(
            LedgerKind::WakeSnoozed,
            Some(&e.session_id),
            json!({"execution_id": e.id, "wake_id": wake.id, "from_wake": from.id,
                   "from": from.due_at_ms, "to": due_at_ms, "by": by,
                   "fired": fired || wake.id != wake_id || pending.is_none()}),
        )?;
        self.commit(&[exec_record(&e)?, row])?;
        Ok(Some(WakeMoved {
            from,
            wake,
            list: e.wakes,
        }))
    }

    /// Pause the wake `wake_id`, in one frame with the row `wake.paused`: the
    /// due scan passes it over. A series keeps its place, and occurrences that
    /// pass meanwhile are counted, as missed ones are. With `until_ms` the
    /// wake is not held but moved: a series' next occurrence at or after it
    /// (the ones before it passed over), a one-shot's time to it if later.
    /// `None` when it is not pending; a wake already paused is returned and
    /// nothing is written.
    pub fn pause_wake(
        &self,
        execution_id: &str,
        wake_id: &str,
        until_ms: Option<u64>,
        by: &str,
    ) -> Result<Option<WakeMoved>> {
        self.require_accepting()?;
        let _w = self.lock(&[execution_id]);
        let mut e = self
            .execution(execution_id)?
            .ok_or_else(|| KernelError::UnknownExecution(execution_id.into()))?;
        let Some(i) = e.wakes.iter().position(|w| w.id == wake_id) else {
            return Ok(None);
        };
        let from = e.wakes[i].clone();
        anyhow::ensure!(
            from.probe().is_none(),
            "a watch is not paused: it checks on its own interval. Cancel it and set another"
        );
        let now = self.now_ms();
        match until_ms {
            None if from.paused_at_ms.is_some() => {
                return Ok(Some(WakeMoved {
                    wake: from.clone(),
                    from,
                    list: e.wakes,
                }))
            }
            None => e.wakes[i].paused_at_ms = Some(now),
            Some(u) => {
                let zone = &self.config().zone;
                let w = &mut e.wakes[i];
                match w.repeat.clone() {
                    Some(r) if u > w.due_at_ms => {
                        let at = u.saturating_sub(1);
                        let Some(next) = r.next_after(zone, at) else {
                            anyhow::bail!(
                                "the series ends before then: pausing it until then would end it; \
                                 cancel it instead"
                            );
                        };
                        let skipped =
                            u32::try_from(r.between(zone, w.due_at_ms, at)).unwrap_or(u32::MAX);
                        w.occurrence = w.occurrence.saturating_add(1).saturating_add(skipped);
                        w.due_at_ms = next;
                    }
                    Some(_) => {}
                    None => w.due_at_ms = w.due_at_ms.max(u),
                }
            }
        }
        let wake = e.wakes[i].clone();
        sort(&mut e.wakes);
        e.updated_at_ms = now;
        let row = self.ledger(
            LedgerKind::WakePaused,
            Some(&e.session_id),
            json!({"execution_id": e.id, "wake_id": wake.id, "by": by, "from": from.due_at_ms,
                   "to": wake.due_at_ms, "until_ms": until_ms,
                   "held": wake.paused_at_ms.is_some()}),
        )?;
        self.commit(&[exec_record(&e)?, row])?;
        Ok(Some(WakeMoved {
            from,
            wake,
            list: e.wakes,
        }))
    }

    /// Resume a paused wake, in one frame with the row `wake.resumed`. A
    /// series goes on at its next occurrence after now, numbered by its place
    /// in the series (the occurrences that fell while it was held are counted
    /// as passed over); a one-shot whose time has passed is due at once, and
    /// says how late it ran. `None` when it is not pending; one that is not
    /// paused is returned and nothing is written.
    pub fn resume_wake(
        &self,
        execution_id: &str,
        wake_id: &str,
        by: &str,
    ) -> Result<Option<WakeMoved>> {
        self.require_accepting()?;
        let _w = self.lock(&[execution_id]);
        let mut e = self
            .execution(execution_id)?
            .ok_or_else(|| KernelError::UnknownExecution(execution_id.into()))?;
        let Some(i) = e.wakes.iter().position(|w| w.id == wake_id) else {
            return Ok(None);
        };
        let from = e.wakes[i].clone();
        if from.paused_at_ms.is_none() {
            return Ok(Some(WakeMoved {
                wake: from.clone(),
                from,
                list: e.wakes,
            }));
        }
        let now = self.now_ms();
        let zone = &self.config().zone;
        let w = &mut e.wakes[i];
        w.paused_at_ms = None;
        if let (Some(r), true) = (w.repeat.clone(), w.due_at_ms <= now) {
            let passed = u32::try_from(r.between(zone, w.due_at_ms, now)).unwrap_or(u32::MAX);
            match r.next_after(zone, now) {
                Some(next) => {
                    w.occurrence = w.occurrence.saturating_add(1).saturating_add(passed);
                    w.due_at_ms = next;
                }
                // The series ended while it was held: nothing is left to run.
                None => {
                    let gone = e.wakes.remove(i);
                    e.updated_at_ms = now;
                    let rows = [
                        self.ledger(
                            LedgerKind::WakeResumed,
                            Some(&e.session_id),
                            json!({"execution_id": e.id, "wake_id": gone.id, "by": by,
                                   "ended": true}),
                        )?,
                        self.ledger(
                            LedgerKind::WakeEnded,
                            Some(&e.session_id),
                            json!({"execution_id": e.id, "wake_id": gone.id,
                                   "occurrence": gone.occurrence, "why": "until"}),
                        )?,
                    ];
                    let mut frame = vec![exec_record(&e)?];
                    frame.extend(rows);
                    self.commit(&frame)?;
                    return Ok(Some(WakeMoved {
                        wake: gone,
                        from,
                        list: e.wakes,
                    }));
                }
            }
        }
        let wake = e.wakes[i].clone();
        sort(&mut e.wakes);
        e.updated_at_ms = now;
        let row = self.ledger(
            LedgerKind::WakeResumed,
            Some(&e.session_id),
            json!({"execution_id": e.id, "wake_id": wake.id, "by": by, "from": from.due_at_ms,
                   "to": wake.due_at_ms, "occurrence": wake.occurrence}),
        )?;
        self.commit(&[exec_record(&e)?, row])?;
        Ok(Some(WakeMoved {
            from,
            wake,
            list: e.wakes,
        }))
    }

    /// Drop a series' next occurrence (theseus-0oldh), in one frame with the
    /// row `wake.skipped { why: "owner" }`: the series goes on at the
    /// occurrence after it, numbered by its place, and nothing else of it
    /// changes. The occurrence dropped is the next one ahead: one that fell
    /// due while nothing scanned is passed over, counted, as a pause's `until`
    /// counts them. A skip opens no run, so a count of runs does not move.
    /// `None` when it is not pending; refused (nothing written) for a held
    /// series, a watch, a wake with no series, and a series whose next
    /// occurrence is its last: that one is a cancel.
    pub fn skip_wake(
        &self,
        execution_id: &str,
        wake_id: &str,
        by: &str,
    ) -> Result<Option<WakeMoved>> {
        self.require_accepting()?;
        let _w = self.lock(&[execution_id]);
        let mut e = self
            .execution(execution_id)?
            .ok_or_else(|| KernelError::UnknownExecution(execution_id.into()))?;
        let Some(i) = e.wakes.iter().position(|w| w.id == wake_id) else {
            return Ok(None);
        };
        let from = e.wakes[i].clone();
        anyhow::ensure!(
            from.probe().is_none(),
            "a watch has no occurrence to skip: it checks on its own interval. Cancel it instead"
        );
        anyhow::ensure!(
            from.paused_at_ms.is_none(),
            "the series is paused: nothing runs until it is resumed, so there is no next \
             occurrence to skip. Resume it first"
        );
        let Some(r) = from.repeat.clone() else {
            anyhow::bail!(
                "a wake that does not repeat has no next occurrence to skip: cancel it instead"
            );
        };
        let now = self.now_ms();
        let zone = &self.config().zone;
        let at = from.due_at_ms.max(now);
        let Some(next) = r.next_after(zone, at) else {
            anyhow::bail!(
                "the next occurrence is the last of the series: skipping it would end the series; \
                 cancel it instead"
            );
        };
        let passed = u32::try_from(r.between(zone, from.due_at_ms, at)).unwrap_or(u32::MAX);
        let w = &mut e.wakes[i];
        w.occurrence = w.occurrence.saturating_add(1).saturating_add(passed);
        w.due_at_ms = next;
        let wake = w.clone();
        sort(&mut e.wakes);
        e.updated_at_ms = now;
        let row = self.ledger(
            LedgerKind::WakeSkipped,
            Some(&e.session_id),
            json!({"execution_id": e.id, "wake_id": wake.id, "due_at_ms": from.due_at_ms,
                   "occurrence": from.occurrence, "next_due_at_ms": wake.due_at_ms,
                   "why": "owner", "by": by, "purpose": wake.purpose.noun()}),
        )?;
        self.commit(&[exec_record(&e)?, row])?;
        Ok(Some(WakeMoved {
            from,
            wake,
            list: e.wakes,
        }))
    }
}

fn wake_id_of(correlation_id: &str) -> String {
    wake_id(correlation_id)
}

fn sort(wakes: &mut [PendingWake]) {
    wakes.sort_by(|a, b| (a.due_at_ms, &a.id).cmp(&(b.due_at_ms, &b.id)));
}

/// A reminder's purpose and a notice's run: what the owner's `wake.create`
/// sets and the model's `wake.at { for: "owner", say_only }` does.
pub fn reminder(due_at_ms: u64, note: &str) -> NewWake {
    NewWake {
        due_at_ms,
        note: note.to_string(),
        purpose: Purpose::Remind,
        run: WakeRun::Notice,
        ..Default::default()
    }
}
