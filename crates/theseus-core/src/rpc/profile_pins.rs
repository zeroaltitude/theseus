//! `profile.pins` and `profile.unpin` (theseus-h66v4.2): the owner's "unpin all current live profiles and
//! tasks and sessions".
//!
//! - **What is pinned.** A session, or a task (a task's own session), held to a profile by
//!   `session.profile`: the META key `session.profile.<id>` holds one. `profile.pins` walks every session
//!   record and counts them: the pinned ones, and the totals beside, so zero pinned reads differently from
//!   zero sessions. A hold on a profile the config dropped is on record, so it counts (`configured`
//!   false), and a read that fails is an error, never a zero.
//! - **A task with a model of its own is a pin too.** `task.start --profile` calls it one
//!   (`task_start.rs`), and `turn/brief_step.rs` and `turn/checkpoint_step.rs` ask Jev nothing about a task
//!   with no `task.route.<session>` mark, **on every turn of its life**: the brief on the first, the
//!   checkpoints at every loop end. So the set is every live task with no mark and a `last_target`, whatever
//!   turn it is on and whether it runs or sleeps; a task that has ended is not in it. `profile.pins` counts
//!   them in `task_pins`, beside the holds and never inside them, told apart by who made them, which the
//!   code decides: `task.start` is the only path that writes a `task.started_by_owner` row, and it writes
//!   the mark exactly when no profile was named (`task.create`, a check, a draft's reviewer, a routine's run
//!   and a retry write `last_target` from the model they were given and never the mark, and no such row).
//!   `task-start` is the owner's (the row is on record, or the task is a retry of one: a retry copies the
//!   model and writes no row), `task-fixed` is Theseus's (no such row, and every row read well), and
//!   `task-unknown` is a task the ledger could not say, because it could not be read or a row that bears on
//!   it could not be decoded: counted, never hidden, never cleared. Each task also carries its `state`
//!   (`pending`, `asleep`, `running`), because letting go changes different things in each.
//!   Zero holds is not zero pins.
//! - **`profile.unpin`** is `profile.release` and more, never less: the live profile's pin ends exactly as
//!   `release` ends it (its `released` carried, the same frame and row), and with `all` every hold is let
//!   go through `session.profile`'s own let-go, so each is its own `session.profile_set` row and each
//!   restores the target the session ran on before it was held. `all` also clears each `task-start` task, by
//!   writing the `task.route.<session>` mark the task would have had with no profile, in one frame with a
//!   `session.profile_set` row (`from` the profile, `to` none), so a running task is let go without being
//!   cancelled. What the mark changes depends on its `state`: a `pending` task meets the brief on its first
//!   turn; an `asleep` one never meets the brief again and only its checkpoints can move it, from its next
//!   turn; a `running` one finishes its current turn on the model it started on. A `task-fixed` or
//!   `task-unknown` task is not cleared: nobody named its model (or nobody can say who did), there is no
//!   `--profile` to drop, and a mark on a check's or a reviewer's task could undo the model its independence
//!   rests on. The answer is `before`, `cleared`,
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

