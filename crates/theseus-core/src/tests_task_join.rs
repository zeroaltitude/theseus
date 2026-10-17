//! The owner joins a task (theseus-slmj1), through the whole core: the
//! owner's message to a running task is heard at its next loop boundary; a
//! message that comes during its last call parks the task instead of
//! reporting; a guest's queues; an ended task refuses; and the report says
//! the owner was there, once.

use std::time::Duration;

use serde_json::json;
use theseus_kernel::ExecState;
use theseus_protocol::TurnSubmitResult;

use crate::bus::EventSink;
use crate::provider::{ProviderRequest, Scripted};
use crate::session::SessionRecord;
use crate::tests_tasks::{
    answers_a_call, exec, first_user, only_task, parent_session, posts, rig, turn, until, Rig, ASK,
};
use crate::turn::{TurnError, TurnRequest};
use crate::Core;

/// `task.create { wake_parent: true }` of `brief`, as the parent's model makes it.
fn start(brief: &str) -> Scripted {
    Scripted::tools(
        "Starting.",
        &[(
            "t_task",
            "task_create",
            json!({"brief": brief, "budget_usd": 1.5, "wake_parent": true,
                   "arrangement": crate::tests_tasks::arranged()}),
        )],
    )
}

fn seen(req: &ProviderRequest) -> String {
    serde_json::to_string(&req.messages).unwrap()
}

/// A parent that starts one task, and a task that `child` answers.
fn rig_with_child(child: impl Fn(&ProviderRequest) -> Scripted + Send + Sync + 'static) -> Rig {
    rig(
        move |req| {
            if first_user(req).contains("CHILD") {
                return child(req);
            }
            if answers_a_call(req) {
                return Scripted::text("Started it.");
            }
            if first_user(req).starts_with("START") && !seen(req).contains("[Report from task") {
                return start("CHILD: survey the north channel");
            }
            Scripted::text("The task reported.")
        },
        |_| {},
    )
}

/// A turn of `session` with `input` from the owner (or a guest), running now.
fn say(
    core: &std::sync::Arc<Core>,
    session: &str,
    input: &str,
    owner: bool,
) -> tokio::task::JoinHandle<anyhow::Result<TurnSubmitResult>> {
    let core = core.clone();
    let rec: SessionRecord = core.store.get_session(session).unwrap().unwrap();
    let (live, _) = core.live_profile();
    let target = core.runner.resolve_target(&live, None, None, None).unwrap();
    let sink = EventSink::new(core.bus.clone(), session, None).from_owner(owner);
    let req = TurnRequest {
        prompt: None,
        surface: None,
        session: rec,
        input: Some(input.into()),
        target,
        sink,
        author: if owner {
            "discord:owner"
        } else {
            "discord:guest"
        }
        .into(),
        recompile: None,
        attachments: vec![],
        arrived: None,
        reply_to: None,
        answer_at: None,
    };
    tokio::spawn(async move { core.runner.run(req).await })
}

fn steers(core: &Core) -> usize {
    crate::tests_tasks::rows(core, "turn.steered").len()
}

/// The class of a refused turn.
fn class_of(e: &anyhow::Error) -> String {
    e.downcast_ref::<TurnError>()
        .map(|t| t.class.clone())
        .unwrap_or_default()
}

/// A task running its first call, held at the model's gate; `r.entered` has
/// seen it.
async fn running(r: &mut Rig) -> (String, theseus_kernel::Execution) {
    *r.model.hold.lock().unwrap() = Some("CHILD".into());
    let parent = parent_session(&r.core);
    turn(&r.core, &parent, ASK).await;
    let task = only_task(&r.core, &parent);
    r.entered.recv().await.unwrap();
    (parent, task)
}

