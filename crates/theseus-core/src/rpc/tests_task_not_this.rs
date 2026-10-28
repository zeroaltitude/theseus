//! `task.not_this` through whole cores: the task ends, one rolling
//! preference is left however many times the owner says it (the last one's),
//! a task that has ended is answered, and only the owner says it.

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;

use serde_json::json;
use theseus_kernel::ExecState;
use theseus_protocol::task_not_this::{TaskNotThisParams, SOURCE};

use crate::approval::{Answerer, Surface};
use crate::provider::Scripted;
use crate::tests_tasks::{
    answers_a_call, arranged, exec, first_user, only_task, parent_session, rig, turn, Rig, ASK,
};
use crate::Core;

fn cli() -> Answerer {
    Answerer {
        label: "the CLI".into(),
        surface: Surface::Cli,
        discord: None,
        agent: None,
    }
}

/// A person in a shared channel nobody bound as private.
fn stranger() -> Answerer {
    Answerer {
        label: "discord:777".into(),
        surface: Surface::Discord,
        discord: Some(theseus_protocol::DiscordOrigin {
            user_id: "777".into(),
            channel_id: "9001".into(),
            guild_id: Some("7".into()),
        }),
        agent: None,
    }
}

/// A conversation whose every turn starts one task, whose brief is named by
/// how many it has started (`CHILD: alpha`, then `CHILD: beta`), and which
/// the model holds in its first call, so each task is running when it is told.
fn rig_with_tasks() -> Rig {
    let started = Arc::new(AtomicUsize::new(0));
    let r = rig(
        move |req| {
            if first_user(req).contains("CHILD:") {
                return Scripted::text("finished anyway");
            }
            if answers_a_call(req) {
                return Scripted::text("Started it.");
            }
            let name = ["alpha", "beta", "gamma"][started.fetch_add(1, Ordering::SeqCst) % 3];
            Scripted::tools(
                "Starting it.",
                &[(
                    "t_task",
                    "task_create",
                    json!({"brief": format!("CHILD: {name}"), "budget_usd": 1.5,
                           "arrangement": arranged()}),
                )],
            )
        },
        |_| {},
    );
    *r.model.hold.lock().unwrap() = Some("CHILD".into());
    r
}

/// A task started and running: its parent's turn done, its first call held.
async fn a_running_task(r: &mut Rig) -> theseus_kernel::Execution {
    let parent = parent_session(&r.core);
    turn(&r.core, &parent, ASK).await;
    let task = only_task(&r.core, &parent);
    r.entered.recv().await.unwrap();
    task
}

fn params(task: &str, reason: Option<&str>) -> TaskNotThisParams {
    TaskNotThisParams {
        task: task.into(),
        reason: reason.map(Into::into),
        author: None,
        discord: None,
    }
}

/// The live preferences `task.not_this` wrote.
fn rolling(core: &Core) -> Vec<theseus_protocol::StandingRecord> {
    let all = core.runner.standing.all(&core.store).unwrap();
    crate::standing::live(&all, None, theseus_protocol::now_unix_ms())
        .into_iter()
        .filter(|r| r.source == SOURCE)
        .cloned()
        .collect()
}

#[tokio::test]
async fn not_this_cancels_the_task_and_writes_a_preference() {
    let mut r = rig_with_tasks();
    let task = a_running_task(&mut r).await;
    let said = r
        .core
        .task_not_this(&params(&task.id, Some("too noisy")), cli())
        .await
        .unwrap();
    assert_eq!(said.task.state, "cancelled", "{said:?}");
    assert_eq!(exec(&r.core, &task.id).state, ExecState::Cancelled);
    let live = rolling(&r.core);
    assert_eq!(live.len(), 1, "{live:?}");
    assert_eq!(live[0].id, said.preference.id);
    assert!(
        live[0].text.contains("alpha") && live[0].text.contains("too noisy"),
        "{}",
        live[0].text
    );
    assert_eq!(said.replaced, None);
}

/// Rolling is one: the second call ends the first's preference and is the
/// one that survives.
#[tokio::test]
async fn two_calls_leave_one_preference_and_it_is_the_seconds() {
    let mut r = rig_with_tasks();
    let first = a_running_task(&mut r).await;
    let one = r
        .core
        .task_not_this(&params(&first.id, None), cli())
        .await
        .unwrap();
    let second = a_running_task(&mut r).await;
    let two = r
        .core
        .task_not_this(&params(&second.id, Some("not again")), cli())
        .await
        .unwrap();
    let live = rolling(&r.core);
    assert_eq!(live.len(), 1, "one rolling preference, not two: {live:?}");
    assert_eq!(live[0].id, two.preference.id);
    assert!(live[0].text.contains("beta"), "{}", live[0].text);
    assert!(!live[0].text.contains("alpha"), "{}", live[0].text);
    assert_eq!(two.replaced.as_deref(), Some(one.preference.id.as_str()));
}

#[tokio::test]
async fn not_this_on_an_ended_task_is_answered_not_refused() {
    let mut r = rig_with_tasks();
    let task = a_running_task(&mut r).await;
    r.core.task_cancel_by(&task.id, "the CLI").await.unwrap();
    let said = r
        .core
        .task_not_this(&params(&task.id, None), cli())
        .await
        .unwrap();
    assert_eq!(said.task.state, "cancelled");
    assert_eq!(rolling(&r.core).len(), 1);
    // And once more: the same words, the same one record.
    let again = r
        .core
        .task_not_this(&params(&task.id, None), cli())
        .await
        .unwrap();
    assert_eq!(again.preference.id, said.preference.id);
    assert_eq!(rolling(&r.core).len(), 1);
}

#[tokio::test]
async fn a_stranger_cancels_nothing_and_writes_nothing() {
    let mut r = rig_with_tasks();
    let task = a_running_task(&mut r).await;
    let refused = r
        .core
        .task_not_this(&params(&task.id, None), stranger())
        .await;
    assert!(refused.is_err());
    assert_ne!(exec(&r.core, &task.id).state, ExecState::Cancelled);
    assert!(rolling(&r.core).is_empty());
}

#[tokio::test]
async fn an_unknown_task_is_not_found_and_writes_nothing() {
    let r = rig_with_tasks();
    let e = r
        .core
        .task_not_this(&params("nosuch", None), cli())
        .await
        .unwrap_err();
    assert_eq!(e.code, theseus_protocol::error_code::NOT_FOUND, "{e:?}");
    assert!(rolling(&r.core).is_empty());
}

#[test]
fn the_text_is_one_line_with_the_shape_and_the_reason() {
    let t = super::preference_text("sweep\nthe   logs", Some("  no\nmore "));
    assert!(!t.contains('\n'), "{t}");
    assert!(
        t.contains("\"sweep the logs\"") && t.ends_with("Their reason: no more"),
        "{t}"
    );
    let long = super::preference_text(&"x".repeat(5000), Some(&"y".repeat(5000)));
    assert!(
        long.chars().count() < crate::standing::MAX_TEXT,
        "{}",
        long.len()
    );
}
