//! A fan-out on Ops (theseus-z9mze.6): each helper's task and budget row carry the call that opened it
//! (`fan_out`, the only marker of a helper), and the turn's wait writes `task.wait.started` and
//! `task.wait.ended`, which Ops reads for "waiting on n tasks". A task that is no helper carries
//! neither.

use serde_json::json;
use std::sync::{Arc, Mutex, OnceLock};
use theseus_protocol::BudgetRow;

use crate::provider::Scripted;
use crate::tests_judge::kinds;
use crate::tests_task_fan_out::{fan_rig, helpers, tasks_of, Call};
use crate::tests_tasks::{parent_session, turn, ASK};

/// The parent's row of the budget tree, with the helpers open under it.
fn tree(core: &crate::Core, parent: &str) -> BudgetRow {
    core.budget_list()
        .unwrap()
        .executions
        .into_iter()
        .find(|r| r.session_id == parent)
        .expect("the parent's row")
}

/// Two helpers answer and the call waits for them: the tasks and the budget rows say which call opened
/// them, the parent's own row says none, and the wait is two rows with the figures of how it ended.
#[tokio::test]
async fn a_fan_outs_helpers_carry_its_call_and_its_wait_writes_two_rows() {
    let call: Call = Arc::new(Mutex::new(
        json!({"helpers": helpers(2), "budget_usd": 6.0}),
    ));
    // A helper's request is made while its task is open: the budget tree is read there, as Ops reads it.
    let core: Arc<OnceLock<Arc<crate::Core>>> = Arc::default();
    let during: Arc<Mutex<Option<(String, BudgetRow)>>> = Arc::default();
    let (c2, d2) = (core.clone(), during.clone());
    let r = fan_rig(
        call,
        move |_| {
            // The guard is dropped before the row is stored: it is not re-entrant.
            let parent = d2.lock().unwrap().as_ref().map(|p| p.0.clone());
            if let (Some(core), Some(parent)) = (c2.get(), parent) {
                let row = tree(core, &parent);
                d2.lock().unwrap().as_mut().unwrap().1 = row;
            }
            Scripted::text("x.rs:1 — f — bug — fix")
        },
        |_| {},
    );
    let parent = parent_session(&r.core);
    core.set(r.core.clone()).ok();
    *during.lock().unwrap() = Some((parent.clone(), BudgetRow::default()));
    turn(&r.core, &parent, ASK).await;

    let tasks = tasks_of(&r.core, &parent);
    assert_eq!(tasks.len(), 2);
    let group = tasks[0].helper_of().expect("a helper").to_string();

    // The task list: each helper names the call, from the kernel's own mark.
    for e in &tasks {
        let rec: Option<crate::session::SessionRecord> =
            r.core.store.get_session(&e.session_id).unwrap();
        let info = crate::task::info(e, rec.as_ref(), 0);
        assert_eq!(info.fan_out.as_deref(), Some(group.as_str()), "{}", e.id);
    }

    // The budget tree, as it stood while a helper ran: the helper open under the parent says the
    // call, and the parent says none.
    let (_, top) = during.lock().unwrap().clone().unwrap();
    assert_eq!(top.fan_out, None);
    assert!(
        !top.tasks.is_empty(),
        "a helper is open under the parent: {top:?}"
    );
    assert!(top
        .tasks
        .iter()
        .all(|k| k.fan_out.as_deref() == Some(group.as_str())));

    // The wait: one started row, one ended row, naming the call.
    let started = kinds(&r.core.store, "task.wait.started");
    let ended = kinds(&r.core.store, "task.wait.ended");
    assert_eq!((started.len(), ended.len()), (1, 1));
    let (s, e) = (&started[0].data, &ended[0].data);
    assert_eq!(
        (s["tasks"].as_u64(), s["fan_out"].as_str()),
        (Some(2), Some(group.as_str()))
    );
    assert!(s["timeout_s"].as_u64().unwrap() >= 30, "{s}");
    assert_eq!(
        (
            e["tasks"].as_u64(),
            e["delivered"].as_u64(),
            e["running"].as_u64()
        ),
        (Some(2), Some(2), Some(0)),
        "{e}"
    );
    for k in ["failed", "cancelled", "budget"] {
        assert_eq!(e[k].as_u64(), Some(0), "{k}: {e}");
    }
    assert_eq!(
        (e["cut"].as_bool(), e["fan_out"].as_str()),
        (Some(false), Some(group.as_str()))
    );
    assert!(e["waited_ms"].is_u64());
    assert_eq!(started[0].turn_id, ended[0].turn_id, "one turn's wait");
}

/// A plain task has no call, on either surface.
#[tokio::test]
async fn a_task_that_is_no_helper_carries_no_call() {
    let r = crate::tests_tasks::rig(
        |req| {
            if crate::tests_tasks::first_user(req).contains("CHILD:") {
                return Scripted::text("done");
            }
            if crate::tests_tasks::answers_a_call(req) {
                return Scripted::text("Started.");
            }
            Scripted::tools(
                "Starting.",
                &[(
                    "t_1",
                    "task_create",
                    json!({"brief": "CHILD: plain", "budget_usd": 1.0,
                           "arrangement": crate::tests_tasks::arranged()}),
                )],
            )
        },
        |_| {},
    );
    let parent = parent_session(&r.core);
    turn(&r.core, &parent, ASK).await;
    let tasks = tasks_of(&r.core, &parent);
    let rec: Option<crate::session::SessionRecord> =
        r.core.store.get_session(&tasks[0].session_id).unwrap();
    assert_eq!(crate::task::info(&tasks[0], rec.as_ref(), 0).fan_out, None);
    assert!(tree(&r.core, &parent)
        .tasks
        .iter()
        .all(|k| k.fan_out.is_none()));
    assert!(kinds(&r.core.store, "task.wait.started").is_empty());
}
