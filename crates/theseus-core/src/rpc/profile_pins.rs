//! `profile.pins` and `profile.unpin` (theseus-h66v4.2): the owner's "unpin all current live profiles and
//! tasks and sessions".
//!
//! - **What is pinned.** A session, or a task (a task's own session), held to a profile by
//!   `session.profile`: the META key `session.profile.<id>` holds one. `profile.pins` walks every session
//!   record and counts them: the pinned ones, and the totals beside, so zero pinned reads differently from
//!   zero sessions. A hold on a profile the config dropped is on record, so it counts (`configured`
//!   false), and a read that fails is an error, never a zero.
//! - **A task started on a profile is a pin too** (`task.start --profile`: `task_start.rs` calls it one,
//!   `turn/brief_step.rs` never asks the brief about it), though no hold records it. `profile.pins` counts
//!   it under its own kind, `task-start`, in `start_pinned`: a task, no turn run, no brief mark
//!   (`brief_step::marked`), whose execution has not begun or ended. `profile.unpin` does **not** clear it,
//!   and says so: the owner chose that model for that task by name, and a bulk let-go does not undo a named
//!   choice. Zero holds is not zero pins.
//! - **`profile.unpin`** is `profile.release` and more, never less: the live profile's pin ends exactly as
//!   `release` ends it (its `released` carried, the same frame and row), and with `all` every hold is let
//!   go through `session.profile`'s own let-go, so each is its own `session.profile_set` row and each
//!   restores the target the session ran on before it was held. The answer is `before`, `cleared`,
//!   `failed` and `after`, counted by the read, so the claim "none left" is a count, not a guess.
//! - **Authority** is `session.profile`'s, per hold: a caller who may not choose a session's model
//!   fails every hold, and `failed` names each one.

use theseus_protocol::ops::{
    PinnedSession, ProfilePinsResult, ProfileUnpinParams, ProfileUnpinResult, UnpinFailure,
};

use super::server::{Conn, RpcFailure};
use super::Core;
use crate::approval::Answerer;
use crate::session::SessionRecord;

impl Core {
    /// Every session and task held to a profile, with the totals they are counted in.
    pub fn profile_pins(&self) -> anyhow::Result<ProfilePinsResult> {
        let (live, live_source) = self.live_profile();
        let mut out = ProfilePinsResult {
            live,
            live_source,
            sessions: 0,
            tasks: 0,
            pinned_sessions: 0,
            pinned_tasks: 0,
            pins: vec![],
            start_pinned: vec![],
        };
        for rec in self.store.list_sessions::<SessionRecord>()? {
            let task = rec.task.is_some();
            match task {
                true => out.tasks += 1,
                false => out.sessions += 1,
            }
            if let Some(pin) = self.start_pin(&rec)? {
                out.start_pinned.push(pin);
            }
            let Some(profile) = self.runner.held_profile(&rec.session_id)? else {
                continue;
            };
            match task {
                true => out.pinned_tasks += 1,
                false => out.pinned_sessions += 1,
            }
            out.pins.push(PinnedSession {
                configured: self.cfg.all_profiles().contains_key(&profile),
                kind: if task { "task" } else { "session" }.into(),
                session_id: rec.session_id,
                profile,
            });
        }
        out.pins.sort_by(|a, b| a.session_id.cmp(&b.session_id));
        out.start_pinned
            .sort_by(|a, b| a.session_id.cmp(&b.session_id));
        Ok(out)
    }

