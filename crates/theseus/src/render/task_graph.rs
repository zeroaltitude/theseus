//! The task graph in `theseus tasks` (M7 39a, theseus-ext.6): every task
//! record as a tree, each line its id, title, state, owner, deps, and
//! version, a waiting proposal and the evidence count marked.

use theseus_protocol::tasks::TaskRecord;

/// The tree, roots oldest first, each child indented under its parent. A
/// task whose parent is not in `records` is a root.
pub fn task_tree_lines(records: &[TaskRecord]) -> Vec<String> {
    if records.is_empty() {
        return vec![];
    }
    let open = records.iter().filter(|t| !t.state.is_closed()).count();
    let mut out = vec![format!(
        "task graph: {open} open, {} closed",
        records.len() - open
    )];
    let is_root = |t: &TaskRecord| {
        t.parent
            .as_deref()
            .is_none_or(|p| !records.iter().any(|x| x.id == p))
    };
    let mut seen = std::collections::HashSet::new();
    let mut stack: Vec<(&TaskRecord, usize)> = records
        .iter()
        .filter(|t| is_root(t))
        .rev()
        .map(|t| (t, 0))
        .collect();
    while let Some((t, depth)) = stack.pop() {
        if !seen.insert(t.id.as_str()) {
            continue;
        }
        out.push(format!("{}{}", "  ".repeat(depth + 1), tree_line(t)));
        for c in records
            .iter()
            .filter(|c| c.parent.as_deref() == Some(t.id.as_str()))
            .rev()
        {
            stack.push((c, depth + 1));
        }
    }
    out
}

/// One task: `tsk_…a1b2c3 "Title" [in_progress] v3 · owner agent · deps … ·
/// session …d4e5f6 · 2 evidence · a change waits`.
pub fn tree_line(t: &TaskRecord) -> String {
    let mut s = format!(
        "{} \"{}\" [{}] v{}",
        t.id,
        t.title,
        t.state.as_str(),
        t.version
    );
    if t.owner != "agent" {
        s.push_str(&format!(" · owner {}", t.owner));
    }
    if !t.deps.is_empty() {
        s.push_str(&format!(" · deps {}", t.deps.join(", ")));
    }
    if let Some(session) = &t.session {
        let n = session.len();
        s.push_str(&format!(" · session {}", &session[n.saturating_sub(6)..]));
    }
    if !t.evidence.is_empty() {
        s.push_str(&format!(" · {} evidence", t.evidence.len()));
    }
    if t.proposal.is_some() {
        s.push_str(" · a change waits for the operator");
    }
    s
}

#[cfg(test)]
mod tests {
    use super::*;
    use theseus_protocol::tasks::TaskState;

    fn task(id: &str, parent: Option<&str>, state: TaskState, version: u64) -> TaskRecord {
        TaskRecord {
            id: id.into(),
            version,
            title: format!("the {id} part"),
            state,
            parent: parent.map(String::from),
            owner: "agent".into(),
            ..TaskRecord::default()
        }
    }

    /// Children sit under their parents, in the order they were made, with
    /// states and versions.
    #[test]
    fn the_tree_indents_children_with_states_and_versions() {
        let records = vec![
            task("tsk_harbour", None, TaskState::InProgress, 2),
            task("tsk_buoy", None, TaskState::Done, 3),
            task("tsk_harbour1", Some("tsk_harbour"), TaskState::Accepted, 1),
            task("tsk_harbour2", Some("tsk_harbour"), TaskState::Accepted, 1),
        ];
        assert_eq!(
            task_tree_lines(&records),
            [
                "task graph: 3 open, 1 closed",
                "  tsk_harbour \"the tsk_harbour part\" [in_progress] v2",
                "    tsk_harbour1 \"the tsk_harbour1 part\" [accepted] v1",
                "    tsk_harbour2 \"the tsk_harbour2 part\" [accepted] v1",
                "  tsk_buoy \"the tsk_buoy part\" [done] v3",
            ]
        );
        assert!(task_tree_lines(&[]).is_empty());
    }
}
