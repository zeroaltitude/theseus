//! `execution.dismiss` and `task.retry` (theseus-7zgsm): the owner's two
//! verbs on a failure. A failed task stays flagged until it is dismissed, and
//! the daemon records the dismissal, so every surface agrees; a retry opens a
//! new, linked task.
//!
//! - **Dismiss** is the kernel's one frame (`Kernel::dismiss_execution`): the
//!   execution's own record says so, so a restart and the push's seed keep it.
//! - **Retry** reads the failed task's brief and quoted pieces and opens a new
//!   task from them under the same parent, in the kernel's one frame
//!   (`Kernel::open_retry`): the new task's session, its brief (the same
//!   words, and a line saying it retries a failed task), its arrangement (the
//!   same pieces, each a `derived_from` edge), its task record, the
//!   `task.retried` row, the carve (never more than the failed one had), and
//!   the failed one dismissed.
//! - Both are the owner's, judged as `session.retire` is (`Act::Session`): the
//!   owner, from a private place, and refused by the CLI inside a job
//!   (`client::OPERATORS`).

use std::sync::Arc;

use serde_json::{json, Value};
use theseus_kernel::micros_to_usd;
use theseus_protocol::{
    error_code, method, ExecutionDismissParams, ExecutionDismissResult, TaskRetryParams,
    TaskRetryResult,
};
use theseus_store::NewRecord;

use super::confirms::Act;
use super::server::{route, Conn, RpcFailure};
use super::Core;
use crate::approval::{Answerer, Refusal};
use crate::fact;
use crate::node::{Body, Node, Origin};
use crate::session::{SessionRecord, TaskOf};

/// The methods routed here.
pub(super) const OWN: [&str; 2] = [method::EXECUTION_DISMISS, method::TASK_RETRY];

impl Core {
    pub(super) fn rpc_retry(
        self: &Arc<Self>,
        m: &str,
        params: Value,
        conn: Conn<'_>,
    ) -> Result<Value, RpcFailure> {
        match m {
            method::EXECUTION_DISMISS => route(params, |p| self.execution_dismiss(p, conn)),
            _ => route(params, |p| self.task_retry(p, conn)),
        }
    }

    fn execution_dismiss(
        &self,
        p: ExecutionDismissParams,
        conn: Conn<'_>,
    ) -> Result<ExecutionDismissResult, RpcFailure> {
        let who = conn.answerer(None, p.discord.clone());
        let done = self
            .dismiss_failure(&p.execution_id, &who)
            .map_err(failure)?;
        Ok(ExecutionDismissResult {
            execution_id: p.execution_id,
            dismissed_at_ms: done.at_ms,
        })
    }

    fn task_retry(
        &self,
        p: TaskRetryParams,
        conn: Conn<'_>,
    ) -> Result<TaskRetryResult, RpcFailure> {
        let who = conn.answerer(None, p.discord.clone());
        self.retry_failed(&p.execution_id, p.note.as_deref(), &who)
            .map_err(failure)
    }

    /// The owner dismisses the failure `execution_id` is in: judged as the
    /// owner's act (`Act::Session`), then the kernel's one frame, and the
    /// narrative says it.
    pub fn dismiss_failure(
        &self,
        execution_id: &str,
        who: &Answerer,
    ) -> anyhow::Result<theseus_kernel::retry::Dismissed> {
        let what = format!("Dismissing the failure of {execution_id}");
        self.judge_act(
            who,
            Act::Session {
                method: method::EXECUTION_DISMISS,
                what: &what,
            },
        )?;
        let done = self.kernel.dismiss_execution(execution_id)?;
        if done.written {
            if let Some(e) = self.kernel.execution(execution_id)? {
                self.rec_for(&e.session_id)
                    .announce(&fact::retry::FailureDismissed {
                        execution_id: &e.id,
                        session_id: &e.session_id,
                        state: e.state.as_str(),
                        by: &who.label,
                    });
            }
        }
        Ok(done)
    }

    /// A fact's context for a core act outside a turn: the session's
    /// watchers hear it, and the narrative says it.
    pub(super) fn rec_for<'a>(&'a self, session: &'a str) -> fact::Rec<'a> {
        fact::Rec {
            narrator: &self.narrator,
            session: Some(session),
            turn: None,
            to: fact::To::Everyone(&self.bus),
            store: &self.store,
        }
    }

