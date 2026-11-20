//! `project.close` (issue 37): the owner's end of a project. It ends the project record and its open
//! items and milestones in one frame, each as `done` or `abandoned` (the states a model's own close
//! writes), and stops what the project set going: its weekly review routine (the routine's own delete,
//! which ends any run going) and its milestones' reminders (`project::remind`, which cancels the wakes
//! of a closed project's milestones).
//!
//! Judged as `project.create` is, the owner's from a private place (`Act::Routine`: the act deletes a
//! routine), and left out of the MCP server's lists. It is refused for a project closed already, for the
//! self loop's container (which is no project), and while a task carries the project or one of what it
//! holds: it never stops running work. The review is stopped before the frame is written, so a failure
//! leaves an open project with no review, which the same call finishes; the refusals come first.
//! There is no reopen.

use serde_json::json;
use theseus_protocol::method;
use theseus_protocol::projects::{ProjectCloseParams, ProjectCloseResult, ProjectEnd};
use theseus_protocol::routines::RoutineNameParams;
use theseus_protocol::tasks::{TaskClaim, TaskEvidence, TaskKind, TaskRecord};

use super::confirms::Act;
use super::projects::failure;
use super::server::RpcFailure;
use super::Core;
use crate::approval::Answerer;
use crate::task_graph as graph;

/// A reason is one short line.
const MAX_REASON: usize = 200;

fn invalid(e: impl std::fmt::Display) -> RpcFailure {
    RpcFailure::new(theseus_protocol::error_code::INVALID_PARAMS, e.to_string())
}

/// How people name a task: the last six characters of its id.
fn short(id: &str) -> &str {
    &id[id.len().saturating_sub(6)..]
}

/// What stands in the way of closing `project` among `tasks` (read as the board reads them): a task that runs
/// the work of a record under it, or that holds a claim on one, named with the command that stops it. `live`
/// says whether a record is a running task's own, `held` whether a claim's holder still runs. None when the
/// project may end. A task that was cancelled or ran out of budget carries nothing, though its record stays open
/// to be resumed, and neither does a claim whose holder has ended, though its lease has not lapsed.
pub(crate) fn carried_by(
    tasks: &[TaskRecord],
    project: &TaskRecord,
    live: &dyn Fn(&TaskRecord) -> bool,
    held: &dyn Fn(&TaskClaim) -> bool,
) -> Option<String> {
    let under = graph::subtree(tasks, &project.id);
    let under = || {
        under
            .iter()
            .filter(|t| t.id != project.id && !t.state.is_closed())
    };
    let task_words = |id: &str, title: &str| {
        let named = if title.is_empty() {
            format!("task {id}")
        } else {
            format!("task {id} (\"{title}\")")
        };
        format!(
            "{named} still carries part of this project: cancel it (`theseus cancel {id}`) or let \
             it finish first, since closing a project does not stop a task that carries part of it"
        )
    };
    if let Some(t) = under().find(|t| live(t)) {
        return Some(task_words(short(&t.id), &t.title));
    }
    let c = under().find_map(|t| t.claim.as_ref().filter(|c| held(c)))?;
    // The holder of a claim is a task, which `theseus cancel` stops, or a conversation, which it does not
    // know: nothing of the owner's releases a claim, so the answer for a conversation is when it frees.
    Some(
        match tasks
            .iter()
            .find(|t| t.session.as_deref() == Some(c.session.as_str()))
        {
            Some(holder) => task_words(short(&c.session), &holder.title),
            None => format!(
                "a conversation (…{}) holds a claim on a step of this project until {}: close the \
                 project after that, since closing a project does not stop a conversation that holds part of it",
                short(&c.session),
                crate::push::hm(c.until_ms)
            ),
        },
    )
}

impl Core {
    /// `project.close`.
    pub async fn project_close(
        &self,
        p: &ProjectCloseParams,
        who: impl Into<Answerer>,
    ) -> Result<ProjectCloseResult, RpcFailure> {
        let who = who.into();
        let reason = match p.reason.as_deref().map(str::trim).filter(|r| !r.is_empty()) {
            Some(r) if r.chars().count() > MAX_REASON || r.contains('\n') => {
                return Err(invalid(format!(
                    "a reason is one line of at most {MAX_REASON} characters"
                )))
            }
            r => r.map(str::to_string),
        };
        self.judge_act(
            &who,
            Act::Routine {
                method: method::PROJECT_CLOSE,
                what: &format!("the close of the project {}", p.project),
            },
        )
        .map_err(failure)?;
        let tasks = graph::all_shown(&self.store, &self.kernel)?;
        let found = graph::resolve(&tasks, &p.project)
            .map_err(|e| RpcFailure::new(theseus_protocol::error_code::NOT_FOUND, e))?;
        if crate::rsi::backlog::container(&self.store).as_deref() == Some(found.id.as_str()) {
            return Err(invalid(
                "that record is the self-improvement backlog's own list, not a project: its items \
                 are managed with `theseus self backlog`, `theseus self drop` and \
                 `theseus self set-aside`",
            ));
        }
        let project = Self::project_named(&tasks, &p.project)?.clone();
        if project.state.is_closed() {
            return Err(invalid(format!(
                "the project is {} already: there is nothing to close",
                project.state.as_str()
            )));
        }
        let now = self.kernel.now_ms();
        if let Some(why) = carried_by(&tasks, &project, &|t| self.runs(t), &|c| self.holds(c)) {
            return Err(invalid(why));
        }
        // The review first: with the refusals past, nothing below can refuse.
        let review_stopped = self.review_stopped(&project, &who).await?;
        // The records, locked in id order, read again under their locks.
        let mut ids: Vec<String> = graph::subtree(&tasks, &project.id)
            .into_iter()
            .map(|t| t.id.clone())
            .collect();
        ids.sort();
        let _locks: Vec<_> = ids.iter().map(|id| self.store.lock_task(id)).collect();
        let ended = self.ending(&project, &ids, (p.how, reason.as_deref()), &who, now)?;
        self.write_closed(&ended, &project, p.how, &who)?;
        let reminders_cancelled = self
            .sync_reminders_counted(&ended_project(&ended, &project), &who.label)
            .map_or(0, |s| s.cancelled);
        let count = |kind: TaskKind| {
            ended
                .iter()
                .filter(|(t, _)| t.id != project.id && t.kind == kind)
                .count() as u32
        };
        let tasks = graph::all(&self.store)?;
        let info = ended_project(&ended, &project);
        Ok(ProjectCloseResult {
            project: self.project_info(&tasks, &info),
            how: p.how,
            items_ended: count(TaskKind::Item),
            milestones_ended: count(TaskKind::Milestone),
            review_stopped,
            reminders_cancelled,
        })
    }

