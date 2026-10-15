//! A project and its milestones as records (theseus-n5ui2): the project and
//! each milestone are TASK records written in one frame, so a project is never
//! seen half made. Used by the owner's `project.create` and the model's
//! `task_create { kind: "project" }`.
//!
//! Ids are deterministic from the call's: the project's is `id_from` the
//! correlation id, a milestone's that and `m<n>`, so a call run again finds
//! what it made.

use theseus_protocol::projects::MilestoneSpec;
use theseus_protocol::tasks::{TaskKind, TaskOrigin, TaskRecord, TaskState};

use crate::task_graph::{id_from, NewTask};
use crate::wake_when::parse_when;

/// What a project asks for.
pub struct NewProject<'a> {
    pub id: String,
    pub title: &'a str,
    pub objective: Option<&'a str>,
    pub acceptance: &'a [String],
    pub milestones: &'a [MilestoneSpec],
    pub due_at_ms: Option<u64>,
    pub origin: TaskOrigin,
    pub review: Option<String>,
}

/// A milestone's id.
pub fn milestone_id(project: &str, n: usize) -> String {
    format!("{project}m{n}")
}

/// The project's id for the call `correlation_id`.
pub fn project_id(correlation_id: &str) -> String {
    id_from(correlation_id)
}

/// Read a milestone's date: `when` words, in the daemon's zone.
pub fn due_of(kernel: &theseus_kernel::Kernel, when: &str, what: &str) -> Result<u64, String> {
    let kc = kernel.config();
    parse_when(when, kernel.now_ms(), &kc.zone, kc.wakes.max_ahead_ms)
        .map_err(|e| format!("{what}: {e}"))
}

/// The records: the project first, then its milestones, in the order given.
/// `due` holds each milestone's date, in step with `milestones`.
pub fn records(p: &NewProject<'_>, due: &[Option<u64>], now_ms: u64) -> Vec<TaskRecord> {
    let objective = p
        .objective
        .map(str::trim)
        .filter(|o| !o.is_empty())
        .unwrap_or(p.title)
        .to_string();
    let mut project = NewTask {
        id: p.id.clone(),
        title: p.title,
        objective,
        acceptance: p.acceptance.to_vec(),
        parent: None,
        deps: vec![],
        session: None,
        origin: p.origin.clone(),
        state: TaskState::Accepted,
    }
    .build(now_ms);
    project.kind = TaskKind::Project;
    project.due_at_ms = p.due_at_ms;
    project.review.clone_from(&p.review);
    let mut out = vec![project];
    for (i, m) in p.milestones.iter().enumerate() {
        let mut rec = NewTask {
            id: milestone_id(&p.id, i + 1),
            title: m.title.trim(),
            objective: m.title.trim().to_string(),
            acceptance: vec![],
            parent: Some(p.id.clone()),
            deps: vec![],
            session: None,
            origin: p.origin.clone(),
            state: TaskState::Accepted,
        }
        .build(now_ms);
        rec.kind = TaskKind::Milestone;
        rec.due_at_ms = due.get(i).copied().flatten();
        out.push(rec);
    }
    out
}

/// Check the milestones' titles and count, and read their dates.
pub fn dates(
    kernel: &theseus_kernel::Kernel,
    milestones: &[MilestoneSpec],
    max: usize,
) -> Result<Vec<Option<u64>>, String> {
    if milestones.len() > max {
        return Err(format!(
            "{} milestones is over the {max} a project is made with ([projects] max_milestones)",
            milestones.len()
        ));
    }
    let mut out = Vec::new();
    for (i, m) in milestones.iter().enumerate() {
        let t = m.title.trim();
        if t.is_empty() || t.chars().count() > 200 || t.contains('\n') {
            return Err(format!(
                "milestone {}: a title is one line of at most 200 characters",
                i + 1
            ));
        }
        out.push(match m.due.as_deref().map(str::trim) {
            None | Some("") => None,
            Some(d) => Some(due_of(
                kernel,
                d,
                &format!("milestone {} (\"{t}\")", i + 1),
            )?),
        });
    }
    Ok(out)
}

