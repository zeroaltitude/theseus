//! `task.rehome` (theseus-jid7t.2): a task's report goes to another
//! conversation or thread from now on.
//!
//! - **What moves** is where the report is posted: the task's record
//!   (`TaskOf.target`), its execution's `reports_to`, and the outbox's place
//!   for it (META `task.place.<session>`), in one frame with the
//!   `task.rehomed` row. A home with no place of its own (a terminal's
//!   conversation, a thread no place has shown) leaves the report on the
//!   board and in its parent's next turn.
//! - **What stays** is the parent: the budget is carved from it, its
//!   reservation settles there, and its next turn still reads the report
//!   (`Execution.parent` never changes), as the answer says.
//! - **Refused**: an ended task; a home that does not exist, is retired, is
//!   itself a task, or is in a shared place; a home whose data mark is weaker
//!   than the task's (a private task never reports into an open conversation).
//! - **Authority** is `task.start`'s (`Act::Task`): the owner, from a private
//!   place. No Discord code runs: the card follows the same records as any
//!   change from another surface.

use std::sync::Arc;

use serde_json::Value;
use theseus_kernel::SessionKind;
use theseus_protocol::method;
use theseus_protocol::ops::{DataMark, TaskRehomeParams, TaskRehomeResult};
use theseus_store::{kinds, NewRecord};

use super::confirms::Act;
use super::server::{route, Conn, RpcFailure};
use super::Core;
use crate::approval::{Answerer, Refusal};
use crate::fact;
use crate::session::SessionRecord;

/// Where a refused act says it is the owner's.
pub(super) fn task_failure(
    what: &'static str,
    m: &'static str,
) -> impl Fn(anyhow::Error) -> RpcFailure {
    move |e| match e.downcast::<Refusal>() {
        Ok(r) => RpcFailure {
            code: theseus_protocol::error_code::REFUSED,
            message: format!("{what} from {} does not count: {}", r.who, r.why),
            data: serde_json::json!({"who": r.who, "via": r.via, "why": r.why, "method": m}),
        },
        Err(e) => match e.downcast::<crate::task::NoSuchTask>() {
            Ok(n) => RpcFailure::new(theseus_protocol::error_code::NOT_FOUND, n.0),
            Err(e) => RpcFailure::invalid(e),
        },
    }
}

/// A home, resolved: its mark and the place its reports are posted at.
struct Home {
    mark: DataMark,
    target: Option<String>,
}

impl Core {
    pub(super) fn rpc_task_rehome(
        self: &Arc<Self>,
        params: Value,
        conn: Conn<'_>,
    ) -> Result<Value, RpcFailure> {
        route(params, |p: TaskRehomeParams| {
            let who = conn.answerer(p.author.clone(), p.discord.clone());
            self.task_rehome(&p, &who)
                .map_err(task_failure("a task's report home", method::TASK_REHOME))
        })
    }

    /// The conversation or thread `home` names, if a report may go there.
    fn home_for(&self, home: &str) -> anyhow::Result<Home> {
        let session = match home.strip_prefix("thread:") {
            Some(slug) => {
                let board = self.threads();
                let t = board
                    .get(slug)
                    .ok_or_else(|| anyhow::anyhow!("no thread {slug}"))?;
                anyhow::ensure!(
                    t.state != theseus_protocol::threads::ThreadState::Done,
                    "thread {slug} is done"
                );
                t.session_id.clone()
            }
            None => {
                let rec = self
                    .store
                    .get_session::<SessionRecord>(home)?
                    .ok_or_else(|| {
                        anyhow::anyhow!(
                            "no home {home}: name a conversation's session id or `thread:<slug>`"
                        )
                    })?;
                anyhow::ensure!(
                    rec.kind != SessionKind::Task && rec.task.is_none(),
                    "{home} is itself a task, not a conversation"
                );
                anyhow::ensure!(rec.retired.is_none(), "{home} is retired");
                anyhow::ensure!(
                    self.runner.class_of(home) != crate::places::PlaceClass::Shared,
                    "{home} is in a shared place, and a task's report never goes to one from here"
                );
                Some(home.to_string())
            }
        };
        // A failed read of the home's mark is a refusal, not the top mark: the top
        // would let any task's report in.
        let mark = match session.as_deref() {
            None => DataMark::Open,
            Some(s) => crate::content_wait::try_mark_of(&self.store, s).map_err(|e| {
                anyhow::anyhow!(
                    "the data mark of {home} cannot be read ({e}): a report never goes to a \
                         home whose mark is unknown"
                )
            })?,
        };
        let target = session
            .as_deref()
            .and_then(|s| self.outbox.report_target(self.outbox.target(s)).0);
        Ok(Home { mark, target })
    }

    /// Give the task `p.task` the home `p.home`, for `who`.
    pub fn task_rehome(
        &self,
        p: &TaskRehomeParams,
        who: &Answerer,
    ) -> anyhow::Result<TaskRehomeResult> {
        let what = format!("Moving the report of task {} to {}", p.task, p.home);
        self.judge_act(who, Act::Task { what: &what })?;
        let tasks = self.kernel.tasks(None)?;
        let e = crate::task::resolve(&tasks, &p.task)
            .map_err(crate::task::NoSuchTask)?
            .clone();
        if e.state.is_terminal() {
            anyhow::bail!("Task {} has ended ({}).", e.session_id, e.state.as_str());
        }
        let home = self.home_for(&p.home)?;
        let mark = crate::content_wait::mark_of(&self.store, &e.session_id);
        anyhow::ensure!(
            home.mark >= mark,
            "task {} is {mark:?} and {} is {:?}: a report never goes to a weaker home",
            crate::task::short(&e.session_id),
            p.home,
            home.mark,
        );
        let sid = e.session_id.as_str();
        let by = who.label.as_str();
        let from = self
            .store
            .with_session(sid, |mut rec| {
                let task = rec
                    .task
                    .as_mut()
                    .ok_or_else(|| anyhow::anyhow!("{sid} is not a task"))?;
                let from = std::mem::replace(&mut task.target, home.target.clone());
                let rehomed = fact::ops::TaskRehomed {
                    task: &e.id,
                    session_id: sid,
                    from: from.as_deref(),
                    to: home.target.as_deref(),
                    by,
                };
                let session = NewRecord::json(kinds::SESSION, Some(sid), &rec)?;
                let row = fact::row(&rehomed, Some(sid), None)?;
                let place = self.outbox.task_place_record(sid, home.target.as_deref())?;
                self.kernel.edit_task(&e.id, |ex| {
                    // It may have ended between the read and the lock.
                    anyhow::ensure!(!ex.state.is_terminal(), "Task {sid} has ended");
                    ex.reports_to = home.target.clone();
                    Ok(((), vec![session, row, place]))
                })?;
                Ok(from)
            })?
            .ok_or_else(|| anyhow::anyhow!("no session {sid}"))?;
        match home.target.as_deref() {
            Some(t) => self.outbox.task_bound(sid, t),
            None => self.outbox.task_unbound(sid),
        }
        self.rec_for(sid).announce(&fact::ops::TaskRehomed {
            task: &e.id,
            session_id: sid,
            from: from.as_deref(),
            to: home.target.as_deref(),
            by,
        });
        let parent = &e.parent;
        Ok(TaskRehomeResult {
            task: sid.to_string(),
            from,
            to: home.target,
            note: format!(
                "its budget stays carved from its original parent ({0}), and that parent's \
                 next turn still reads the report",
                parent.as_deref().unwrap_or("none")
            ),
        })
    }
}

#[cfg(test)]
#[path = "tests_task_rehome.rs"]
mod tests;
