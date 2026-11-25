//! The owner's verbs on projects (theseus-n5ui2): `project.create`,
//! `project.list`, `project.show` and `project.attach`. Making one is the
//! owner's act, like a routine's (`judge_act(Act::Routine)`: it makes a
//! routine, its weekly review); attaching a conversation is the owner's like
//! any change to a session (`Act::Session`). Each write is one frame. A
//! milestone's reminders are set in Home right after (`project::remind`).

use std::sync::Arc;

use anyhow::Result;
use serde_json::{json, Value};
use theseus_protocol::projects::{
    ProjectAttachParams, ProjectAttachResult, ProjectCloseParams, ProjectCreateParams,
    ProjectCreateResult, ProjectInfo, ProjectItemAddParams, ProjectItemCloseParams,
    ProjectListParams, ProjectListResult, ProjectShowParams, ProjectShowResult, ProjectTask,
    PROJECT_TASKS_LISTED,
};
use theseus_protocol::tasks::{TaskOrigin, TaskRecord};
use theseus_protocol::{error_code, method};

use super::confirms::Act;
use super::server::{route, Conn, RpcFailure};
use super::Core;
use crate::approval::{Answerer, Refusal};
use crate::project::{self, create::NewProject, remind};

/// The projects' methods, routed by `rpc_prefixed`.
pub(super) const OWN: [&str; 8] = [
    method::PROJECT_CREATE,
    method::PROJECT_LIST,
    method::PROJECT_SHOW,
    method::PROJECT_ATTACH,
    method::PROJECT_ITEM_ADD,
    method::PROJECT_ITEM_CLOSE,
    method::PROJECT_ITEM_LIST,
    method::PROJECT_CLOSE,
];

fn invalid(e: impl std::fmt::Display) -> RpcFailure {
    RpcFailure::new(error_code::INVALID_PARAMS, e.to_string())
}

pub(super) fn failure(e: anyhow::Error) -> RpcFailure {
    match e.downcast::<Refusal>() {
        Ok(r) => RpcFailure {
            code: error_code::REFUSED,
            message: format!(
                "a project request from {} does not count: {}. Projects are the owner's, from a \
                 private place.",
                r.who, r.why
            ),
            data: json!({"who": r.who, "via": r.via, "why": r.why}),
        },
        Err(e) => RpcFailure::invalid(e),
    }
}

/// One project as `look.work` reads it: what `project.list` shows, and whose words its title and its next
/// milestone's title are.
pub struct ProjectRow {
    pub info: ProjectInfo,
    pub title: crate::content::Hand,
    pub next: Option<crate::content::Hand>,
}

/// What `Core::projects_upto` reads: the projects, and how many older open ones were left unread.
pub type ProjectsRead = (Vec<ProjectRow>, usize);

impl Core {
    /// The projects' methods, by name.
    pub(super) async fn rpc_projects(
        self: &Arc<Self>,
        name: &str,
        params: Value,
        conn: Conn<'_>,
    ) -> Result<Value, RpcFailure> {
        match name {
            method::PROJECT_CREATE => route(params, |p: ProjectCreateParams| {
                let who = conn.answerer(p.author.clone(), p.discord.clone());
                self.project_create(&p, who)
            }),
            method::PROJECT_LIST => route(params, |p| self.project_list(&p)),
            method::PROJECT_SHOW => route(params, |p| self.project_show(&p)),
            method::PROJECT_ITEM_ADD => route(params, |p: ProjectItemAddParams| {
                let who = conn.answerer(p.author.clone(), p.discord.clone());
                self.project_item_add(&p, who)
            }),
            method::PROJECT_ITEM_CLOSE => route(params, |p: ProjectItemCloseParams| {
                let who = conn.answerer(p.author.clone(), p.discord.clone());
                self.project_item_close(&p, who)
            }),
            method::PROJECT_ITEM_LIST => route(params, |p| self.project_item_list(&p)),
            method::PROJECT_CLOSE => {
                let p: ProjectCloseParams = super::server::parse(params)?;
                let who = conn.answerer(p.author.clone(), p.discord.clone());
                super::server::reply(self.project_close(&p, who).await?)
            }
            _ => route(params, |p: ProjectAttachParams| {
                let who = conn.answerer(p.author.clone(), p.discord.clone());
                self.project_attach(&p, who)
            }),
        }
    }

