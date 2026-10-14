//! The owner's verbs on wakes, with no model turn (theseus-8ux6x):
//! `wake.create`, `wake.snooze`, `wake.pause`, `wake.resume` and `wake.done`.
//! Each is one kernel frame under the execution's lock (`kernel/wake_owner.rs`),
//! whatever the conversation is doing, with its row. A reminder made with no
//! session lives in the owner's Home (`wake_home.rs`).
//!
//! They are anyone's, as `wake.cancel` is: a reminder asks nothing of a model
//! or a tool, and the daemon-wide cap (`[kernel] max_reminders`) bounds a flood.
//! The four answers of a fired reminder call these directly (`wake.done`,
//! `wake.snooze`); a later `work.answer` will route them, one answer for every
//! kind of question and notice.

use serde_json::{json, Value};
use theseus_kernel::{NewWake, Purpose, WakeRun};
use theseus_protocol::{
    error_code, method, WakeCreateParams, WakeDoneResult, WakeEditResult, WakeNameParams,
    WakePauseParams, WakeSnoozeParams,
};

use super::server::{route, Conn, RpcFailure};
use super::Core;
use crate::session::SessionRecord;
use crate::wake::{Input, MAX_NOTE_CHARS};
use crate::wake_when::parse_when;

/// The owner's verbs, routed by `rpc_prefixed` (the list and the cancel keep
/// their own arms in `dispatch`).
pub(super) const OWN: [&str; 5] = [
    method::WAKE_CREATE,
    method::WAKE_SNOOZE,
    method::WAKE_PAUSE,
    method::WAKE_RESUME,
    method::WAKE_DONE,
];

/// What a snooze with no time means.
const DEFAULT_SNOOZE: &str = "10m";

impl Core {
    /// The owner's verbs, by name (`rpc_prefixed` sends them here).
    pub(super) fn rpc_wakes(
        &self,
        name: &str,
        params: Value,
        conn: Conn<'_>,
    ) -> Result<Value, RpcFailure> {
        match name {
            method::WAKE_CREATE => route(params, |p| self.wake_create(p, conn)),
            method::WAKE_SNOOZE => route(params, |p| self.wake_snooze(p, conn)),
            method::WAKE_PAUSE => route(params, |p| self.wake_pause(p, conn)),
            method::WAKE_RESUME => route(params, |p| self.wake_resume(p, conn)),
            _ => route(params, |p| self.wake_done(p, conn)),
        }
    }

    /// `wake.create`: a reminder for the owner, for its time, with its note as
    /// the whole message. In `session_id`'s conversation, or Home.
    pub(super) fn wake_create(
        &self,
        p: WakeCreateParams,
        conn: Conn<'_>,
    ) -> Result<WakeEditResult, RpcFailure> {
        let by = conn.actor(p.author.as_deref());
        let note = p.note.trim().to_string();
        if note.is_empty() {
            return Err(invalid("the note is empty: say what the reminder says"));
        }
        let n = note.chars().count();
        if n > MAX_NOTE_CHARS {
            return Err(invalid(format!(
                "the note is {n} characters, over the {MAX_NOTE_CHARS} a wake takes"
            )));
        }
        let now = self.kernel.now_ms();
        let kc = self.kernel.config();
        let due = parse_when(&p.when, now, &kc.zone, kc.wakes.max_ahead_ms).map_err(invalid)?;
        let repeat = match &p.every {
            None => {
                if !p.days.is_empty() || p.until.is_some() {
                    return Err(invalid(
                        "`days` and `until` belong to a repeating reminder: give `every` too",
                    ));
                }
                None
            }
            Some(every) => {
                let until_ms = match &p.until {
                    Some(u) => {
                        Some(parse_when(u, now, &kc.zone, kc.wakes.max_ahead_ms).map_err(invalid)?)
                    }
                    None => None,
                };
                let i = Input {
                    every: Some(every.clone()),
                    days: (!p.days.is_empty()).then(|| p.days.clone()),
                    first_ms: Some(due),
                    until_ms,
                    ..Default::default()
                };
                let (first, repeat) = crate::wake::plan_of(
                    &i,
                    now,
                    &kc.zone,
                    (kc.min_repeat_ms, kc.wakes.max_ahead_ms),
                )
                .map_err(invalid)?;
                Some((first, repeat))
            }
        };
        let (session_id, execution_id, home) = match &p.session_id {
            Some(s) => {
                let rec = self.session(s)?;
                let Some(ex) = rec.execution_id.clone() else {
                    return Err(invalid(format!(
                        "session {s} has no conversation to hold it"
                    )));
                };
                (rec.session_id, ex, false)
            }
            None => {
                let h = self.home().map_err(RpcFailure::from)?;
                (h.session_id, h.execution_id, true)
            }
        };
        let (due_at_ms, repeat) = match repeat {
            Some((first, r)) => (first, r),
            None => (due, None),
        };
        let new = NewWake {
            due_at_ms,
            note,
            target: self.outbox.target(&session_id),
            repeat,
            purpose: Purpose::Remind,
            run: WakeRun::Notice,
        };
        let set = self
            .kernel
            .create_wake(&execution_id, None, new, &by)
            .map_err(invalid_of)?;
        self.edited(&execution_id, &set.wake, home)
    }