    /// The pin a task was started with, while its first turn is still ahead: a task whose turn has not run
    /// and which carries no brief mark (`task.start` writes the mark only when no profile was asked for, so
    /// its absence is `brief_step`'s own meaning of "pinned"), whose execution has neither ended nor begun
    /// its first turn (a turn fixes its model once it starts, and later turns are continuations that no
    /// route moves). A mark that cannot be read reads as absent, so a failure counts a pin, never hides one.
    fn start_pin(&self, rec: &SessionRecord) -> anyhow::Result<Option<PinnedSession>> {
        let sid = &rec.session_id;
        if rec.task.is_none() || rec.turns > 0 || crate::turn::brief_step::marked(&self.store, sid)
        {
            return Ok(None);
        }
        if let Some(e) = rec
            .execution_id
            .as_deref()
            .map(|id| self.kernel.execution(id))
            .transpose()?
            .flatten()
        {
            if e.state.is_terminal() || e.state == theseus_kernel::ExecState::Running {
                return Ok(None);
            }
        }
        let Some(target) = &rec.last_target else {
            return Ok(None);
        };
        Ok(Some(PinnedSession {
            configured: self.cfg.all_profiles().contains_key(&target.profile),
            kind: "task-start".into(),
            session_id: sid.clone(),
            profile: target.profile.clone(),
        }))
    }

    /// Let go of every hold for `who`, each by `session.profile`'s own let-go (its row, its restore).
    /// A hold that cannot be let go is named in the second list; the rest still go.
    pub fn unpin_holds(&self, who: &Answerer) -> (Vec<PinnedSession>, Vec<UnpinFailure>) {
        let mut cleared = vec![];
        let mut failed = vec![];
        let pins = match self.profile_pins() {
            Ok(p) => p.pins,
            Err(e) => {
                failed.push(UnpinFailure {
                    session_id: String::new(),
                    error: format!("the holds were not read: {e:#}"),
                });
                return (cleared, failed);
            }
        };
        for pin in pins {
            let p = theseus_protocol::ops::SessionProfileParams {
                session_id: pin.session_id.clone(),
                profile: None,
                author: None,
                discord: None,
            };
            match self.session_set_profile(&p, who) {
                Ok(r) if r.profile.is_none() => cleared.push(pin),
                Ok(r) => failed.push(UnpinFailure {
                    session_id: pin.session_id,
                    error: r
                        .refused
                        .unwrap_or_else(|| "the hold is still on the session".into()),
                }),
                Err(e) => failed.push(UnpinFailure {
                    session_id: pin.session_id,
                    error: format!("{e:#}"),
                }),
            }
        }
        (cleared, failed)
    }

    /// A failed read of the holds, after the live pin's release: a store failure, and the answer says what
    /// was done.
    fn unread(live: &theseus_protocol::ProfileReleaseResult, e: anyhow::Error) -> RpcFailure {
        let did = match live.released {
            true => format!(
                "the live pin was released (from {}, now {})",
                live.previous, live.live
            ),
            false => format!("the live profile {} was already the config's", live.live),
        };
        RpcFailure::from(anyhow::anyhow!("the holds were not read: {e:#}; {did}"))
    }

    pub(super) fn profile_unpin(
        &self,
        p: ProfileUnpinParams,
        conn: Conn<'_>,
    ) -> Result<ProfileUnpinResult, RpcFailure> {
        // `release` first: one hold that cannot be read must not keep the live pin from going, as it does
        // not for `release`. The read fails after, and says the pin is released.
        let (was, was_source) = self.live_profile();
        let live = self.profile_release(conn)?;
        let mut before = self.profile_pins().map_err(|e| Self::unread(&live, e))?;
        // The live half of `before` is as it stood when the call came, not as the release left it.
        (before.live, before.live_source) = (was, was_source);
        let (cleared, failed) = match p.all {
            true => self.unpin_holds(&conn.answerer(None, None)),
            false => (vec![], vec![]),
        };
        let after = self.profile_pins().map_err(|e| Self::unread(&live, e))?;
        Ok(ProfileUnpinResult {
            released: live.released,
            previous: live.previous,
            live: live.live,
            by: live.by,
            all: p.all,
            before,
            cleared,
            failed,
            after,
        })
    }
}

#[cfg(test)]
#[path = "tests_profile_pins.rs"]
mod tests;