/// Who made the tasks no brief or checkpoint routes, as `makers` reads it.
struct Makers {
    /// Tasks whose model the owner chose: `task.start --profile`, or a retry of one.
    owned: std::collections::HashSet<String>,
    /// A ledger row that bears on this could not be read.
    unreadable: bool,
}

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
            task_pins: vec![],
        };
        for rec in self.store.list_sessions::<SessionRecord>()? {
            let task = rec.task.is_some();
            match task {
                true => out.tasks += 1,
                false => out.sessions += 1,
            }
            if let Some(pin) = self.task_pin(&rec)? {
                out.task_pins.push(pin);
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
                state: None,
            });
        }
        out.pins.sort_by(|a, b| a.session_id.cmp(&b.session_id));
        out.task_pins
            .sort_by(|a, b| a.session_id.cmp(&b.session_id));
        if !out.task_pins.is_empty() {
            let makers = self.makers();
            for pin in &mut out.task_pins {
                pin.kind = match &makers {
                    Ok(m) if m.owned.contains(&pin.session_id) => "task-start",
                    Ok(m) if m.unreadable => "task-unknown",
                    Ok(_) => "task-fixed",
                    Err(_) => "task-unknown",
                }
                .into();
            }
        }
        Ok(out)
    }

    /// A live task no brief or checkpoint routes: no `task.route.<session>` mark (`task.start` writes it only
    /// when no profile was named, so its absence is `brief_step`'s own meaning of "pinned") and a model
    /// recorded in `last_target`, whatever turn it is on, whether it runs or sleeps. Only an ended task is
    /// out. A mark that cannot be read reads as absent, so a failure counts a pin, never hides one. The kind
    /// is set after, once for the whole read.
    fn task_pin(&self, rec: &SessionRecord) -> anyhow::Result<Option<PinnedSession>> {
        let sid = &rec.session_id;
        if rec.task.is_none() || crate::turn::brief_step::marked(&self.store, sid) {
            return Ok(None);
        }
        let exec = rec
            .execution_id
            .as_deref()
            .map(|id| self.kernel.execution(id))
            .transpose()?
            .flatten();
        if exec.as_ref().is_some_and(|e| e.state.is_terminal()) {
            return Ok(None);
        }
        let Some(target) = &rec.last_target else {
            return Ok(None);
        };
        // Where it is in its life decides what letting go changes: the brief runs on a task's first turn
        // only, and a turn's checkpoints are read once, when it starts.
        let state = match exec.map(|e| e.state) {
            Some(theseus_kernel::ExecState::Running) => "running",
            _ if rec.turns == 0 => "pending",
            _ => "asleep",
        };
        Ok(Some(PinnedSession {
            configured: self.cfg.all_profiles().contains_key(&target.profile),
            kind: String::new(),
            session_id: sid.clone(),
            profile: target.profile.clone(),
            state: Some(state.into()),
        }))
    }

    /// Every ledger row of `kind`, and how many of its rows could not be decoded. An error when the ledger's
    /// index cannot be read.
    fn ledger_rows_counted(
        &self,
        kind: theseus_protocol::LedgerKind,
    ) -> anyhow::Result<(Vec<crate::ledger::LedgerRow>, usize)> {
        use theseus_store::pages::ledger_kind;
        use theseus_store::{kinds, Page};
        let (mut rows, mut unreadable, mut before) = (vec![], 0, None);
        loop {
            let page = Page {
                kind: kinds::LEDGER,
                tags: vec![ledger_kind(kind.as_str())],
                before,
                limit: 512,
                ..Default::default()
            };
            let got = self
                .store
                .ledger_page(&page)?
                .ok_or_else(|| anyhow::anyhow!("the ledger's index is still being built"))?;
            for r in &got.records {
                match r.decode::<crate::ledger::LedgerRow>() {
                    Ok(row) => rows.push(row),
                    Err(_) => unreadable += 1,
                }
            }
            match (got.more, got.first) {
                (true, Some(first)) => before = Some(first),
                _ => return Ok((rows, unreadable)),
            }
        }
    }

    /// Whose choice each task's model was, as the ledger has it. `task.start` is the only path that writes a
    /// `task.started_by_owner` row, so a task with one and no mark is the owner's pin. A retry copies its
    /// failed task's model and writes neither mark nor row, so it inherits "the owner's" through its
    /// `task.retried` row when the task it retries is the owner's (down a chain of retries); a task the brief
    /// chose the model of is marked, and its retry is Theseus's. Any row that cannot be decoded, or names no
    /// session, leaves `unreadable` set: the tasks it might have named are then `task-unknown`, never
    /// `task-fixed`. An error when the ledger's index cannot be read.
    fn makers(&self) -> anyhow::Result<Makers> {
        use theseus_protocol::LedgerKind;
        let chosen = |sid: &str| !crate::turn::brief_step::marked(&self.store, sid);
        let (started, mut unreadable) = self.ledger_rows_counted(LedgerKind::TaskStartedByOwner)?;
        let (retried, bad) = self.ledger_rows_counted(LedgerKind::TaskRetried)?;
        unreadable += bad;
        let mut owned = std::collections::HashSet::new();
        for r in started {
            match r.session_id {
                Some(sid) if chosen(&sid) => {
                    owned.insert(sid);
                }
                Some(_) => {}
                None => unreadable += 1,
            }
        }
        let mut retries = vec![];
        for r in retried {
            match (r.session_id, r.data["failed_session"].as_str()) {
                (Some(new), Some(failed)) => retries.push((new, failed.to_string())),
                _ => unreadable += 1,
            }
        }
        loop {
            let before = owned.len();
            for (new, failed) in &retries {
                if owned.contains(failed) && !owned.contains(new) && chosen(new) {
                    owned.insert(new.clone());
                }
            }
            if owned.len() == before {
                return Ok(Makers {
                    owned,
                    unreadable: unreadable > 0,
                });
            }
        }
    }

    /// Let go of a `task-start` task for `who`: write the brief's mark and the `session.profile_set` row in
    /// one frame, as a hold's let-go writes its own. Authority is `session.profile`'s.
    fn unpin_start(&self, pin: &PinnedSession, who: &Answerer) -> anyhow::Result<()> {
        let sid = pin.session_id.as_str();
        let what = format!("Choosing the model of session {sid}");
        self.judge_act(
            who,
            super::Act::Session {
                method: theseus_protocol::method::SESSION_PROFILE,
                what: &what,
            },
        )?;
        let set = crate::fact::ops::SessionProfileSet {
            session_id: sid,
            from: Some(&pin.profile),
            to: None,
            by: who.label.as_str(),
        };
        self.store.append(&[
            crate::turn::brief_step::mark(sid)?,
            crate::fact::row(&set, Some(sid), None)?,
        ])?;
        self.rec_for(sid).announce(&set);
        Ok(())
    }

    /// Let go of every hold for `who`, each by `session.profile`'s own let-go (its row, its restore), then of
    /// every task the owner started on a profile (`unpin_start`: its mark, its row). A pin that cannot be let
    /// go is named in the second list; the rest still go. A task Theseus made, or whose maker is unknown, is
    /// not touched.
    pub fn unpin_holds(&self, who: &Answerer) -> (Vec<PinnedSession>, Vec<UnpinFailure>) {
        let mut cleared = vec![];
        let mut failed = vec![];
        let (pins, starts) = match self.profile_pins() {
            Ok(p) => (p.pins, p.task_pins),
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
        for pin in starts.into_iter().filter(|p| p.kind == "task-start") {
            match self.unpin_start(&pin, who) {
                Ok(()) => cleared.push(pin),
                Err(e) => failed.push(UnpinFailure {
                    session_id: pin.session_id,
                    error: format!("{e:#}"),
                }),
            }
        }
        (cleared, failed)
    }

    /// A failed read of the holds, after the live pin's release: a store failure, and the answer says what
    /// was done, the let-gos and the refusals of an `all` included, so a failed read after them hides none.
    fn unread(
        live: &theseus_protocol::ProfileReleaseResult,
        (cleared, failed): (&[PinnedSession], &[UnpinFailure]),
        e: anyhow::Error,
    ) -> RpcFailure {
        let mut did = match live.released {
            true => format!(
                "the live pin was released (from {}, now {})",
                live.previous, live.live
            ),
            false => format!("the live profile {} was already the config's", live.live),
        };
        if !cleared.is_empty() || !failed.is_empty() {
            did.push_str(&format!(
                "; {} let go, {} not let go",
                cleared.len(),
                failed.len()
            ));
            for f in failed {
                did.push_str(&format!(" ({}: {})", f.session_id, f.error));
            }
        }
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
        let mut before = self
            .profile_pins()
            .map_err(|e| Self::unread(&live, (&[], &[]), e))?;
        // The live half of `before` is as it stood when the call came, not as the release left it.
        (before.live, before.live_source) = (was, was_source);
        let (cleared, failed) = match p.all {
            true => self.unpin_holds(&conn.answerer(None, None)),
            false => (vec![], vec![]),
        };
        let after = self
            .profile_pins()
            .map_err(|e| Self::unread(&live, (&cleared, &failed), e))?;
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
