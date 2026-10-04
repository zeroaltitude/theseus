//! Health's parked tasks (M5 28b; the parked-task invariant): a line per
//! task that cannot progress by itself, with what holds it. Apart from
//! `render.rs`, whose length the shape budget caps.

use theseus_protocol::TasksHealth;

use super::{fmt_time, push, Line, Tag};

/// `tasks parked: 2`, then a line per task; nothing when none is.
pub(super) fn push_health(o: &mut Vec<Line>, h: Option<&TasksHealth>) {
    for line in parked_lines(h) {
        push(o, Tag::Plain, &line);
    }
}

/// The lines: `tasks parked: 1`, then `  task a1b2c3 "Sweep the dock": an
/// approval unanswered for 26 h (since 09:14)`.
pub fn parked_lines(h: Option<&TasksHealth>) -> Vec<String> {
    let Some(h) = h.filter(|h| !h.parked.is_empty()) else {
        return vec![];
    };
    let mut out = vec![format!(
        "tasks parked: {} (each waits on something that will not come by itself)",
        h.parked.len()
    )];
    for p in &h.parked {
        out.push(format!(
            "  task {}{}: {} (since {}; `theseus cancel {}` ends it)",
            p.short,
            p.title
                .as_deref()
                .map(|t| format!(" \"{t}\""))
                .unwrap_or_default(),
            p.detail,
            fmt_time(p.since_ms),
            p.short
        ));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use theseus_protocol::ParkedTask;

    #[test]
    fn parked_tasks_are_listed_with_their_blockers_and_none_says_nothing() {
        assert!(parked_lines(None).is_empty());
        assert!(parked_lines(Some(&TasksHealth::default())).is_empty());
        let h = TasksHealth {
            parked: vec![ParkedTask {
                task_id: "ses_0000dock01".into(),
                short: "dock01".into(),
                execution_id: "exe_1".into(),
                title: Some("Sweep the dock".into()),
                state: "waiting".into(),
                blocker: "approval".into(),
                detail: "an approval unanswered for 26 h".into(),
                since_ms: 0,
            }],
        };
        let lines = parked_lines(Some(&h));
        assert_eq!(lines.len(), 2);
        assert!(lines[0].starts_with("tasks parked: 1"), "{lines:?}");
        assert!(
            lines[1]
                .starts_with("  task dock01 \"Sweep the dock\": an approval unanswered for 26 h"),
            "{lines:?}"
        );
    }
}
