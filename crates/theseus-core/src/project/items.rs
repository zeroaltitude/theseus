//! A project's items (issue 13): the front door to the project's own list. An
//! item is a task record of kind `item` under the project (what a plan step
//! already is), so nothing is a second list: the slice's "next steps", the
//! progress counts and the project's view all read the same records, and an
//! item survives a restart as any task does. No priorities, owners, labels or
//! dependencies: an item is a title and a state.
//!
//! Pure: the records are passed in. The writes are `rpc/project_items.rs`'s.

use theseus_protocol::tasks::{TaskKind, TaskRecord};

use crate::task_graph::subtree;

/// An item's title: one line, at most this many characters.
pub const MAX_TITLE: usize = 200;

/// The title as stored, or why it will not do.
pub fn title(raw: &str) -> Result<String, String> {
    let t = raw.trim();
    if t.is_empty() || t.chars().count() > MAX_TITLE || t.contains('\n') {
        return Err(format!(
            "an item's title is one line of at most {MAX_TITLE} characters"
        ));
    }
    Ok(t.to_string())
}

/// The items under `project`, oldest first: the open ones, or all with `all`.
/// An abandoned one is not work left and is listed only with `all`.
pub fn list<'a>(tasks: &'a [TaskRecord], project: &str, all: bool) -> Vec<&'a TaskRecord> {
    let mut v: Vec<&TaskRecord> = subtree(tasks, project)
        .into_iter()
        .filter(|t| t.kind == TaskKind::Item && t.id != project)
        .filter(|t| all || !t.state.is_closed())
        .collect();
    v.sort_by_key(|t| (t.created_at_ms, t.id.clone()));
    v
}

/// The item `name` means among `tasks`: an id, or an end of one, of an item
/// that belongs to some project.
pub fn named<'a>(tasks: &'a [TaskRecord], name: &str) -> Result<&'a TaskRecord, String> {
    let t = crate::task_graph::resolve(tasks, name)?;
    match (t.kind == TaskKind::Item, super::project_of(tasks, t)) {
        (true, Some(_)) => Ok(t),
        (true, None) => Err(format!("task {} is not under a project", t.id)),
        (false, _) => Err(format!(
            "task {} is a {}, not an item of a project",
            t.id,
            t.kind.as_str()
        )),
    }
}
