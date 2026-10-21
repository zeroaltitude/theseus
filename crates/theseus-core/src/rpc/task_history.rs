//! `task.list`'s filters (theseus-ypnxt.1): find a task after it has ended.
//!
//! - **Absent means as before.** A call with no `ended`, `since_ms`, `query`,
//!   `id` or `limit` is `task_list`'s old body, byte for byte; nothing here runs.
//! - **The report** is read where it is kept: the node its record's
//!   `report:` evidence names (one point read), or for a task that failed or
//!   was stopped, the reason it ended. It is read only for `ended`, `query` and
//!   `id`, and the digest names each reported task by the same function
//!   (`Core::report_text`).
//! - **Details beside the tasks.** The report's first line, and for `id` the
//!   whole report and the hold, ride `TaskListResult.details`, so `TaskInfo`
//!   (built in many places) is unchanged.

use std::collections::HashSet;

use theseus_protocol::tasks::{TaskDetail, TaskRecord};
use theseus_protocol::{error_code, TaskInfo, TaskListParams, TaskListResult};

use super::server::RpcFailure;
use super::Core;
use crate::node::Body;

/// The longest first line of a report the wire carries, in characters.
const LINE_CHARS: usize = 240;

/// Whether an execution state (`TaskInfo.state`) is an end.
fn ended(state: &str) -> bool {
    matches!(
        state,
        "complete" | "failed" | "cancelled" | "budget_exhausted"
    )
}

/// The first non-empty line of `text`, cut to `LINE_CHARS`.
pub(crate) fn first_line(text: &str) -> Option<String> {
    let l = text.lines().map(str::trim).find(|l| !l.is_empty())?;
    Some(match l.char_indices().nth(LINE_CHARS) {
        Some((i, _)) => format!("{}…", &l[..i]),
        None => l.to_string(),
    })
}

impl Core {
    /// A task's report in its own words: the node its record's `report:`
    /// evidence names, else the reason it ended (what a failed task's
    /// evidence notes, or a stop's). None for a task that has not ended.
    pub(crate) fn report_text(
        &self,
        rec: Option<&TaskRecord>,
        reason: Option<&str>,
    ) -> Option<String> {
        let ev = rec.and_then(|r| {
            r.evidence
                .iter()
                .rev()
                .find(|e| e.identity.starts_with("report:"))
        });
        let node = ev
            .and_then(|e| e.node.as_deref())
            .and_then(|id| self.store.get_node(id).ok().flatten())
            .and_then(|(_, n)| match n.body {
                Body::AssistantMessage { blocks, .. } => {
                    Some(crate::provider::text_of(&blocks)).filter(|t| !t.is_empty())
                }
                _ => None,
            });
        node.or_else(|| ev.and_then(|e| e.note.clone()))
            .or_else(|| reason.map(String::from))
            .filter(|t| !t.trim().is_empty())
    }

    /// A task's report as `task show` and the ended list give it: `report_text`
    /// for a task that failed or was stopped; for one that finished, its report
    /// node, else its last message that has text (its model closed the task's
    /// record before writing it, so no report evidence names it), else what its
    /// own close said, else that it closed with no report. Never the word its
    /// end is filed under (theseus-mgs4m).
    pub(crate) fn report_of(
        &self,
        rec: Option<&TaskRecord>,
        t: &theseus_protocol::TaskInfo,
    ) -> Option<String> {
        if t.state != "complete" {
            return self.report_text(rec, t.ended_reason.as_deref());
        }
        self.report_text(rec, None)
            .or_else(|| {
                let nodes = self.store.session_nodes(&t.task_id).ok()?;
                crate::task::last_message(nodes.iter().map(|(_, n)| n)).map(|(_, text)| text)
            })
            .or_else(|| rec.and_then(crate::task::close_summary))
            .or_else(|| Some(crate::task::NO_REPORT.to_string()))
    }