/// Rule 1 and the report's line (rules 4 and 5): the owner's message during
/// the task's first call joins its next request, after the tool result, as
/// the owner's message, in one `turn.steered` row; the task finishes, reports
/// once, and the parent wakes once and reads that it was joined.
#[tokio::test]
async fn the_owners_message_steers_into_a_running_task_and_the_report_says_so() {
    let mut r = rig_with_child(|req| {
        let s = seen(req);
        match (s.contains("also check the tide table"), answers_a_call(req)) {
            (true, _) => Scripted::text("Surveyed, tide table included."),
            (false, false) => Scripted::tools(
                "Looking.",
                &[("t1", "fs_read", json!({"path": "missing.txt"}))],
            ),
            (false, true) => Scripted::text("Surveyed."),
        }
    });
    let (parent, task) = running(&mut r).await;
    let owner = say(&r.core, &task.session_id, "also check the tide table", true);
    until("the message is offered", || {
        r.core.runner.tools.job_waits.steers.any()
    })
    .await;
    r.model.gate.add_permits(10);
    let answered = owner.await.unwrap().unwrap();
    assert!(
        answered.output.ends_with("Surveyed, tide table included."),
        "answered by the running turn: {}",
        answered.output
    );
    until("the task completes", || {
        exec(&r.core, &task.id).state == ExecState::Complete
    })
    .await;
    assert_eq!(steers(&r.core), 1, "one steer, one frame");
    let calls = r.model.requests.lock().unwrap().clone();
    let child = calls
        .iter()
        .rfind(|c| first_user(c).contains("CHILD"))
        .unwrap();
    let last = seen(child);
    let (result, said) = (
        last.find("tool_result"),
        last.find("also check the tide table"),
    );
    assert!(
        result.is_some() && said.is_some() && result < said,
        "the message follows the tool result: {last}"
    );
    assert_eq!(exec(&r.core, &task.id).turns, 1, "no second turn ran");
    assert_eq!(posts(&r.core, "report").len(), 1, "one report");
    // The parent was woken by the report, once, and read the line.
    let pid = task.parent.clone().unwrap();
    until("the parent's report turn", || {
        let p = exec(&r.core, &pid);
        p.turns == 2 && p.state == ExecState::Waiting
    })
    .await;
    let last = crate::tests_tasks::last_user(r.model.requests.lock().unwrap().last().unwrap());
    assert!(last.contains("Surveyed, tide table included."), "{last}");
    assert!(last.contains("\n\n(you joined: 1 message)"), "{last}");
    tokio::time::sleep(Duration::from_millis(600)).await;
    assert_eq!(exec(&r.core, &pid).turns, 2, "the parent woke once");
    let _ = parent;
}

/// Rule 2: a message that comes during the task's last call, which makes no
/// tool calls, parks the task; the message wakes it, and that turn's message
/// is the report. Then the task has ended, and the next message is refused
/// as `execution_complete` (rule 3).
#[tokio::test]
async fn a_message_during_the_last_call_parks_the_task_and_the_next_turn_reports() {
    let mut r = rig_with_child(|req| match seen(req).contains("one more thing") {
        true => Scripted::text("Done, one more thing included."),
        false => Scripted::text("Done."),
    });
    let (_, task) = running(&mut r).await;
    let owner = say(&r.core, &task.session_id, "one more thing", true);
    until("the message is offered", || {
        r.core.runner.tools.job_waits.steers.any()
    })
    .await;
    r.model.gate.add_permits(10);
    let answered = owner.await.unwrap().unwrap();
    assert_eq!(
        answered.output, "Done, one more thing included.",
        "its own turn answered"
    );
    until("the task completes", || {
        exec(&r.core, &task.id).state == ExecState::Complete
    })
    .await;
    assert_eq!(
        exec(&r.core, &task.id).turns,
        2,
        "it parked, took the message, and reported"
    );
    assert_eq!(
        steers(&r.core),
        0,
        "taken as its turn's input, not as a steer"
    );
    let reports = posts(&r.core, "report");
    assert_eq!(reports.len(), 1, "reported once");
    let body = crate::outbox::body_of(&reports[0]);
    assert_eq!(
        r.core
            .outbox
            .said(body["node"].as_str().unwrap())
            .as_deref(),
        Some("Done, one more thing included."),
        "that turn's message is the report, not the first's"
    );
    // The message that woke it is one the owner joined with (the report's line).
    assert_eq!(
        r.core
            .store
            .get_meta::<u32>(&crate::task::joined_key(&task.session_id))
            .unwrap(),
        Some(1),
        "the turn the owner's message began counts"
    );
    // Rule 3: it stays ended.
    let late = say(&r.core, &task.session_id, "and another", true)
        .await
        .unwrap();
    assert_eq!(class_of(&late.unwrap_err()), "execution_complete");
    assert_eq!(posts(&r.core, "report").len(), 1, "and reports no more");
}

/// A guest's input is not steered: it queues behind the turn, and with the
/// task over it is refused; the task finished as if nobody had spoken.
#[tokio::test]
async fn a_guests_message_is_not_steered() {
    let mut r = rig_with_child(|req| match answers_a_call(req) {
        false => Scripted::tools(
            "Looking.",
            &[("t1", "fs_read", json!({"path": "missing.txt"}))],
        ),
        true => Scripted::text("Surveyed."),
    });
    let (_, task) = running(&mut r).await;
    let guest = say(&r.core, &task.session_id, "hi from a guest", false);
    tokio::time::sleep(Duration::from_millis(200)).await;
    assert!(
        !r.core.runner.tools.job_waits.steers.any(),
        "nothing was offered"
    );
    r.model.gate.add_permits(10);
    until("the task completes", || {
        exec(&r.core, &task.id).state == ExecState::Complete
    })
    .await;
    assert_eq!(
        class_of(&guest.await.unwrap().unwrap_err()),
        "execution_complete"
    );
    assert_eq!(steers(&r.core), 0);
    assert!(!seen(r.model.requests.lock().unwrap().last().unwrap()).contains("hi from a guest"));
}