    /// `wake.snooze`: move a pending wake, or set a fired reminder again, for
    /// `when` (10 minutes by default).
    pub(super) fn wake_snooze(
        &self,
        p: WakeSnoozeParams,
        conn: Conn<'_>,
    ) -> Result<WakeEditResult, RpcFailure> {
        let by = conn.actor(p.author.as_deref());
        let now = self.kernel.now_ms();
        let kc = self.kernel.config();
        let when = p.when.as_deref().unwrap_or(DEFAULT_SNOOZE);
        let due = parse_when(when, now, &kc.zone, kc.wakes.max_ahead_ms).map_err(invalid)?;
        let pending = self.kernel.pending_wakes()?;
        let found = match crate::wake::resolve(&pending, &p.wake) {
            Ok((e, w)) if !p.fired => Some((e.id.clone(), w.id.clone())),
            _ => self.fired_wake(&p.wake)?.map(|(e, w)| (e, w.id)),
        };
        let Some((execution_id, id)) = found else {
            return Err(not_found(format!(
                "no pending or fired wake is named `{}`: a fired reminder is named by its whole \
                 id (`{}` and its notice)",
                p.wake,
                crate::task::short(&p.wake)
            )));
        };
        let moved = self
            .kernel
            .snooze_wake(&execution_id, &id, due, p.fired, &by)
            .map_err(invalid_of)?
            .ok_or_else(|| not_found(format!("wake {} is neither pending nor fired", p.wake)))?;
        self.edited(&execution_id, &moved.wake, false)
    }

    /// `wake.pause`: hold a wake, or move it past `until`.
    pub(super) fn wake_pause(
        &self,
        p: WakePauseParams,
        conn: Conn<'_>,
    ) -> Result<WakeEditResult, RpcFailure> {
        let by = conn.actor(p.author.as_deref());
        let now = self.kernel.now_ms();
        let kc = self.kernel.config();
        let until = match &p.until {
            Some(u) => Some(parse_when(u, now, &kc.zone, kc.wakes.max_ahead_ms).map_err(invalid)?),
            None => None,
        };
        let (execution_id, id) = self.pending_named(&p.wake)?;
        let moved = self
            .kernel
            .pause_wake(&execution_id, &id, until, &by)
            .map_err(invalid_of)?
            .ok_or_else(|| not_found(format!("wake {} is no longer pending", p.wake)))?;
        self.edited(&execution_id, &moved.wake, false)
    }

