//! A deferred task through the whole core (theseus-rcgph): `task.create` with
//! `start_after` opens it asleep, carves its budget at once, runs no turn until
//! it is due, starts then with the note that says so, and cancels like any
//! task before it starts.

use std::time::Duration;

use serde_json::{json, Value};
use theseus_kernel::{ExecState, Wake};

use crate::provider::Scripted;
use crate::tests_tasks::{
    answers_a_call, arranged, exec, first_user, last_user, only_task, parent_session, posts, rig,
    turn, until, Rig, ASK,
};

/// A parent that answers `ASK` with a `task_create` of `CHILD: <what>` and
/// the given extra input, and a child that says it is done.
fn rigged(extra: Value) -> Rig {
    rig(
        move |req| {
            if first_user(req).contains("CHILD") {
                return Scripted::text("Drafted it.");
            }
            if answers_a_call(req) {
                return Scripted::text("Scheduled it.");
            }
            if last_user(req).starts_with("START") {
                let mut input = json!({"brief": "CHILD: draft the release notes",
                                       "budget_usd": 1.5, "arrangement": arranged()});
                for (k, v) in extra.as_object().unwrap() {
                    input[k] = v.clone();
                }
                return Scripted::tools("Scheduling.", &[("t_task", "task_create", input)]);
            }
            Scripted::text("ok")
        },
        |_| {},
    )
}

/// Whether any request to the model so far was the child's.
fn child_ran(r: &Rig) -> bool {
    r.model
        .requests
        .lock()
        .unwrap()
        .iter()
        .any(|q| first_user(q).contains("CHILD"))
}

/// The tool result the parent's last request carried.
fn tool_result(r: &Rig) -> String {
    let req = r.model.requests.lock().unwrap().last().cloned().unwrap();
    serde_json::to_string(&req.messages).unwrap()
}

/// Asleep from the open (its carve held, no turn, its start on `task.list`
/// and the board), started once when due with the note that says so, and its
/// report read once.
#[tokio::test]
async fn a_deferred_task_sleeps_holds_its_carve_and_starts_when_due() {
    let r = rigged(json!({"start_after": "2s"}));
    let parent = parent_session(&r.core);
    turn(&r.core, &parent, ASK).await;
    let task = only_task(&r.core, &parent);
    let Some(Wake::DueAt { at_ms }) = task.wake.clone() else {
        panic!("not asleep: {task:?}");
    };
    assert_eq!((task.state, task.turns), (ExecState::Waiting, 0));
    assert!(!child_ran(&r), "no turn before it is due");
    let seen = tool_result(&r);
    assert!(seen.contains("Scheduled task"), "{seen}");
    // The carve is the parent's at once, so it cannot spend it meanwhile.
    let pe = crate::tests_tasks::parent_exec(&r.core, &parent);
    assert_eq!(pe.budget.reserved_micros, 1_500_000);
    assert!(pe
        .budget
        .reservations
        .contains_key(&theseus_kernel::carve_key(&task.id)));
    // Seen: the list says when it starts, and the board says so.
    let listed = r.core.tasks(Some(&parent), None).unwrap();
    assert_eq!(listed.len(), 1);
    assert_eq!(listed[0].starts_at_ms, Some(at_ms));
    let label = listed[0].attention.as_ref().unwrap().label.clone();
    assert!(label.starts_with("▷ starts "), "{label}");
    // It starts when due, and not before.
    tokio::time::sleep(Duration::from_millis(600)).await;
    assert!(!child_ran(&r), "still asleep before its time");
    until("the child's report", || {
        exec(&r.core, &task.id).state == ExecState::Complete
    })
    .await;
    let req = r
        .model
        .requests
        .lock()
        .unwrap()
        .iter()
        .find(|q| first_user(q).contains("CHILD"))
        .cloned()
        .unwrap();
    let seen = serde_json::to_string(&req.messages).unwrap();
    assert!(seen.contains("Deferred task, set "), "{seen}");
    assert!(seen.contains("; started "), "{seen}");
    assert_eq!(
        seen.matches("Deferred task, set ").count(),
        1,
        "the note is written once"
    );
    assert_eq!(posts(&r.core, "report").len(), 1);
    let done = r.core.tasks(Some(&parent), None).unwrap();
    assert_eq!(done[0].starts_at_ms, None, "it has started");
    let rec = r.core.store.session_nodes(&task.session_id).unwrap();
    assert_eq!(
        rec.iter()
            .filter(|(_, n)| n.author.as_deref() == Some("harness:deferred"))
            .count(),
        1
    );
}

