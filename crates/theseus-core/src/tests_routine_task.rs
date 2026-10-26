//! A task proposes a routine (theseus-lyxn7, the owner, 2026-10-25 13:58: "i want
//! there to be precious few hard gates /anywhere/ in theseus"). Its card waits
//! on the owner as any routine's does; the owner's yes makes the routine. Before,
//! the card was raised and the yes was then answered "a task cannot set up a
//! routine". A sibling of `tests_ask`, whose rig it uses.

use serde_json::{json, Value};

use crate::provider::{ProviderRequest, Scripted};
use crate::tests_tasks::{
    answers_a_call, arranged, first_user, only_task, parent_session, rig, turn, until, ASK,
};

const BRIEF: &str = "Read the tide table and say when the next high tide is.";

/// The text of the tool result the request's last message carries.
fn result_text(req: &ProviderRequest) -> String {
    let last = req.messages.last().cloned().unwrap_or_default();
    match &last["content"][0]["content"] {
        Value::String(s) => s.clone(),
        Value::Array(b) => b
            .iter()
            .filter_map(|b| b["text"].as_str())
            .collect::<Vec<_>>()
            .join("\n"),
        _ => String::new(),
    }
}

fn model() -> impl Fn(&ProviderRequest) -> Scripted + Send + Sync + 'static {
    |req| {
        if first_user(req).contains("CHILD:") {
            if answers_a_call(req) {
                return Scripted::text(&format!("Routine result: {}", result_text(req)));
            }
            return Scripted::tools(
                "",
                &[(
                    "t_routine",
                    "routine_create",
                    json!({"title": "Tide check", "schedule": "weekdays 08:00", "brief": BRIEF}),
                )],
            );
        }
        if answers_a_call(req) {
            return Scripted::text("Started it.");
        }
        Scripted::tools(
            "Starting it.",
            &[(
                "t_task",
                "task_create",
                json!({"brief": "CHILD: set up a tide routine", "arrangement": arranged()}),
            )],
        )
    }
}

/// The task's `routine_create` raises its card, the owner's yes makes the
/// routine, and the task is told it exists.
#[tokio::test(flavor = "multi_thread")]
async fn a_task_proposes_a_routine_and_the_owners_yes_makes_it() {
    let r = rig(model(), |_| {});
    let parent = parent_session(&r.core);
    r.core.bind_places(vec![crate::places::BoundPlace {
        target: "discord:dm:42".into(),
        name: "DM @owner".into(),
        private: true,
        ..Default::default()
    }]);
    turn(&r.core, &parent, ASK).await;
    let task = only_task(&r.core, &parent);
    until("the task's card", || {
        r.core.confirm_list().unwrap().len() == 1
    })
    .await;
    let card = r.core.confirm_list().unwrap().remove(0);
    assert_eq!(card.tool, "routine.create");
    assert_eq!(card.session_id, task.session_id);
    r.core
        .confirm_action(&card.correlation_id, true, None, "cli")
        .unwrap();
    until("the task to finish", || {
        crate::tests_tasks::exec(&r.core, &task.id).state == theseus_kernel::ExecState::Complete
    })
    .await;
    let made = r.core.runner.tools.routines.all(&r.core.store).unwrap();
    assert_eq!(made.len(), 1, "the owner's yes made no routine: {made:?}");
    let spec = r
        .core
        .runner
        .tools
        .routines
        .all(&r.core.store)
        .unwrap()
        .remove(0);
    assert_eq!(spec.created_in.as_deref(), Some(task.session_id.as_str()));
    assert_eq!(spec.created_by, format!("model:{}", task.session_id));
}

/// Nothing exists before the owner answers, and a "no" makes nothing: the
/// gate is the owner's, not the session's kind.
#[tokio::test(flavor = "multi_thread")]
async fn a_task_s_routine_waits_on_the_owner_and_a_no_makes_nothing() {
    let r = rig(model(), |_| {});
    let parent = parent_session(&r.core);
    r.core.bind_places(vec![crate::places::BoundPlace {
        target: "discord:dm:42".into(),
        name: "DM @owner".into(),
        private: true,
        ..Default::default()
    }]);
    turn(&r.core, &parent, ASK).await;
    until("the task's card", || {
        r.core.confirm_list().unwrap().len() == 1
    })
    .await;
    let routines = || r.core.runner.tools.routines.all(&r.core.store).unwrap();
    assert!(routines().is_empty(), "nothing exists before the answer");
    let card = r.core.confirm_list().unwrap().remove(0);
    r.core
        .confirm_action(&card.correlation_id, false, None, "cli")
        .unwrap();
    until("the card to close", || {
        r.core.confirm_list().unwrap().is_empty()
    })
    .await;
    assert!(routines().is_empty(), "a no makes no routine");
}