    pub(super) fn project_info(&self, tasks: &[TaskRecord], p: &TaskRecord) -> ProjectInfo {
        let env = crate::routine::Env {
            kernel: &self.kernel,
            store: &self.store,
            outbox: &self.outbox,
            target: None,
        };
        let book = &self.runner.tools.routines;
        let next = |id: &str| {
            let spec = book.find(&self.store, id).ok()?;
            crate::routine::info_of(&env, &spec).ok()?.next_ms
        };
        let mut info = project::info::info(tasks, p, self.kernel.now_ms(), &next);
        let involved = self.project_involved(tasks, p);
        info.tasks = involved.len() as u32;
        info.tasks_running = involved.iter().filter(|t| !t.ended).count() as u32;
        info
    }

    /// The tasks involved in `p` (`project::involved`): those still going first, then the newest first.
    pub(crate) fn project_involved(
        &self,
        tasks: &[TaskRecord],
        p: &TaskRecord,
    ) -> Vec<ProjectTask> {
        let mut tied = project::involved::tied(tasks, p, self.kernel.now_ms());
        // A review's runs are the tasks of its routine's execution; the routine's record outlives its delete.
        let routine = p.review.as_deref().and_then(|id| {
            self.store
                .get_meta::<theseus_protocol::routines::RoutineSpec>(&format!(
                    "{}{id}",
                    theseus_protocol::routines::KEY_PREFIX
                ))
                .ok()
                .flatten()
        });
        if let Some(spec) = routine {
            for e in self
                .kernel
                .tasks(Some(&spec.execution_id))
                .unwrap_or_default()
            {
                if !tied.iter().any(|(s, _)| *s == e.session_id) {
                    tied.push((e.session_id.clone(), project::involved::REVIEW));
                }
            }
        }
        let mut out: Vec<ProjectTask> = tied
            .into_iter()
            .filter_map(|(session, how)| self.project_task(tasks, &session, how))
            .collect();
        project::involved::order(&mut out);
        out
    }

    /// One involved task as it is listed: none for a session that is not a task or has no execution.
    fn project_task(&self, tasks: &[TaskRecord], session: &str, how: &str) -> Option<ProjectTask> {
        let tail = session.split_once('_').map_or(session, |(_, t)| t);
        let e = self
            .kernel
            .execution(&format!("exe_{tail}"))
            .ok()
            .flatten()?;
        let rec: Option<crate::session::SessionRecord> =
            self.store.get_session(session).ok().flatten();
        rec.as_ref()?.task.as_ref()?;
        let title = rec
            .as_ref()
            .and_then(|r| r.title.clone())
            .or_else(|| {
                tasks
                    .iter()
                    .find(|t| t.session.as_deref() == Some(session))
                    .map(|t| t.title.clone())
            })
            .unwrap_or_default();
        let ended = e.state.is_terminal();
        Some(ProjectTask {
            task_id: session.to_string(),
            short: crate::task::short(session),
            title,
            state: e.state.as_str().to_string(),
            ended,
            how: how.to_string(),
            started_at_ms: e.created_at_ms,
            ended_at_ms: ended.then_some(e.updated_at_ms),
        })
    }