    /// `wake.resume`: a held wake goes on.
    pub(super) fn wake_resume(
        &self,
        p: WakeNameParams,
        conn: Conn<'_>,
    ) -> Result<WakeEditResult, RpcFailure> {
        let by = conn.actor(p.author.as_deref());
        let (execution_id, id) = self.pending_named(&p.wake)?;
        let moved = self
            .kernel
            .resume_wake(&execution_id, &id, &by)
            .map_err(invalid_of)?
            .ok_or_else(|| not_found(format!("wake {} is no longer pending", p.wake)))?;
        self.edited(&execution_id, &moved.wake, false)
    }

    /// `wake.done`: the reminder was seen and is settled: its row, and no
    /// later snooze needed. A series' next occurrence is untouched.
    pub(super) fn wake_done(
        &self,
        p: WakeNameParams,
        conn: Conn<'_>,
    ) -> Result<WakeDoneResult, RpcFailure> {
        let by = conn.actor(p.author.as_deref());
        let pending = self.kernel.pending_wakes()?;
        let (execution_id, wake) = match crate::wake::resolve(&pending, &p.wake) {
            Ok((e, w)) => (e.id.clone(), w.clone()),
            Err(_) => self
                .fired_wake(&p.wake)?
                .ok_or_else(|| not_found(format!("no wake is named `{}`", p.wake)))?,
        };
        let session = self.kernel.execution(&execution_id)?.map(|e| e.session_id);
        let row = crate::ledger::LedgerRow::new(
            theseus_protocol::LedgerKind::WakeDone,
            session.as_deref(),
            None,
            json!({"execution_id": execution_id, "wake_id": wake.id, "by": by,
                   "note": wake.note.chars().take(200).collect::<String>()}),
        );
        self.store.append(&[theseus_store::NewRecord::json(
            theseus_store::kinds::LEDGER,
            None,
            &row,
        )?])?;
        Ok(WakeDoneResult {
            short: crate::task::short(&wake.id),
            wake_id: wake.id,
        })
    }

    /// The pending wake `name` names: its execution and id.
    fn pending_named(&self, name: &str) -> Result<(String, String), RpcFailure> {
        let pending = self.kernel.pending_wakes()?;
        let (e, w) = crate::wake::resolve(&pending, name).map_err(not_found)?;
        Ok((e.id.clone(), w.id.clone()))
    }

    /// A reminder that fired, by its whole id: the execution it lived on and
    /// the wake as it was (the kernel keeps it for a snooze).
    pub(super) fn fired_wake(
        &self,
        id: &str,
    ) -> anyhow::Result<Option<(String, theseus_kernel::PendingWake)>> {
        if !id.starts_with("wak_") {
            return Ok(None);
        }
        self.store
            .get_meta::<(String, theseus_kernel::PendingWake)>(&format!(
                "{}{id}",
                theseus_kernel::wake_owner::FIRED_PREFIX
            ))
    }

    /// The wake as it is, for a verb's answer.
    fn edited(
        &self,
        execution_id: &str,
        wake: &theseus_kernel::PendingWake,
        home: bool,
    ) -> Result<WakeEditResult, RpcFailure> {
        let e = self
            .kernel
            .execution(execution_id)?
            .ok_or_else(|| not_found(format!("execution {execution_id} is gone")))?;
        let rec: Option<SessionRecord> = self.store.get_session(&e.session_id)?;
        Ok(WakeEditResult {
            wake: crate::wake::info(&e, wake, rec.and_then(|r| r.title)),
            home,
        })
    }
}

fn invalid(e: impl std::fmt::Display) -> RpcFailure {
    RpcFailure::new(error_code::INVALID_PARAMS, e.to_string())
}

fn invalid_of(e: anyhow::Error) -> RpcFailure {
    RpcFailure::invalid(e)
}

fn not_found(e: impl std::fmt::Display) -> RpcFailure {
    RpcFailure::new(error_code::NOT_FOUND, e.to_string())
}