    /// `task.list` with a filter set. `tasks` and `records` are what the
    /// plain call would answer.
    pub(super) fn task_history(
        &self,
        p: &TaskListParams,
        tasks: Vec<TaskInfo>,
        records: Vec<TaskRecord>,
    ) -> Result<TaskListResult, RpcFailure> {
        let by_session = |s: &str| records.iter().find(|r| r.session.as_deref() == Some(s));
        if let Some(name) = p.id.as_deref() {
            return self.task_one(name, &tasks, &records);
        }
        let words: Vec<String> = p
            .query
            .iter()
            .flat_map(|q| q.split_whitespace())
            .map(str::to_lowercase)
            .collect();
        let wants_report = p.ended == Some(true) || !words.is_empty();
        let mut kept: Vec<TaskInfo> = Vec::new();
        let mut details: Vec<TaskDetail> = Vec::new();
        // `tasks` is newest first, so a limit takes the newest.
        for t in tasks {
            let is_ended = ended(&t.state);
            if p.ended.is_some_and(|want| want != is_ended)
                || p.since_ms.is_some_and(|ms| t.updated_at_ms < ms)
            {
                continue;
            }
            let rec = by_session(&t.task_id);
            let line = (wants_report && is_ended)
                .then(|| self.report_of(rec, &t))
                .flatten();
            if !words.is_empty() {
                let hay = self.haystack(&t, rec, line.as_deref());
                if !words.iter().all(|w| hay.contains(w)) {
                    continue;
                }
            }
            if wants_report {
                details.push(TaskDetail {
                    task_id: t.task_id.clone(),
                    report_line: line.as_deref().and_then(first_line),
                    ..Default::default()
                });
            }
            kept.push(t);
            if p.limit.is_some_and(|n| kept.len() >= n as usize) {
                break;
            }
        }
        let sessions: HashSet<&str> = kept.iter().map(|t| t.task_id.as_str()).collect();
        let records = records
            .iter()
            .filter(|r| r.session.as_deref().is_some_and(|s| sessions.contains(s)))
            .cloned()
            .collect();
        Ok(TaskListResult {
            tasks: kept,
            records,
            details,
        })
    }

    /// The lowercase text a query is matched against: the title, the
    /// objective (else the brief) and the report's first line.
    fn haystack(&self, t: &TaskInfo, rec: Option<&TaskRecord>, report: Option<&str>) -> String {
        let mut hay = t.title.clone().unwrap_or_default();
        let objective = rec.map(|r| r.objective.as_str()).filter(|o| !o.is_empty());
        match objective {
            Some(o) => hay.push_str(o),
            None => {
                if let Ok(Some(Body::UserMessage { text, .. })) =
                    crate::task::brief(&self.store, &t.task_id).map(|n| n.map(|n| n.body))
                {
                    hay.push_str(&text);
                }
            }
        }
        hay.push('\n');
        hay.push_str(report.and_then(first_line).as_deref().unwrap_or(""));
        hay.to_lowercase()
    }

    /// One task by `name` (its id or the end of it), with its children and
    /// its whole report.
    fn task_one(
        &self,
        name: &str,
        tasks: &[TaskInfo],
        records: &[TaskRecord],
    ) -> Result<TaskListResult, RpcFailure> {
        let rec = crate::task_graph::resolve(records, name)
            .map_err(|e| RpcFailure::new(error_code::NOT_FOUND, e))?;
        let info = rec
            .session
            .as_deref()
            .and_then(|s| tasks.iter().find(|t| t.task_id == s));
        let mut kept = records_below(records, rec);
        kept.insert(0, rec.clone());
        let detail = info.map(|t| {
            let text = ended(&t.state)
                .then(|| self.report_of(Some(rec), t))
                .flatten();
            TaskDetail {
                task_id: t.task_id.clone(),
                report_line: text.as_deref().and_then(first_line),
                report: text,
                paused: self.runner.task_paused(&t.task_id),
            }
        });
        Ok(TaskListResult {
            tasks: info.cloned().into_iter().collect(),
            records: kept,
            details: detail.into_iter().collect(),
        })
    }
}

/// `rec`'s children, in the order they were made.
fn records_below(records: &[TaskRecord], rec: &TaskRecord) -> Vec<TaskRecord> {
    crate::task_graph::children(records, &rec.id)
        .into_iter()
        .cloned()
        .collect()
}

#[cfg(test)]
#[path = "tests_task_history.rs"]
mod tests;
