//! A task's report when its model closes the task's own record (theseus-mgs4m):
//! the record is closed before the final message, so no `report` evidence
//! names that message, and `task show` read the end's reason, the word
//! `reported`. The report is the task's last substantive text, else what
//! `task.close` itself said, else that it closed with no report; never the
//! status word.

use std::sync::{Arc, Mutex};

use serde_json::json;
use theseus_kernel::ExecState;
use theseus_protocol::task_start::{TaskStartParams, TaskStartResult};
use theseus_protocol::TaskListParams;

use crate::approval::{Answerer, Surface};
use crate::provider::{ProviderRequest, Scripted};
use crate::task::NO_REPORT;
use crate::tests_tasks::{answers_a_call, exec, first_user, rig, until, Rig};

/// What the task's model does once it has closed its record: `Some(text)` says
/// it, `None` says nothing at all.
type After = Option<&'static str>;

/// A rig whose task closes its own record at its first loop (`id` is set by
/// the test once the task is known), then ends with `after`.
pub(crate) fn closing(id: Arc<Mutex<String>>, evidence: serde_json::Value, after: After) -> Rig {
    rig(
        move |req: &ProviderRequest| {
            if !first_user(req).contains("Close first") {
                return Scripted::text("ok");
            }
            if answers_a_call(req) {
                return match after {
                    Some(t) => Scripted::text(t),
                    None => Scripted::Blocks {
                        blocks: vec![],
                        stop_reason: "end_turn".into(),
                    },
                };
            }
            let id = id.lock().unwrap().clone();
            Scripted::tools(
                "",
                &[(
                    "c1",
                    "task_close",
                    json!({"id": id, "version": 1, "outcome": "done", "evidence": evidence}),
                )],
            )
        },
        |_| {},
    )
}

pub(crate) fn cli() -> Answerer {
    Answerer {
        label: "the CLI".into(),
        surface: Surface::Cli,
        discord: None,
        agent: None,
    }
}

/// Start the task, let it run once its record's id is known, and wait for
/// its end; what `task show` reports.
pub(crate) async fn run(
    r: &mut Rig,
    id: &Arc<Mutex<String>>,
    brief: &str,
) -> (TaskStartResult, Option<String>) {
    r.model.hold.lock().unwrap().replace("Close first".into());
    let started = r
        .core
        .task_start(
            &TaskStartParams {
                brief: brief.into(),
                ..Default::default()
            },
            cli(),
        )
        .unwrap();
    *id.lock().unwrap() = crate::task_graph::of_session(&started.session_id);
    r.entered.recv().await.expect("the task's first call began");
    r.model.gate.add_permits(8);
    let eid = started.session_id.replacen("ses_", "exe_", 1);
    until("the task's end", || {
        exec(&r.core, &eid).state == ExecState::Complete
    })
    .await;
    // The model's own close took: the record is closed before the last message.
    let rec = crate::task_graph::get(&r.core.store, &id.lock().unwrap())
        .unwrap()
        .expect("the task's record");
    assert_eq!(rec.state, crate::task_graph::TaskState::Done, "{rec:?}");
    let shown = r
        .core
        .task_list(TaskListParams {
            id: Some(started.short.clone()),
            ..Default::default()
        })
        .unwrap();
    (started, shown.details[0].report.clone())
}

#[tokio::test]
async fn a_task_closed_before_its_last_message_reports_that_message() {
    let id: Arc<Mutex<String>> = Arc::default();
    let evidence = json!([{"identity": "boats.txt", "note": "counted 14"}]);
    let mut r = closing(id.clone(), evidence, Some("Counted 14 boats."));
    let (_, report) = run(&mut r, &id, "Close first, then count the boats").await;
    assert_eq!(report.as_deref(), Some("Counted 14 boats."));
}

#[tokio::test]
async fn a_task_closed_with_no_message_reports_what_its_close_said() {
    let id: Arc<Mutex<String>> = Arc::default();
    let evidence = json!([{"identity": "boats.txt", "note": "counted 14"}]);
    let mut r = closing(id.clone(), evidence, None);
    let (_, report) = run(&mut r, &id, "Close first, then count the boats").await;
    let report = report.expect("a report");
    assert!(report.contains("counted 14"), "{report}");
    assert_ne!(report, "reported");
}

#[tokio::test]
async fn a_task_that_ended_with_nothing_to_report_says_so() {
    let r = rig(
        |req| match first_user(req).contains("Say nothing") {
            true => Scripted::Blocks {
                blocks: vec![],
                stop_reason: "end_turn".into(),
            },
            false => Scripted::text("ok"),
        },
        |_| {},
    );
    let started = r
        .core
        .task_start(
            &TaskStartParams {
                brief: "Say nothing".into(),
                ..Default::default()
            },
            cli(),
        )
        .unwrap();
    let eid = started.session_id.replacen("ses_", "exe_", 1);
    until("the task's end", || {
        exec(&r.core, &eid).state == ExecState::Complete
    })
    .await;
    let shown = r
        .core
        .task_list(TaskListParams {
            id: Some(started.short.clone()),
            ..Default::default()
        })
        .unwrap();
    assert_eq!(shown.details[0].report.as_deref(), Some(NO_REPORT));
    // The report the parent reads and the post a surface renders say it too.
    let report = crate::task::load_report(&r.core.store, &r.core.kernel, &eid)
        .unwrap()
        .expect("a report");
    assert_eq!(report.text.as_deref(), Some(NO_REPORT));
    assert_eq!(report.post_body()["text"], NO_REPORT);
    assert!(
        report.node_text().contains(NO_REPORT),
        "{}",
        report.node_text()
    );
}