    /// `project.create`: the project, its milestones, and (unless
    /// `review: "none"`) its weekly review routine.
    pub fn project_create(
        &self,
        p: &ProjectCreateParams,
        who: impl Into<Answerer>,
    ) -> Result<ProjectCreateResult, RpcFailure> {
        let who = who.into();
        let title = p.title.trim();
        if title.is_empty() || title.chars().count() > 200 || title.contains('\n') {
            return Err(invalid(
                "a project's title is one line of at most 200 characters",
            ));
        }
        self.judge_act(
            &who,
            Act::Routine {
                method: method::PROJECT_CREATE,
                what: &format!("the project {title}"),
            },
        )
        .map_err(failure)?;
        let cfg = &self.runner.tools.projects;
        let due = project::create::dates(&self.kernel, &p.milestones, cfg.max_milestones)
            .map_err(invalid)?;
        let home = self.home().map_err(RpcFailure::from)?;
        let id = project::create::project_id(&theseus_kernel::new_id("act"));
        let mut review = None;
        if p.review.as_deref().map(str::trim) != Some("none") {
            let new =
                project::review::request(cfg, (&id, title), p.review.as_deref(), p.review_usd);
            let spec = project::review::make(
                &self.kernel,
                &self.store,
                &self.runner.tools.routines,
                &new,
                (&who.label, None, 0),
            )
            .map_err(invalid)?;
            review = Some(spec.session_id);
        }
        let origin = TaskOrigin {
            session: home.session_id.clone(),
            principal: "operator".into(),
            by_model: false,
            ..Default::default()
        };
        let now = self.kernel.now_ms();
        let records = project::create::records(
            &NewProject {
                id,
                title,
                objective: p.objective.as_deref(),
                acceptance: &p.acceptance,
                milestones: &p.milestones,
                due_at_ms: None,
                origin,
                review,
            },
            &due,
            now,
        );
        let rec = self.session_rec(&home.session_id);
        let mut frame = Vec::new();
        let mut changes = Vec::new();
        for t in &records {
            let c = crate::fact::task_graph::Change {
                session_id: home.session_id.clone(),
                task: t.clone(),
                from: None,
                detail: json!({"pieces": 0}),
            };
            frame.push(crate::task_graph::record(t)?);
            frame.push(crate::fact::task_graph::row_of(&rec, "created", &c)?);
            changes.push(c);
        }
        self.store.append(&frame)?;
        for c in &changes {
            crate::fact::task_graph::announce(&rec, "created", c);
        }
        self.sync_reminders(&records[0], &who.label);
        let tasks = crate::task_graph::all(&self.store)?;
        let project = self.project_info(&tasks, &records[0]);
        Ok(ProjectCreateResult { project, records })
    }

    /// Set, move or cancel the reminders of `changed`'s milestones, in Home.
    pub(crate) fn sync_reminders(&self, changed: &TaskRecord, by: &str) {
        self.sync_reminders_counted(changed, by);
    }

    /// The same, saying what it did: none when it could not run.
    pub(crate) fn sync_reminders_counted(
        &self,
        changed: &TaskRecord,
        by: &str,
    ) -> Option<remind::Synced> {
        let home = match self.home() {
            Ok(h) => h,
            Err(e) => {
                tracing::warn!(error = %format!("{e:#}"), "Home was not made for a milestone's reminder");
                return None;
            }
        };
        let place = remind::Place {
            execution_id: home.execution_id,
            target: home.target,
        };
        let grace = self.runner.tools.projects.overdue_after_ms();
        match remind::sync_after(&self.kernel, &self.store, changed, (&place, by), grace) {
            Ok(done) => Some(done),
            Err(e) => {
                tracing::warn!(error = %format!("{e:#}"), task = %changed.id, "a milestone's reminder was not synced");
                None
            }
        }
    }

    /// `project.list`: the open projects, or all with `all`.
    pub fn project_list(&self, p: &ProjectListParams) -> Result<ProjectListResult, RpcFailure> {
        let tasks = crate::task_graph::all(&self.store)?;
        let projects = tasks
            .iter()
            .filter(|t| project::is_project(t) && (p.all || !t.state.is_closed()))
            .map(|t| self.project_info(&tasks, t))
            .collect();
        Ok(ProjectListResult { projects })
    }

