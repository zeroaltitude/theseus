//! A self build parked on a question to the owner (issue 20). On 2026-11-17 a build's turn ended on
//! a question, its execution `Waiting`, and the pipeline read that as a build whose turn had ended
//! with no branch (`queues::Queues::ended`): it failed the item and removed its working copy. The
//! question stayed open and reminded the owner for an hour and a half; his answer then woke a
//! session whose work was gone, which spent more and built nothing. And `theseus self pipeline`
//! showed nothing building the whole time, so a restart looked safe.
//!
//! Three things, each one place:
//!
//! - **A build parked on a question is building**, waiting on the owner ([`waiting_words`] is how
//!   `theseus self pipeline` says so), and holds its slot: its working copy is open and its turn
//!   will go on when he answers. That needs the working copy: one that is gone is a build that
//!   ended (`queues`).
//! - **A question outlives no build.** Every tick, a question of a self build session that is not
//!   the question of a build in the building queue is withdrawn ([`Core::withdraw_ended_questions`]):
//!   the item failed, was set aside, dropped, retried or done, its working copy is closed or
//!   removed, or its session was superseded by a newer start. The card is edited to say so
//!   ([`ENDED_WHY`]), its reminders stop (`rpc::question_waits::forget_noticed` ends a count with
//!   its question), and the execution it parked is stopped, so nothing can wake it.
//! - **An answer after that is late**, recorded as a refused act and nothing else ([`late`]).
//!
//! A restart with a parked build changes none of this: the question and the working copy are on
//! disk, the build is read as parked again, and its question is as wanted as before. A restart that
//! comes after the working copy went finds the question unwanted at the first tick.
//! `docs/design/self-own-tree-asks.md` has the rest.

use anyhow::Result;
use theseus_kernel::Action;
use theseus_protocol::tasks::TaskRecord;

use super::queues::{Entry, Queues};
use crate::approval::{Answerer, Refusal};
use crate::outbox::Closed;
use crate::rpc::Core;
use crate::rsi::tree::{self, Place};
use crate::rsi::SELF_PRINCIPAL;

/// Who a withdrawal is attributed to in the kernel's resolution (`declined by withdrawn: ...`),
/// which `outbox::Closed::of` reads back as a withdrawn card.
const WITHDRAWN: &str = "withdrawn";

/// What the card, the record and the owner are told.
pub(crate) const ENDED_WHY: &str = "no longer needed: the build it belonged to has ended";

/// What `theseus self pipeline` says of a build parked on `what`: the answer's command first, since
/// the view cuts a long line from its end.
pub(super) fn waiting_words(what: &str, id: &str) -> String {
    let what = crate::scrub::safe::head(what, 110).0;
    format!(
        "waiting on you: {what} (answer it with `theseus answer {}`)",
        crate::task::short(id)
    )
}

/// The refusal an answer to a withdrawn question gets, or none when the question was not
/// withdrawn. It is recorded as a refused act (`approval.refused`) by the caller, so a late
/// answer is a row and a message and nothing else: it binds nothing, runs nothing, wakes nothing.
pub(crate) fn late(a: &Action, who: &Answerer) -> Option<Refusal> {
    let r = a.resolution.as_deref()?;
    r.starts_with(&format!("declined by {WITHDRAWN}: "))
        .then(|| Refusal {
            who: who.who(),
            via: who.via(),
            why: format!(
            "that question was withdrawn ({ENDED_WHY}), so this answer is late: nothing was run, \
             nothing was spent and nothing was woken"
        ),
        })
}

/// The self item whose build `session` is (or was): the task of the self principal that names the
/// session as its current one or in its evidence of starts.
pub(super) fn item_of<'a>(tasks: &'a [TaskRecord], session: &str) -> Option<&'a TaskRecord> {
    let started = format!("session:{session}");
    tasks.iter().find(|t| {
        t.origin.principal == SELF_PRINCIPAL
            && (t.session.as_deref() == Some(session)
                || t.evidence.iter().any(|e| e.identity == started))
    })
}

impl Core {
    /// A build whose turn ended on a question to the owner is building, waiting on him, while its
    /// working copy is open: `e` is marked so, and says what it waits for. One whose working copy is
    /// gone is a build that ended, and is left as it was.
    pub(super) fn mark_parked(&self, e: &mut Entry, pending: &[Action], place: &Place, item: &str) {
        let asked = e
            .execution
            .as_deref()
            .and_then(|exec| pending.iter().find(|a| a.execution_id == exec));
        if let Some(a) = asked.filter(|_| tree::find(place, item).is_some()) {
            e.live = true;
            e.parked_on = Some(a.correlation_id.clone());
            e.why = Some(self.parked_words(a));
        }
    }

    /// The title of the self item whose build asked `session`'s question, or none when the session
    /// is not a self build's. The reminder's cadence and its words read this.
    pub(crate) fn self_build_title(&self, session: &str) -> Option<String> {
        let tasks = crate::task_graph::all(&self.store).ok()?;
        item_of(&tasks, session).map(|t| t.title.clone())
    }

    /// The reminder of a waiting self build's question: what builds, what it wants, how long it has
    /// waited, how to answer, and how often it will be told again.
    pub(crate) fn build_wait_words(
        &self,
        a: &Action,
        title: &str,
        waited_ms: u64,
        told: u64,
        life_ms: u64,
    ) -> String {
        let what = self.what_of(a).unwrap_or_else(|| format!("`{}`", a.tool));
        let pace = match told {
            0 | 1 => format!(
                "Theseus will remind you once more in {}, then once an hour.",
                crate::rpc::waited_words(life_ms)
            ),
            _ => "Theseus will remind you once an hour from here.".to_string(),
        };
        format!(
            "⏳ A change Theseus is building for itself (\u{201c}{title}\u{201d}) is waiting on you: it \
             wants to {what}. It has waited {}. It is held, not cancelled: it runs or is declined \
             when you answer (`theseus answer {}`). {pace}",
            crate::rpc::waited_words(waited_ms),
            crate::task::short(&a.correlation_id),
        )
    }

