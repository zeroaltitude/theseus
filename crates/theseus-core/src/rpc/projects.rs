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
    ProjectAttachParams, ProjectAttachResult, ProjectCreateParams, ProjectCreateResult,
    ProjectInfo, ProjectListParams, ProjectListResult, ProjectShowParams, ProjectShowResult,
};
use theseus_protocol::tasks::{TaskOrigin, TaskRecord};
use theseus_protocol::{error_code, method};

use super::confirms::Act;
use super::server::{route, Conn, RpcFailure};
use super::Core;
use crate::approval::{Answerer, Refusal};
use crate::project::{self, create::NewProject, remind};

/// The projects' methods, routed by `rpc_prefixed`.
pub(super) const OWN: [&str; 4] = [
    method::PROJECT_CREATE,
    method::PROJECT_LIST,
    method::PROJECT_SHOW,
    method::PROJECT_ATTACH,
];

fn invalid(e: impl std::fmt::Display) -> RpcFailure {
    RpcFailure::new(error_code::INVALID_PARAMS, e.to_string())
}

fn failure(e: anyhow::Error) -> RpcFailure {
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

impl Core {
    /// The projects' methods, by name.
    pub(super) fn rpc_projects(
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
            _ => route(params, |p: ProjectAttachParams| {
                let who = conn.answerer(p.author.clone(), p.discord.clone());
                self.project_attach(&p, who)
            }),
        }
    }

    fn project_info(&self, tasks: &[TaskRecord], p: &TaskRecord) -> ProjectInfo {
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
        project::info::info(tasks, p, self.kernel.now_ms(), &next)
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
                (&who.label, None),
            )
            .map_err(invalid)?;
            review = Some(spec.session_id);
        }
        let origin = TaskOrigin {
            session: home.session_id.clone(),
            principal: "operator".into(),
            by_model: false,
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
        let home = match self.home() {
            Ok(h) => h,
            Err(e) => {
                tracing::warn!(error = %format!("{e:#}"), "Home was not made for a milestone's reminder");
                return;
            }
        };
        let place = remind::Place {
            execution_id: home.execution_id,
            target: home.target,
        };
        let grace = self.runner.tools.projects.overdue_after_ms();
        if let Err(e) = remind::sync_after(&self.kernel, &self.store, changed, (&place, by), grace)
        {
            tracing::warn!(error = %format!("{e:#}"), task = %changed.id, "a milestone's reminder was not synced");
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

    fn project_named<'a>(
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
        Ok(ProjectShowResult {
            project: self.project_info(&tasks, t),
            records,
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
