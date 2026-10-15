//! A task's progress line in the sidebar (theseus-xlias): the work board's
//! `3/5 · line`, by the position rule, on the row of the session it names.

use serde_json::json;
use theseus_protocol::work::WorkView;

use crate::board::Board;

fn view(position: u64, steps: (u32, u32), said: Option<&str>) -> WorkView {
    serde_json::from_value(json!({
        "id": "exe_1", "kind": "task", "position": position, "at_ms": 1, "root": "exe_1",
        "session_id": "ses_task", "title": "t", "state": "running",
        "attention": {"level": "working", "label": "turn 1", "since_ms": 1},
        "started_at_ms": 1, "state_since_ms": 1, "flagged": false,
        "progress": {"steps_done": steps.0, "steps_total": steps.1, "said": said, "at_ms": 1}
    }))
    .unwrap()
}

#[test]
fn a_rows_line_follows_the_work_board_by_position() {
    let mut b = Board::default();
    assert_eq!(b.progress_line("ses_task").as_deref(), None);
    b.work_views(&[view(5, (0, 3), Some("Laying out the plan."))]);
    assert_eq!(
        b.progress_line("ses_task").as_deref(),
        Some("0/3 · Laying out the plan.")
    );
    b.work_views(&[view(9, (3, 3), Some("Closing the last step."))]);
    assert_eq!(
        b.progress_line("ses_task").as_deref(),
        Some("3/3 · Closing the last step.")
    );
    // An older view, heard late, changes nothing.
    b.work_views(&[view(7, (1, 3), None)]);
    assert_eq!(
        b.progress_line("ses_task").as_deref(),
        Some("3/3 · Closing the last step.")
    );
}