    /// The words of `a`, as the card says what it wants.
    pub(super) fn parked_words(&self, a: &Action) -> String {
        let what = self.what_of(a).unwrap_or_else(|| a.tool.clone());
        waiting_words(&what, &a.correlation_id)
    }

    /// Withdraw every open question of a self build that is not in the building queue `q`. Returns
    /// how many were withdrawn. A question of any other session (a person's, a task's, a review's)
    /// is never read: the session must belong to a self item.
    pub(super) async fn withdraw_ended_questions(&self, q: &Queues) -> Result<usize> {
        let mut withdrawn = 0;
        for a in self.kernel.pending_confirms()? {
            let Some(t) = item_of(&q.tasks, &a.session_id) else {
                continue;
            };
            let wanted = q
                .building
                .iter()
                .any(|e| e.item == t.id && e.session.as_deref() == Some(a.session_id.as_str()));
            if wanted {
                continue;
            }
            match self.withdraw(&a).await {
                Ok(()) => withdrawn += 1,
                Err(e) => tracing::warn!(error = %format!("{e:#}"), item = %t.id,
                    correlation_id = %a.correlation_id, "a question of an ended build was not withdrawn"),
            }
        }
        Ok(withdrawn)
    }

    /// One question withdrawn: the call declined in the kernel's frame, its clients told, its card
    /// edited, and the execution it parked ended (cancelled), so that nothing wakes it.
    async fn withdraw(&self, a: &Action) -> Result<()> {
        self.kernel.frame(&[&a.execution_id], |k| {
            k.decline_action(&a.correlation_id, WITHDRAWN, ENDED_WHY)?;
            Ok(())
        })?;
        self.session_rec(&a.session_id)
            .announce(&crate::fact::answer::BuildQuestionWithdrawn {
                action: a,
                why: ENDED_WHY,
            });
        self.card_closed(
            &a.correlation_id,
            Closed {
                note: Some(ENDED_WHY.into()),
                ..Closed::new("withdrawn", None)
            },
        );
        self.end_execution(&a.execution_id).await?;
        self.admission.notify_waiters();
        Ok(())
    }

    /// An execution of a build that is over, ended: cancelled, which settles whatever it planned and
    /// never sent, stops its running calls, terminals and jobs, and makes it terminal, so no call of
    /// it can start. Nothing when it has ended already.
    async fn end_execution(&self, exec: &str) -> Result<bool> {
        let live = self
            .kernel
            .execution(exec)?
            .is_some_and(|e| !e.state.is_terminal());
        if live {
            self.cancel_execution(exec, "self-pipeline: the build ended")
                .await?;
        }
        Ok(live)
    }

    /// The executions of every session that was ever `item`'s build: the item's `execution:` and
    /// `session:` evidence, and the execution each such session record names.
    fn executions_of(&self, item: &str) -> Vec<String> {
        let Ok(Some(t)) = crate::task_graph::get(&self.store, item) else {
            return Vec::new();
        };
        let mut sessions: Vec<String> = t.session.clone().into_iter().collect();
        let mut out: Vec<String> = Vec::new();
        for e in &t.evidence {
            if let Some(s) = e.identity.strip_prefix("session:") {
                sessions.push(s.to_string());
            } else if let Some(x) = e.identity.strip_prefix("execution:") {
                out.push(x.to_string());
            }
        }
        for s in sessions {
            let rec: Option<crate::session::SessionRecord> =
                self.store.get_session(&s).ok().flatten();
            out.extend(rec.and_then(|r| r.execution_id));
        }
        out.retain(|x| !x.is_empty());
        out.sort();
        out.dedup();
        out
    }

    /// Every live session of `item`'s build ended, its open questions withdrawn first, so that
    /// nothing of it can run once its working copy is gone (issue 20: a session that kept calling in
    /// a tree removed under it failed at launch, and the failure was filed as a fault of the tool).
    /// A session that is waiting on the owner counts as live. `spare` is the one session that is
    /// making this call itself (a builder closing its own tree as it hands its change in): its turn
    /// is running and is not ended from inside. Returns how many executions were ended.
    pub(crate) async fn end_item_sessions(&self, item: &str, spare: Option<&str>) -> Result<usize> {
        let spared = spare.and_then(|s| {
            let rec: Option<crate::session::SessionRecord> = self.store.get_session(s).ok()?;
            rec?.execution_id
        });
        let pending = self.kernel.pending_confirms()?;
        let mut ended = 0;
        for exec in self.executions_of(item) {
            if spared.as_deref() == Some(exec.as_str()) {
                continue;
            }
            let live = self
                .kernel
                .execution(&exec)?
                .is_some_and(|e| !e.state.is_terminal());
            for a in pending.iter().filter(|a| a.execution_id == exec) {
                if let Err(e) = self.withdraw(a).await {
                    tracing::warn!(error = %format!("{e:#}"), item,
                        correlation_id = %a.correlation_id, "a question of an ended build was not withdrawn");
                }
            }
            self.end_execution(&exec).await?;
            ended += usize::from(live);
        }
        Ok(ended)
    }
}
