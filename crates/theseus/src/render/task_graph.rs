//! The task graph in `theseus tasks` (M7 39a, theseus-ext.6): every task
//! record as a tree, each line its id, title, state, owner, deps, and
//! version, a waiting proposal, a claim (39b), and the evidence count
//! marked.

use theseus_protocol::tasks::TaskRecord;

/// The tree, roots oldest first, each child indented under its parent. A
/// task whose parent is not in `records` is a root.
pub fn task_tree_lines(records: &[TaskRecord]) -> Vec<String> {
    task_tree_lines_at(records, theseus_protocol::now_unix_ms())
}

/// `task_tree_lines` at `now_ms`, which a claim's lease is read against.
pub fn task_tree_lines_at(records: &[TaskRecord], now_ms: u64) -> Vec<String> {
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
        out.push(format!(
            "{}{}",
            "  ".repeat(depth + 1),
            tree_line_at(t, now_ms)
        ));
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
    tree_line_at(t, theseus_protocol::now_unix_ms())
}

/// `tree_line` at `now_ms`: a claim reads `claimed by session …d4e5f6, in
/// 29m` while its lease holds.
pub fn tree_line_at(t: &TaskRecord, now_ms: u64) -> String {
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
    if let Some(c) = t.claim_at(now_ms) {
        s.push_str(&format!(
            " · claimed by session {}, {}",
            c.session_short(),
            super::until_due(c.until_ms, now_ms)
        ));
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

    /// A layer-1 question reads as its card does (39b), with how to accept or
    /// decline it.
    #[test]
    fn a_layer_one_question_says_the_change() {
        let c: theseus_protocol::ConfirmRequest = serde_json::from_value(serde_json::json!({
            "correlation_id": "act_0000reef", "session_id": "ses_0000lagoon",
            "execution_id": "exe_0000lagoon", "tool": "task.update", "input": {},
            "reason": "layer 1", "by": "operator", "requested_at_ms": 1, "expires_at_ms": 2,
            "change": {"task": "tsk_0000reef", "title": "Chart the reef", "field": "objective",
                       "before": "chart the north edge", "after": "chart the whole reef"}
        }))
        .unwrap();
        let lines: Vec<String> = crate::render::confirm_lines(&c)
            .into_iter()
            .map(|l| l.text)
            .collect();
        assert_eq!(
            lines,
            [
                "  ? Change the objective of tsk_0000reef (Chart the reef)? Before: chart the \
                 north edge After: chart the whole reef",
                "      accept: theseus confirm act_0000reef",
                "      decline: theseus confirm --decline act_0000reef",
            ]
        );
    }

    /// A claim shows its session and how long its lease has left while it
    /// holds, and nothing once it lapsed (39b).
    #[test]
    fn a_claim_shows_while_its_lease_holds() {
        let mut t = task("tsk_reef", None, TaskState::Accepted, 2);
        t.claim = Some(theseus_protocol::tasks::TaskClaim {
            by: "exe_0000harbour".into(),
            session: "ses_0000harbour".into(),
            until_ms: 1_800_000,
        });
        assert_eq!(
            tree_line_at(&t, 60_000),
            "tsk_reef \"the tsk_reef part\" [accepted] v2 · claimed by session arbour, in 29m"
        );
        assert_eq!(
            tree_line_at(&t, 1_800_000),
            "tsk_reef \"the tsk_reef part\" [accepted] v2"
        );
    }
}
