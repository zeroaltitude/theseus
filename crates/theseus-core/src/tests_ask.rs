//! A task asks the owner (theseus-j7cje): `owner_ask` from a background task
//! parks the task on a question, the owner answers it from the CLI by
//! number, and the task resumes with the option; the task was started with
//! `notify_done`, so its report interrupts where a plain one informs. A
//! sibling of `tests_tasks`, whose model and helpers it uses.

use serde_json::{json, Value};
use theseus_kernel::ExecState;
use theseus_protocol::notices::{Urgency, Why};

use crate::provider::{ProviderRequest, Scripted};
use crate::tests_tasks::{
    answers_a_call, arranged, exec, first_user, only_task, parent_session, posts, rig, turn, until,
    ASK,
};

/// The text of the tool result the request's last message carries.
fn result_text(req: &ProviderRequest) -> String {
    let last = req.messages.last().cloned().unwrap_or_default();
    let block = &last["content"][0]["content"];
    match block {
        Value::String(s) => s.clone(),
        Value::Array(b) => b
            .iter()
            .filter_map(|b| b["text"].as_str())
            .collect::<Vec<_>>()
            .join("\n"),
        _ => String::new(),
    }
}

/// `theseus answer <id> <option>`: `work.answer` from the CLI.
async fn answer_from_cli(core: &std::sync::Arc<crate::Core>, id: &str, option: &str) {
    crate::tests_m3::rpc_as(
        core,
        crate::tests_m3::surface("sock#1", crate::approval::Surface::Cli),
        theseus_protocol::method::WORK_ANSWER,
        json!({"question_id": id, "option": option}),
    )
    .await
    .expect("the answer counts");
}

/// The task's report notice, once the board has applied the frame that made it.
async fn report_notice(
    r: &crate::tests_tasks::Rig,
    task: &theseus_kernel::Execution,
) -> theseus_protocol::notices::Notice {
    let find = || {
        r.core
            .push
            .notices_since(0)
            .0
            .into_iter()
            .find(|n| n.why == Why::Reported && n.work.id == task.id)
    };
    until("the report's notice", || find().is_some()).await;
    find().unwrap()
}

fn model(notify_done: bool) -> impl Fn(&ProviderRequest) -> Scripted + Send + Sync + 'static {
    move |req| {
        if first_user(req).contains("CHILD:") {
            if answers_a_call(req) {
                return Scripted::text(&format!("Went with: {}", result_text(req)));
            }
            return Scripted::tools(
                "",
                &[(
                    "t_ask",
                    "owner_ask",
                    json!({"question": "Which plan?",
                           "options": [{"label": "Careful"}, {"label": "Fast", "detail": "skips the check"}],
                           "default": 1}),
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
                json!({"brief": "CHILD: pick a plan, asking the owner",
                       "arrangement": arranged(), "notify_done": notify_done}),
            )],
        )
    }
}

/// The task parks on its question, the CLI's `work.answer` by number sets it
/// going with the choice, and its finish interrupts because the owner asked
/// to hear when it was done.
#[tokio::test]
async fn a_task_asks_the_owner_and_resumes_with_the_choice() {
    let r = rig(model(true), |_| {});
    r.core.push.ensure(&r.core).await.unwrap();
    let parent = parent_session(&r.core);
    turn(&r.core, &parent, ASK).await;
    let task = only_task(&r.core, &parent);
    until("the task's question", || {
        r.core.confirm_list().unwrap().len() == 1
    })
    .await;
    let q = r.core.confirm_list().unwrap().remove(0);
    assert_eq!(q.tool, "owner_ask");
    assert_eq!(q.session_id, task.session_id);
    assert_eq!(
        q.task.as_ref().map(|t| t.task_id.as_str()),
        Some(task.session_id.as_str())
    );
    assert!(q.options.iter().any(|o| o.id == "choice:1"));
    assert_ne!(exec(&r.core, &task.id).state, ExecState::Complete);
    // The brief says what a question is for.
    let child = r
        .model
        .requests
        .lock()
        .unwrap()
        .iter()
        .find(|q| first_user(q).contains("CHILD:"))
        .cloned()
        .unwrap();
    assert!(
        first_user(&child).contains("ask with `owner_ask`"),
        "{}",
        first_user(&child)
    );
    // The owner answers from the CLI: the second option.
    answer_from_cli(&r.core, &q.correlation_id, "2").await;
    until("the task completes", || {
        exec(&r.core, &task.id).state == ExecState::Complete
    })
    .await;
    let reports = posts(&r.core, "report");
    assert_eq!(reports.len(), 1);
    let said = r
        .core
        .outbox
        .said(
            crate::outbox::body_of(&reports[0])["node"]
                .as_str()
                .unwrap(),
        )
        .unwrap();
    assert_eq!(said, "Went with: The owner chose 2, \"Fast\".");
    // `notify_done`: its report interrupts.
    let told = report_notice(&r, &task).await;
    assert_eq!(told.urgency, Urgency::Interrupt);
}

/// A task not flagged is told as it always was: its report informs.
#[tokio::test]
async fn an_unflagged_task_reports_as_an_inform() {
    let r = rig(model(false), |_| {});
    r.core.push.ensure(&r.core).await.unwrap();
    let parent = parent_session(&r.core);
    turn(&r.core, &parent, ASK).await;
    let task = only_task(&r.core, &parent);
    until("the task's question", || {
        r.core.confirm_list().unwrap().len() == 1
    })
    .await;
    let q = r.core.confirm_list().unwrap().remove(0);
    answer_from_cli(&r.core, &q.correlation_id, "1").await;
    until("the task completes", || {
        exec(&r.core, &task.id).state == ExecState::Complete
    })
    .await;
    let told = report_notice(&r, &task).await;
    assert_eq!(told.urgency, Urgency::Inform);
}