    /// The owner tries the failed task `failed_id` again, as a new task
    /// linked to it (see the module's note).
    pub fn retry_failed(
        &self,
        failed_id: &str,
        note: Option<&str>,
        who: &Answerer,
    ) -> anyhow::Result<TaskRetryResult> {
        let what = format!("Retrying the failed task {failed_id}");
        self.judge_act(
            who,
            Act::Session {
                method: method::TASK_RETRY,
                what: &what,
            },
        )?;
        let note = note.map(str::trim).filter(|n| !n.is_empty());
        let by = who.label.as_str();
        let failed = self
            .kernel
            .execution(failed_id)?
            .ok_or_else(|| anyhow::anyhow!("no execution {failed_id}"))?;
        let copy = self.copy_of(&failed)?;
        let correlation_id = crate::new_id("act");
        let target = copy.task_of.target.clone();
        let opened = self
            .kernel
            .open_retry(failed_id, &correlation_id, |failed, task| {
                copy.records(self, &correlation_id, failed, task, (note, by))
            })?;
        let (task, failed) = (&opened.task, &opened.failed);
        if let Some(t) = &target {
            self.outbox.task_bound(&task.session_id, t);
        }
        let rec = self.rec_for(&task.session_id);
        rec.announce(&fact::retry::TaskRetried {
            failed_execution: &failed.id,
            failed_session: &failed.session_id,
            execution_id: &task.id,
            session_id: &task.session_id,
            limit: task.budget.limit_micros,
            failed_limit: failed.budget.limit_micros,
            note,
            by,
        });
        Ok(TaskRetryResult {
            task_id: task.session_id.clone(),
            execution_id: task.id.clone(),
            retry_of: failed.id.clone(),
            limit_usd: micros_to_usd(task.budget.limit_micros),
            failed_limit_usd: micros_to_usd(failed.budget.limit_micros),
        })
    }
}

/// What a retry copies from the failed task: its session record, its brief's
/// words, its pieces and claim, and its task record.
struct Copy {
    old: SessionRecord,
    task_of: TaskOf,
    words: String,
    pieces: Vec<crate::arrangement::Piece>,
    fidelity_ack: bool,
    claim: Option<crate::check::Claim>,
    graph_old: Option<crate::task_graph::TaskRecord>,
}

impl Core {
    /// Read what `failed` leaves to copy. A task from before its brief or
    /// session was kept cannot be retried, and says so.
    fn copy_of(&self, failed: &theseus_kernel::Execution) -> anyhow::Result<Copy> {
        let bad = |m: String| anyhow::anyhow!(m);
        let old = self
            .store
            .get_session::<SessionRecord>(&failed.session_id)?
            .ok_or_else(|| bad(format!("no session {}", failed.session_id)))?;
        let Some(task_of) = old.task.clone() else {
            return Err(bad(format!(
                "{} is not a task: only a failed task can be retried",
                failed.id
            )));
        };
        let no_brief = || bad(format!("task {} has no brief to retry", failed.id));
        let brief = crate::task::brief(&self.store, &failed.session_id)?.ok_or_else(no_brief)?;
        let Body::UserMessage { text, .. } = &brief.body else {
            return Err(no_brief());
        };
        let words = text
            .split_once("\n\n")
            .map_or(text.as_str(), |(_, b)| b)
            .to_string();
        let arranged = task_of
            .arrangement
            .as_deref()
            .and_then(|id| self.store.get_node(id).ok().flatten())
            .map(|(_, n)| n);
        let (pieces, fidelity_ack, claim) = match arranged.map(|n| n.body) {
            Some(Body::Arrangement {
                pieces,
                fidelity_ack,
                claim,
            }) => (pieces, fidelity_ack, claim),
            _ => (Vec::new(), false, None),
        };
        let graph_old = crate::task_graph::all(&self.store)?
            .into_iter()
            .find(|t| t.session.as_deref() == Some(failed.session_id.as_str()));
        Ok(Copy {
            old,
            task_of,
            words,
            pieces,
            fidelity_ack,
            claim,
            graph_old,
        })
    }
}

impl Copy {
    /// The new task's record in the task graph: the failed one's title,
    /// objective, acceptance, parent and deps, in progress again.
    fn graph_record(&self, task: &theseus_kernel::Execution) -> anyhow::Result<Option<NewRecord>> {
        let Some(g) = &self.graph_old else {
            return Ok(None);
        };
        let graph = crate::task_graph::NewTask {
            id: crate::task_graph::of_session(&task.session_id),
            title: &g.title,
            objective: g.objective.clone(),
            acceptance: g.acceptance.clone(),
            parent: g.parent.clone(),
            deps: g.deps.clone(),
            session: Some(task.session_id.clone()),
            origin: g.origin.clone(),
            state: crate::task_graph::TaskState::InProgress,
        }
        .build(theseus_protocol::now_unix_ms());
        Ok(Some(crate::task_graph::record(&graph)?))
    }