    /// The newest `limit` open projects as `look.work` reads them, in `project.list`'s order, and how
    /// many older open ones were left unread. Each carries whose words its title is, and whose its next
    /// milestone's is (`task_graph::title_hands`: the owner's, or the session of the model that wrote or
    /// last retitled it). The records are decoded once, as `project.list` does; what is limited is the
    /// building of each project's progress, which walks the records again for every project.
    pub fn projects_upto(&self, limit: usize) -> Result<ProjectsRead, RpcFailure> {
        let tasks = crate::task_graph::all(&self.store)?;
        let open: Vec<&TaskRecord> = tasks
            .iter()
            .filter(|t| project::is_project(t) && !t.state.is_closed())
            .collect();
        let older = open.len().saturating_sub(limit);
        let picked: Vec<(&TaskRecord, Option<&TaskRecord>)> = open
            .into_iter()
            .skip(older)
            .map(|t| {
                let next = project::progress::milestones(&tasks, &t.id)
                    .into_iter()
                    .find(|m| !m.state.is_closed());
                (t, next)
            })
            .collect();
        // One bounded read of the ledger for every title shown (`task_graph::retitle`).
        let recs: Vec<&TaskRecord> = picked
            .iter()
            .flat_map(|(t, m)| std::iter::once(*t).chain(*m))
            .collect();
        let mut hands = crate::task_graph::title_hands(&self.store, &recs)?.into_iter();
        let short = || RpcFailure::invalid(anyhow::anyhow!("a title was left without its hand"));
        let mut shown = Vec::new();
        for (t, m) in &picked {
            let title = hands.next().ok_or_else(short)?;
            let next = match m {
                Some(_) => Some(hands.next().ok_or_else(short)?),
                None => None,
            };
            shown.push(ProjectRow {
                info: self.project_info(&tasks, t),
                title,
                next,
            });
        }
        Ok((shown, older))
    }

    pub(super) fn project_named<'a>(
        tasks: &'a [TaskRecord],
        name: &str,
    ) -> Result<&'a TaskRecord, RpcFailure> {
        let t = crate::task_graph::resolve(tasks, name)
            .map_err(|e| RpcFailure::new(error_code::NOT_FOUND, e))?;
        if !project::is_project(t) {
            return Err(invalid(format!(
                "task {} is a {}, not a project",
                t.id,
                t.kind.as_str()
            )));
        }
        Ok(t)
    }

    /// `project.show`: the project and everything under it, depth first.
    pub fn project_show(&self, p: &ProjectShowParams) -> Result<ProjectShowResult, RpcFailure> {
        let tasks = crate::task_graph::all_shown(&self.store, &self.kernel)?;
        let t = Self::project_named(&tasks, &p.id)?;
        let records = crate::task_graph::subtree(&tasks, &t.id)
            .into_iter()
            .cloned()
            .collect();
        let (involved, more) =
            project::involved::bounded(self.project_involved(&tasks, t), PROJECT_TASKS_LISTED);
        Ok(ProjectShowResult {
            project: self.project_info(&tasks, t),
            records,
            tasks: involved,
            tasks_more: more,
        })
    }

    /// `project.attach`: the session carries the project's slice from its
    /// next turn; without `project`, it detaches.
    pub fn project_attach(
        &self,
        p: &ProjectAttachParams,
        who: impl Into<Answerer>,
    ) -> Result<ProjectAttachResult, RpcFailure> {
        let who = who.into();
        let rec = self.session(&p.session)?;
        self.judge_act(
            &who,
            Act::Session {
                method: method::PROJECT_ATTACH,
                what: &format!("the project of {}", rec.session_id),
            },
        )
        .map_err(failure)?;
        let id = match p.project.as_deref().map(str::trim) {
            None | Some("") | Some("none") => None,
            Some(name) => {
                let tasks = crate::task_graph::all(&self.store)?;
                Some(Self::project_named(&tasks, name)?.id.clone())
            }
        };
        self.store
            .update_session(&rec.session_id, |r| {
                r.project.clone_from(&id);
                Ok(vec![])
            })?
            .ok_or_else(|| RpcFailure::new(error_code::NOT_FOUND, "no such session"))?;
        Ok(ProjectAttachResult {
            session: rec.session_id,
            project: id,
        })
    }
}
