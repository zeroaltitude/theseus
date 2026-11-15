//! A project's items on the wire (issue 13): `project.item.add`,
//! `project.item.close` and `project.item.list`, thin verbs over the task
//! records (`project::items`). Adding and closing are judged as the project's
//! other session-level acts are (the owner's, from a private place); the list
//! is a read. Each write is one frame: the record and its row.

use anyhow::Result;
use serde_json::json;
use theseus_protocol::projects::{
    ProjectItemAddParams, ProjectItemCloseParams, ProjectItemListParams, ProjectItemsResult,
};
use theseus_protocol::tasks::{TaskOrigin, TaskRecord, TaskState};
use theseus_protocol::{error_code, method};

use super::confirms::Act;
use super::projects::failure;
use super::server::RpcFailure;
use super::Core;
use crate::approval::Answerer;
use crate::project::items;
use crate::task_graph::{self as graph, NewTask};

fn invalid(e: impl std::fmt::Display) -> RpcFailure {
    RpcFailure::new(error_code::INVALID_PARAMS, e.to_string())
}

/// The project `name` means: its id or an end of it, else the one open
/// project with that title (case aside).
fn project_by<'a>(tasks: &'a [TaskRecord], name: &str) -> Result<&'a TaskRecord, RpcFailure> {
    match Core::project_named(tasks, name) {
        Ok(p) => Ok(p),
        Err(first) => {
            let want = name.trim().to_lowercase();
            let mut found = tasks.iter().filter(|t| {
                crate::project::is_project(t)
                    && !t.state.is_closed()
                    && t.title.trim().to_lowercase() == want
            });
            match (found.next(), found.next()) {
                (Some(p), None) => Ok(p),
                (Some(_), Some(_)) => Err(invalid(format!(
                    "more than one open project is titled {name:?}: name it by its id"
                ))),
                _ => Err(first),
            }
        }
    }
}

impl Core {
    /// `project.item.add`: a step on the project's list.
    pub fn project_item_add(
        &self,
        p: &ProjectItemAddParams,
        who: impl Into<Answerer>,
    ) -> Result<ProjectItemsResult, RpcFailure> {
        let who = who.into();
        let title = items::title(&p.title).map_err(invalid)?;
        self.judge_act(
            &who,
            Act::Session {
                method: method::PROJECT_ITEM_ADD,
                what: &format!("an item of the project {}", p.project),
            },
        )
        .map_err(failure)?;
        let tasks = graph::all(&self.store)?;
        let project = project_by(&tasks, &p.project)?;
        if project.state.is_closed() {
            return Err(invalid(format!(
                "the project {} is {}: nothing is added to a closed project",
                project.title,
                project.state.as_str()
            )));
        }
        let home = self.home().map_err(RpcFailure::from)?;
        let origin = TaskOrigin {
            session: home.session_id.clone(),
            principal: "operator".into(),
            by_model: false,
            ..Default::default()
        };
        let item = NewTask {
            id: graph::id_from(&theseus_kernel::new_id("act")),
            title: &title,
            objective: title.clone(),
            acceptance: vec![],
            parent: Some(project.id.clone()),
            deps: vec![],
            session: None,
            origin,
            state: TaskState::Accepted,
        }
        .build(self.kernel.now_ms());
        let rec = self.session_rec(&home.session_id);
        let change = crate::fact::task_graph::Change {
            session_id: home.session_id.clone(),
            task: item.clone(),
            from: None,
            detail: json!({"pieces": 0}),
        };
        self.store.append(&[
            graph::record(&item)?,
            crate::fact::task_graph::row_of(&rec, "created", &change)?,
        ])?;
        crate::fact::task_graph::announce(&rec, "created", &change);
        self.items_result(&project.id, Some(item))
    }

    /// `project.item.close`: an item done.
    pub fn project_item_close(
        &self,
        p: &ProjectItemCloseParams,
        who: impl Into<Answerer>,
    ) -> Result<ProjectItemsResult, RpcFailure> {
        let who = who.into();
        self.judge_act(
            &who,
            Act::Session {
                method: method::PROJECT_ITEM_CLOSE,
                what: &format!("closing the item {}", p.item),
            },
        )
        .map_err(failure)?;
        let tasks = graph::all(&self.store)?;
        let found =
            items::named(&tasks, &p.item).map_err(|e| RpcFailure::new(error_code::NOT_FOUND, e))?;
        let project = crate::project::project_of(&tasks, found)
            .map(|t| t.id.clone())
            .unwrap_or_default();
        let id = found.id.clone();
        let home = self.home().map_err(RpcFailure::from)?;
        let _lock = self.store.lock_task(&id);
        let mut item = graph::get(&self.store, &id)?
            .ok_or_else(|| RpcFailure::new(error_code::NOT_FOUND, "no such item"))?;
        if item.state.is_closed() {
            return Err(invalid(format!(
                "the item {} is closed already ({})",
                item.title,
                item.state.as_str()
            )));
        }
        let from = item.version;
        item.state = TaskState::Done;
        item.proposal = None;
        item.claim = None;
        item.version += 1;
        item.updated_at_ms = self.kernel.now_ms();
        let rec = self.session_rec(&home.session_id);
        let change = crate::fact::task_graph::Change {
            session_id: home.session_id.clone(),
            task: item.clone(),
            from: Some(from),
            detail: json!({"added": 0, "evidence": item.evidence}),
        };
        self.store.append(&[
            graph::record(&item)?,
            crate::fact::task_graph::row_of(&rec, "closed", &change)?,
        ])?;
        crate::fact::task_graph::announce(&rec, "closed", &change);
        self.items_result(&project, Some(item))
    }

    /// `project.item.list`: the project's open items, or all with `all`.
    pub fn project_item_list(
        &self,
        p: &ProjectItemListParams,
    ) -> Result<ProjectItemsResult, RpcFailure> {
        let tasks = graph::all(&self.store)?;
        let project = project_by(&tasks, &p.project)?;
        Ok(ProjectItemsResult {
            project: project.id.clone(),
            title: project.title.clone(),
            items: items::list(&tasks, &project.id, p.all)
                .into_iter()
                .cloned()
                .collect(),
            changed: None,
        })
    }

    /// The open list as it now stands, and the item that changed.
    fn items_result(
        &self,
        project: &str,
        changed: Option<TaskRecord>,
    ) -> Result<ProjectItemsResult, RpcFailure> {
        let tasks = graph::all(&self.store)?;
        let title = tasks
            .iter()
            .find(|t| t.id == project)
            .map(|t| t.title.clone())
            .unwrap_or_default();
        Ok(ProjectItemsResult {
            project: project.to_string(),
            title,
            items: items::list(&tasks, project, false)
                .into_iter()
                .cloned()
                .collect(),
            changed,
        })
    }
}