    /// The records the new task's frame carries: its session, its brief, its arrangement and the
    /// edges to its pieces, where it reports, its task record, and the
    /// `task.retried` row.
    fn records(
        &self,
        core: &Core,
        correlation_id: &str,
        failed: &theseus_kernel::Execution,
        task: &theseus_kernel::Execution,
        (note, by): (Option<&str>, &str),
    ) -> anyhow::Result<Vec<NewRecord>> {
        let task_of = &self.task_of;
        let mut rec = SessionRecord::with_id(
            task.session_id.clone(),
            theseus_protocol::SessionKind::Task,
            Some("task".into()),
        );
        rec.title = self.old.title.clone();
        rec.execution_id = Some(task.id.clone());
        rec.dir = self.old.dir.clone();
        rec.last_target = self.old.last_target.clone();
        let author = format!("session:{}", task_of.parent_session);
        let mut arrangement = Node::arrangement(
            &task.session_id,
            &author,
            self.pieces.clone(),
            self.fidelity_ack,
        );
        if let Body::Arrangement { claim: c, .. } = &mut arrangement.body {
            c.clone_from(&self.claim);
        }
        rec.task = Some(TaskOf {
            parent_session: task_of.parent_session.clone(),
            parent_execution: task_of.parent_execution.clone(),
            by: correlation_id.to_string(),
            target: task_of.target.clone(),
            arrangement: Some(arrangement.id.clone()),
            check: task_of.check.clone(),
            notify_done: false,
        });
        let text = retry_brief(
            &task.session_id,
            &task_of.parent_session,
            &self.words,
            &failed.session_id,
            failed.ended_reason.as_deref(),
            note,
        );
        let brief = Node::relayed(&task.session_id, None, Origin::Agent, &author, &text);
        let mut records = vec![NewRecord::json(
            theseus_store::kinds::SESSION,
            Some(&rec.session_id),
            &rec,
        )?];
        records.push(brief.record()?);
        records.push(arrangement.record()?);
        let pieces = self
            .pieces
            .iter()
            .map(|p| (&p.node, crate::graph::VIA_ARRANGEMENT));
        let claim = self
            .claim
            .iter()
            .map(|c| (&c.node, crate::graph::VIA_CLAIM));
        for (node, via) in pieces.chain(claim) {
            records.push(
                crate::graph::Edge::new(
                    crate::graph::EdgeKind::DerivedFrom,
                    &arrangement.id,
                    node,
                    via,
                )
                .record()?,
            );
        }
        if let Some(t) = &task_of.target {
            records.push(core.outbox.task_record(&task.session_id, t)?);
        }
        records.extend(self.graph_record(task)?);
        records.push(fact::row(
            &fact::retry::TaskRetried {
                failed_execution: &failed.id,
                failed_session: &failed.session_id,
                execution_id: &task.id,
                session_id: &task.session_id,
                limit: task.budget.limit_micros,
                failed_limit: failed.budget.limit_micros,
                note,
                by,
            },
            Some(&task.session_id),
            None,
        )?);
        Ok(records)
    }
}

/// What the retry's line begins with, after a blank line.
const RETRY_LINE: &str = "\n\n[This is a retry of task";

/// The retry's brief: the failed task's own words, and a line saying it is a
/// retry, why the last try ended, and what the owner added.
fn retry_brief(
    task_session: &str,
    parent_session: &str,
    brief: &str,
    failed_session: &str,
    why: Option<&str>,
    note: Option<&str>,
) -> String {
    // A retry of a retry reads the first brief, not the last one's note.
    let mut words = brief
        .split(RETRY_LINE)
        .next()
        .unwrap_or(brief)
        .trim_end()
        .to_string();
    words.push_str(&format!(
        "{RETRY_LINE} {}, which ended without finishing{}.{}]",
        crate::task::short(failed_session),
        why.filter(|w| !w.is_empty())
            .map(|w| format!(": {w}"))
            .unwrap_or_default(),
        note.map(|n| format!(" The owner adds: {n}"))
            .unwrap_or_default()
    ));
    crate::task::brief_text(task_session, parent_session, &words)
}

/// A refusal of the owner's act stays one; any other error is the params'.
fn failure(e: anyhow::Error) -> RpcFailure {
    match e.downcast::<Refusal>() {
        Ok(r) => RpcFailure {
            code: error_code::REFUSED,
            message: format!(
                "from {} through {}: {}. The failure is as it was.",
                r.who, r.via, r.why
            ),
            data: json!({"who": r.who, "via": r.via, "why": r.why}),
        },
        Err(e) => RpcFailure::invalid(e),
    }
}
