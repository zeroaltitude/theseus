//! A task's `doing` through whole cores (theseus-ztkhg): while its foreground command runs the view says
//! the command, with the command's own time, and once the command is back the model's thinking counts from
//! the command's end, not from the turn's start.

use serde_json::json;
use theseus_protocol::work::{DoingKind, WorkKind};

use crate::provider::Scripted;
use crate::push::WorkScope;
use crate::tests_tasks::{
    answers_a_call, arranged, first_user, parent_session, rig, turn, until, ASK,
};
use crate::tests_work::Watcher;

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_running_tool_is_named_with_its_own_time() {
    let r = rig(
        |req| {
            if first_user(req).contains("CHILD:") {
                return match answers_a_call(req) {
                    true => Scripted::text("Ran it."),
                    false => Scripted::tools(
                        "Running it.",
                        &[("p1", "proc_run", json!({"argv": ["sleep", "3"]}))],
                    ),
                };
            }
            if answers_a_call(req) {
                return Scripted::text("Started it.");
            }
            Scripted::tools(
                "Starting it.",
                &[(
                    "t_task",
                    "task_create",
                    json!({"brief": "CHILD: run the command", "budget_usd": 1.5, "arrangement": arranged()}),
                )],
            )
        },
        |c| {
            c.policy
                .tools
                .insert("proc.run".into(), crate::policy::Posture::Open);
        },
    );
    let mut r = r;
    *r.model.hold.lock().unwrap() = Some("CHILD".into());
    let parent = parent_session(&r.core);
    let (mut w, _) = Watcher::start(&r.core, WorkScope::default()).await;
    turn(&r.core, &parent, ASK).await;
    r.entered.recv().await.unwrap();
    r.model.gate.add_permits(1);
    let doing = |w: &mut Watcher| {
        w.held()
            .values()
            .find(|v| v.kind == WorkKind::Task)
            .and_then(|v| v.doing.clone())
    };
    until("the command is the task's doing", || {
        doing(&mut w).is_some_and(|d| d.what != DoingKind::Thinking)
    })
    .await;
    let d = doing(&mut w).unwrap();
    assert!(matches!(d.what, DoingKind::Tool | DoingKind::Job), "{d:?}");
    assert!(
        d.label.contains("sleep") || d.label.contains("proc"),
        "{d:?}"
    );
    // The command ends, and the model's next call waits at the gate: its thinking counts from the
    // command's end.
    r.entered.recv().await.unwrap();
    until("thinking again", || {
        doing(&mut w).is_some_and(|d| d.what == DoingKind::Thinking)
    })
    .await;
    let t = doing(&mut w).unwrap();
    assert!(
        t.since_ms >= d.since_ms + 2_500,
        "thinking counts from the command's end: {t:?} after {d:?}"
    );
    r.model.gate.add_permits(4);
}