/// What `task.create` of a project or a milestone adds to the plan item it
/// makes: read before the record is built (so a refusal writes nothing), and
/// applied to it.
pub struct Long {
    kind: TaskKind,
    due: Option<u64>,
    dates: Vec<Option<u64>>,
}

impl Long {
    /// Check the kind's rules and read its dates.
    pub fn read(
        tc: &crate::toolrun::TurnCtx<'_>,
        item: &crate::task_graph::tools::Item<'_>,
        all: &[TaskRecord],
        parent: Option<&str>,
    ) -> Result<Self, String> {
        let kind = item.kind;
        match kind {
            TaskKind::Item => {}
            TaskKind::Milestone => {
                let under = parent
                    .and_then(|p| all.iter().find(|t| t.id == p))
                    .and_then(|p| super::project_of(all, p));
                if under.is_none() {
                    return Err(
                        "a milestone goes under a project: give `parent` (the project's id)".into(),
                    );
                }
            }
            TaskKind::Project => {
                if parent.is_some() {
                    return Err("a project has no parent: leave `parent` out".into());
                }
            }
        }
        let max = item.ext.map_or(32, |(_, c)| c.max_milestones);
        Ok(Self {
            kind,
            due: item.due.map(|d| due_of(tc.kernel, d, "due")).transpose()?,
            dates: dates(tc.kernel, item.milestones, max)?,
        })
    }

    /// The plan item as its kind makes it: the record, the milestone records
    /// that go in the same frame, and what to add to the call's result.
    pub fn apply(
        self,
        tc: &crate::toolrun::TurnCtx<'_>,
        mut rec: TaskRecord,
        item: &crate::task_graph::tools::Item<'_>,
    ) -> Result<(TaskRecord, Vec<TaskRecord>, String), String> {
        rec.kind = self.kind;
        rec.due_at_ms = self.due;
        if self.kind != TaskKind::Project {
            return Ok((rec, vec![], String::new()));
        }
        let mut said = String::new();
        let want_review = item.review.map(str::trim) != Some("none");
        if let (true, Some((book, cfg))) = (want_review, item.ext) {
            if tc.task.is_some() {
                return Err("a task cannot set up a routine: give `review: \"none\"`".into());
            }
            if let Some(why) = crate::places::refusal(
                tc.class,
                crate::routine_tool::CREATE,
                &theseus_tools::Plan::default(),
                &[],
            ) {
                return Err(format!("Refused: {why}."));
            }
            let new = super::review::request(cfg, (&rec.id, &rec.title), item.review, None);
            let by = format!("model:{}", tc.session_id);
            let spec =
                super::review::make(tc.kernel, tc.store, book, &new, (&by, Some(tc.session_id)))?;
            said = format!(
                " Its weekly review is routine {} ({}): `theseus routine runs {}` is the project's history.",
                crate::routine::short(&spec.session_id),
                spec.schedule,
                crate::routine::short(&spec.session_id)
            );
            rec.review = Some(spec.session_id);
        }
        let made = records(
            &NewProject {
                id: rec.id.clone(),
                title: &rec.title,
                objective: None,
                acceptance: &[],
                milestones: item.milestones,
                due_at_ms: None,
                origin: rec.origin.clone(),
                review: None,
            },
            &self.dates,
            rec.created_at_ms,
        );
        let more: Vec<TaskRecord> = made.into_iter().skip(1).collect();
        if !more.is_empty() {
            said.push_str(&format!(
                " Milestones: {}.",
                more.iter()
                    .map(|m| format!("{} ({})", m.title, crate::task::short(&m.id)))
                    .collect::<Vec<_>>()
                    .join("; ")
            ));
        }
        Ok((rec, more, said))
    }
}
