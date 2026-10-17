//! The owner holds a task (`task.pause`) and lets it go on (`task.resume`)
//! through whole cores: the task parks at its next loop and never reports, a
//! model call in flight is never cut, resume wakes it, the hold survives a
//! restart, the work board marks it, and only the owner holds.

use std::sync::Arc;

use serde_json::json;
use theseus_kernel::ExecState;

use crate::approval::{Answerer, Surface};
use crate::provider::{FakeProvider, Scripted};
use crate::rpc::Parts;
use crate::tests_tasks::{
    answers_a_call, arranged, exec, first_user, last_user, only_task, parent_session, posts, rig,
    turn, until, Rig, ASK,
};
use crate::Core;

fn cli() -> Answerer {
    Answerer {
        label: "the CLI".into(),
        surface: Surface::Cli,
        discord: None,
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
    }
}

/// A conversation that starts one task: the task reads a file (its first
/// loop), then, once it has the result, or when it is told it was resumed,
/// reports.
fn rig_with_a_task() -> Rig {
    let r = rig(
        |req| {
            let (first, last) = (first_user(req), last_user(req));
            if first.contains("CHILD:") {
                return if answers_a_call(req) || last.contains("resumed by the owner") {
                    Scripted::text("Counted to three.")
                } else {
                    Scripted::tools("Looking.", &[("c1", "proc_run", json!({"argv": ["true"]}))])
                };
            }
            if answers_a_call(req) {
                return Scripted::text("Started it.");
            }
            if last.starts_with("START") {
                return Scripted::tools(
                    "Starting it.",
                    &[(
                        "t_task",
                        "task_create",
                        json!({"brief": "CHILD: count to three", "budget_usd": 1.5,
                               "arrangement": arranged()}),
                    )],
                );
            }
            Scripted::text("Fine.")
        },
        |_| {},
    );
    *r.model.hold.lock().unwrap() = Some("CHILD".into());
    r
}

fn child_calls(r: &Rig) -> usize {
    r.model
        .requests
        .lock()
        .unwrap()
        .iter()
        .filter(|q| first_user(q).contains("CHILD:"))
        .count()
}

/// A copy of `c`'s store taken now, opened by a second core.
fn restarted(c: &Core) -> Arc<Core> {
    let from = std::path::Path::new(&c.cfg.server.state_dir).join("store");
    let to = std::path::Path::new(&c.cfg.server.state_dir).join("store-restarted");
    assert!(std::process::Command::new("cp")
        .arg("-r")
        .arg(&from)
        .arg(&to)
        .status()
        .unwrap()
        .success());
    let store = crate::store::Store::open(&to).unwrap();
    Core::build(Parts::for_tests(
        (*c.cfg).clone(),
        Arc::new(FakeProvider::default()),
        store,
    ))
    .unwrap()
}

#[tokio::test]
async fn a_held_task_parks_at_its_next_loop_never_reports_and_resume_goes_on() {
    let mut r = rig_with_a_task();
    r.core.push.ensure(&r.core).await.unwrap();
    let parent = parent_session(&r.core);
    turn(&r.core, &parent, ASK).await;
    let task = only_task(&r.core, &parent);
    // Its first call is in flight when the owner holds it: never cut.
    r.entered.recv().await.unwrap();
    let held = r.core.task_hold(&task.id, true, &cli()).unwrap();
    assert!(held.paused && held.changed, "{held:?}");
    r.model.gate.add_permits(1);
    // The call answers, its tool runs, and at the next loop the turn parks
    // on input instead of calling the model again or reporting.
    until("the task parks", || {
        exec(&r.core, &task.id).state == ExecState::Waiting
    })
    .await;
    tokio::time::sleep(std::time::Duration::from_millis(600)).await;
    assert_eq!(exec(&r.core, &task.id).state, ExecState::Waiting);
    assert_eq!(child_calls(&r), 1, "no second model call while it is held");
    assert!(
        posts(&r.core, "report").is_empty(),
        "a held task reports nothing"
    );
    let w = r.core.push.work_all().remove(&task.id).unwrap();
    assert!(w.paused, "the work board marks it: {w:?}");
    // Holding it again changes nothing.
    assert!(!r.core.task_hold(&task.id, true, &cli()).unwrap().changed);
    // Resume wakes it with one owner node, and it goes on to report.
    r.model.gate.add_permits(1);
    let back = r.core.task_hold(&task.id, false, &cli()).unwrap();
    assert!(!back.paused && back.changed, "{back:?}");
    until("the task completes", || {
        exec(&r.core, &task.id).state == ExecState::Complete
    })
    .await;
    assert_eq!(child_calls(&r), 2);
    let reports = posts(&r.core, "report");
    assert_eq!(reports.len(), 1, "one report, once it is let go");
    assert!(!r.core.push.work_all().remove(&task.id).unwrap().paused);
}

#[tokio::test]
async fn only_the_owner_holds_and_an_ended_task_cannot_be() {
    let mut r = rig_with_a_task();
    let parent = parent_session(&r.core);
    turn(&r.core, &parent, ASK).await;
    let task = only_task(&r.core, &parent);
    r.entered.recv().await.unwrap();
    let refused = r.core.task_hold(&task.id, true, &stranger()).unwrap_err();
    assert!(
        format!("{refused:#}").contains("owner") || format!("{refused:#}").contains("private"),
        "{refused:#}"
    );
    assert!(
        r.core
            .store
            .get_meta::<bool>(&format!("task.paused.{}", task.session_id))
            .unwrap()
            .is_none(),
        "a refused hold writes nothing"
    );
    r.model.gate.add_permits(2);
    until("the task completes", || {
        exec(&r.core, &task.id).state == ExecState::Complete
    })
    .await;
    let ended = r.core.task_hold(&task.id, true, &cli()).unwrap_err();
    assert!(format!("{ended:#}").contains("ended"), "{ended:#}");
}

#[tokio::test]
async fn the_hold_survives_a_restart() {
    let mut r = rig_with_a_task();
    let parent = parent_session(&r.core);
    turn(&r.core, &parent, ASK).await;
    let task = only_task(&r.core, &parent);
    r.entered.recv().await.unwrap();
    r.core.task_hold(&task.id, true, &cli()).unwrap();
    assert_eq!(
        r.core
            .store
            .get_meta::<bool>(&format!("task.paused.{}", task.session_id))
            .unwrap(),
        Some(true)
    );
    let again = restarted(&r.core);
    assert!(
        again.runner.task_paused(&task.session_id),
        "read back from the key"
    );
    assert!(!again.runner.task_paused("ses_someone_else"));
    r.model.gate.add_permits(1);
}
