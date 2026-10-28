//! A task's own close (theseus-ay0yx): the task's record is the task's own, so
//! its `task.close` in its own session runs without the owner's yes; and the task's brief tells it the task
//! graph is the system's business, so its report leads with the result.

use std::sync::{Arc, Mutex};

use serde_json::json;
use theseus_kernel::ExecState;

use crate::tests_task_report::{closing, run};
use crate::tests_tasks::exec;

#[tokio::test]
async fn a_tasks_own_close_runs_without_asking() {
    let id: Arc<Mutex<String>> = Arc::default();
    let evidence = json!([{"identity": "boats.txt", "note": "counted 14"}]);
    let mut r = closing(id.clone(), evidence, Some("Counted 14 boats."));
    // `run` waits for the task to end and reads its record closed: a close
    // that waited for the owner would never let it end.
    let (started, report) = run(&mut r, &id, "Close first, then count the boats").await;
    let eid = started.session_id.replacen("ses_", "exe_", 1);
    assert_eq!(exec(&r.core, &eid).state, ExecState::Complete);
    assert_eq!(report.as_deref(), Some("Counted 14 boats."));
    let pending = r.core.tasks(None, None).unwrap();
    assert!(
        pending.iter().all(|t| t.pending_confirms == 0),
        "{pending:?}"
    );
}

#[test]
fn the_brief_says_the_task_graph_is_the_systems_business() {
    for text in [
        crate::task::brief_text_for_tests("ses_aaaaaaaa", "ses_bbbbbbbb", "Count the boats"),
        crate::rpc::task_start::brief_text_for_tests(
            "ses_aaaaaaaa",
            "ses_bbbbbbbb",
            "Count the boats",
        ),
    ] {
        assert!(
            text.contains("task graph is the system's business"),
            "{text}"
        );
        assert!(text.contains("open your report with the result"), "{text}");
    }
}