/// A start whose due time fell before this process began says the daemon was
/// not running then.
#[tokio::test]
async fn a_start_that_fell_due_while_the_daemon_was_down_says_so() {
    let r = rigged(json!({"start_after": "2s"}));
    let parent = parent_session(&r.core);
    turn(&r.core, &parent, ASK).await;
    let task = only_task(&r.core, &parent);
    // Its due time moves to an hour before this process began.
    let began = r.core.kernel.process_started_ms();
    assert!(began > 0);
    let mut d: crate::task_start::Deferred = r
        .core
        .store
        .get_meta(&crate::task_start::key(&task.session_id))
        .unwrap()
        .unwrap();
    d.due_at_ms = began - 3_600_000;
    r.core
        .store
        .append(&[crate::task_start::record(&task.session_id, &d).unwrap()])
        .unwrap();
    until("the child's report", || {
        exec(&r.core, &task.id).state == ExecState::Complete
    })
    .await;
    let nodes = r.core.store.session_nodes(&task.session_id).unwrap();
    let note = nodes
        .iter()
        .find(|(_, n)| n.author.as_deref() == Some("harness:deferred"))
        .map(|(_, n)| format!("{:?}", n.body))
        .unwrap();
    assert!(
        note.contains("late: the daemon was not running then"),
        "{note}"
    );
}

/// Cancelled before its start: its carve comes back, no turn ever runs, and
/// the time passing queues nothing.
#[tokio::test]
async fn a_cancel_before_the_start_releases_the_carve_and_no_turn_runs() {
    let r = rigged(json!({"start_after": "1s"}));
    let parent = parent_session(&r.core);
    turn(&r.core, &parent, ASK).await;
    let task = only_task(&r.core, &parent);
    let res = r
        .core
        .task_cancel_by(
            &crate::task::short(&task.session_id),
            "discord:zeroaltitude",
        )
        .await
        .unwrap();
    assert_eq!(res.task.state, "cancelled");
    let pe = crate::tests_tasks::parent_exec(&r.core, &parent);
    assert_eq!(pe.budget.reserved_micros, 0, "the carve is released");
    tokio::time::sleep(Duration::from_millis(1_800)).await;
    assert!(!child_ran(&r), "no turn ever runs");
    let t = exec(&r.core, &task.id);
    assert_eq!((t.state, t.turns), (ExecState::Cancelled, 0));
}

/// A start the tool cannot honour is refused in words, and opens nothing.
#[tokio::test]
async fn a_start_past_the_furthest_or_under_a_second_is_refused_in_words() {
    for (extra, words) in [
        (json!({"start_after": "366d"}), "at most 365 days from now"),
        (json!({"start_after": "0s"}), "at least 1 s from now"),
        (
            json!({"start_at": "2020-01-01T00:00:00Z"}),
            "`start_at` (2020-01-01T00:00:00Z) is",
        ),
        (
            json!({"start_at": "2099-01-01T00:00:00Z", "start_after": "1h"}),
            "not both",
        ),
    ] {
        let r = rigged(extra);
        let parent = parent_session(&r.core);
        turn(&r.core, &parent, ASK).await;
        let seen = tool_result(&r);
        assert!(seen.contains(words), "{words}: {seen}");
        let rec: crate::session::SessionRecord =
            r.core.store.get_session(&parent).unwrap().unwrap();
        assert!(
            r.core
                .kernel
                .tasks(rec.execution_id.as_deref())
                .unwrap()
                .is_empty(),
            "{words}: nothing opened"
        );
        assert_eq!(
            crate::tests_tasks::parent_exec(&r.core, &parent)
                .budget
                .reserved_micros,
            0
        );
    }
}