    /// The records of `ids` (the project's subtree, locked by the caller) as the close leaves them, each with the
    /// version it had; one a task took hold of meanwhile stops the close.
    fn ending(
        &self,
        project: &TaskRecord,
        ids: &[String],
        (how, reason): (ProjectEnd, Option<&str>),
        who: &Answerer,
        now: u64,
    ) -> Result<Vec<(TaskRecord, u64)>, RpcFailure> {
        let mut ended = Vec::new();
        for id in ids {
            let Some(mut t) = graph::get(&self.store, id)? else {
                continue;
            };
            if t.state.is_closed() {
                continue;
            }
            if t.id != project.id
                && (self.runs(&t)
                    || t.claim
                        .as_ref()
                        .is_some_and(|c| c.until_ms > now && self.holds(c)))
            {
                return Err(invalid(
                    "a task took hold of this project while it was being closed: try again",
                ));
            }
            let from = t.version;
            t.state = how.state();
            t.proposal = None;
            t.claim = None;
            t.version += 1;
            t.updated_at_ms = now;
            if t.id == project.id {
                if let Some(r) = reason {
                    t.evidence.push(TaskEvidence {
                        node: None,
                        identity: format!("closed {}", how.word()),
                        note: Some(r.to_string()),
                        by: who.label.clone(),
                        at_ms: now,
                    });
                }
            }
            ended.push((t, from));
        }
        Ok(ended)
    }

    /// The close's one frame: each record and its `task.closed` row, then the notifications.
    fn write_closed(
        &self,
        ended: &[(TaskRecord, u64)],
        project: &TaskRecord,
        how: ProjectEnd,
        who: &Answerer,
    ) -> Result<(), RpcFailure> {
        let home = self.home().map_err(RpcFailure::from)?;
        let rec = self.session_rec(&home.session_id);
        let mut frame = Vec::new();
        let mut changes = Vec::new();
        for (t, from) in ended {
            let c = crate::fact::task_graph::Change {
                session_id: home.session_id.clone(),
                task: t.clone(),
                from: Some(*from),
                detail: json!({
                    "added": t.evidence.len(),
                    "evidence": t.evidence,
                    "by": who.who(),
                    "via": who.via(),
                    "how": how.word(),
                    "with_project": project.id,
                }),
            };
            frame.push(graph::record(t)?);
            frame.push(crate::fact::task_graph::row_of(&rec, "closed", &c)?);
            changes.push(c);
        }
        self.store.append(&frame)?;
        for c in &changes {
            crate::fact::task_graph::announce(&rec, "closed", c);
        }
        Ok(())
    }

    /// Whether a task's execution is still alive: it runs, waits or is blocked.
    fn runs(&self, t: &TaskRecord) -> bool {
        graph::execution_of(t)
            .and_then(|id| self.kernel.execution(&id).ok().flatten())
            .is_some_and(|e| !e.state.is_terminal())
    }

    /// Whether a claim's holder, an execution, is still alive.
    fn holds(&self, c: &TaskClaim) -> bool {
        self.kernel
            .execution(&c.by)
            .ok()
            .flatten()
            .is_some_and(|e| !e.state.is_terminal())
    }

    /// End the project's live weekly review by the routine's own delete; its id, or none when the project
    /// has no live one.
    async fn review_stopped(
        &self,
        project: &TaskRecord,
        who: &Answerer,
    ) -> Result<Option<String>, RpcFailure> {
        let Some(id) = project.review.as_deref() else {
            return Ok(None);
        };
        let Ok(spec) = self.runner.tools.routines.find(&self.store, id) else {
            return Ok(None);
        };
        let stopped = spec.session_id.clone();
        self.routine_delete(
            &RoutineNameParams {
                id: stopped.clone(),
                author: None,
                discord: None,
            },
            who.clone(),
        )
        .await?;
        Ok(Some(stopped))
    }
}

/// The project's record as the close wrote it.
fn ended_project(ended: &[(TaskRecord, u64)], project: &TaskRecord) -> TaskRecord {
    ended
        .iter()
        .find(|(t, _)| t.id == project.id)
        .map_or_else(|| project.clone(), |(t, _)| t.clone())
}
