//! The owner's end of a project (issue 37; `rpc/project_close.rs`): the project, its open items and its
//! milestones end in one go, its weekly review and its reminders stop, a task that carries any of it stops
//! the close, a shared place and an MCP client are refused, and the loop's own container is kept out of
//! the plans' list. A model's own close is `tests_projects`'.

use std::sync::Arc;

use serde_json::{json, Value};
use theseus_kernel::Purpose;
use theseus_protocol::tasks::{TaskClaim, TaskKind, TaskRecord, TaskState};

use crate::approval::{Answerer, Surface};
use crate::tests_mcp_memory::rpc as rpc_on;
use crate::tests_task_graph::{rig, rows, Rig};
use crate::{task_graph as graph, Core};

async fn rpc(core: &Arc<Core>, method: &str, params: Value) -> Result<Value, String> {
    rpc_on(core, Surface::Cli, method, params)
        .await
        .map_err(|e| e.message)
}

/// A project with two dated milestones and a review, and two open items under it.
async fn project(r: &Rig) -> String {
    let p = rpc(
        &r.core,
        "project.create",
        json!({
            "title": "Harbour survey",
            "milestones": [
                {"title": "Survey the moorings", "due": "1h"},
                {"title": "Chart the channel", "due": "2h"},
            ],
        }),
    )
    .await
    .unwrap();
    let id = p["project"]["id"].as_str().unwrap().to_string();
    for title in ["Order the sounding line", "Book the boat"] {
        rpc(
            &r.core,
            "project.item.add",
            json!({"project": id, "title": title}),
        )
        .await
        .unwrap();
    }
    id
}

fn reminders(core: &Core) -> usize {
    core.kernel
        .pending_wakes()
        .unwrap()
        .into_iter()
        .filter(|(_, w)| w.purpose == Purpose::Remind)
        .count()
}

async fn routines(core: &Arc<Core>) -> usize {
    rpc(core, "routine.list", json!({})).await.unwrap()["routines"]
        .as_array()
        .unwrap()
        .len()
}

fn below(r: &Rig, id: &str) -> Vec<TaskRecord> {
    r.tasks()
        .into_iter()
        .filter(|t| t.parent.as_deref() == Some(id))
        .collect()
}

/// A task that really runs (its own execution, held open), made a child of `parent` as a plan's task is.
async fn carry(r: &Rig, parent: &str) -> TaskRecord {
    r.model
        .hold_tasks
        .store(true, std::sync::atomic::Ordering::SeqCst);
    let started = rpc(&r.core, "task.start", json!({"brief": "Sound the channel"}))
        .await
        .unwrap();
    let session = started["session_id"].as_str().unwrap().to_string();
    let mut t = r
        .tasks()
        .into_iter()
        .find(|t| t.session.as_deref() == Some(session.as_str()))
        .expect("the task's record");
    t.parent = Some(parent.into());
    t.version += 1;
    r.core.store.append(&[graph::record(&t).unwrap()]).unwrap();
    t
}

/// The close ends the project, both items and both milestones as dropped, in the states a model's own
/// abandon writes, ends the review routine, cancels the four reminders, and says what it did; every row
/// is the owner's and names the way.
#[tokio::test(flavor = "multi_thread")]
async fn the_owner_drops_a_project_and_everything_it_set_going_stops() {
    let r = rig();
    let id = project(&r).await;
    assert_eq!((reminders(&r.core), routines(&r.core).await), (4, 1));
    let review = r.task(&id).review.unwrap();
    let said = rpc(
        &r.core,
        "project.close",
        json!({"project": id, "how": "dropped", "reason": "the harbour moved"}),
    )
    .await
    .unwrap();
    assert_eq!(said["how"], "dropped");
    assert_eq!(said["items_ended"], 2, "{said}");
    assert_eq!(said["milestones_ended"], 2, "{said}");
    assert_eq!(said["review_stopped"], review.as_str(), "{said}");
    assert_eq!(said["reminders_cancelled"], 4, "{said}");
    assert_eq!(said["project"]["state"], "abandoned");

    let project = r.task(&id);
    assert_eq!(project.state, TaskState::Abandoned);
    assert_eq!(project.version, 2);
    assert_eq!(
        project.evidence.last().unwrap().note.as_deref(),
        Some("the harbour moved")
    );
    let kids = below(&r, &id);
    assert_eq!(kids.len(), 4);
    assert!(kids.iter().all(|k| k.state == TaskState::Abandoned));
    // One frame: every record carries the one moment.
    assert!(kids
        .iter()
        .all(|k| k.updated_at_ms == project.updated_at_ms));
    assert_eq!((reminders(&r.core), routines(&r.core).await), (0, 0));

    let closed = rows(&r.core, "task.closed");
    assert_eq!(closed.len(), 5, "{closed:?}");
    assert!(closed.iter().all(|row| row.data["how"] == "dropped"
        && row.data["via"] == "cli"
        && row.data["by"].is_string()));
    // The open list no longer holds it, the whole list does.
    let open = rpc(&r.core, "project.list", json!({})).await.unwrap();
    assert!(open["projects"].as_array().unwrap().is_empty(), "{open}");
    let all = rpc(&r.core, "project.list", json!({"all": true}))
        .await
        .unwrap();
    assert_eq!(all["projects"][0]["state"], "abandoned");
    assert!(r.core.projects_upto(10).unwrap().0.is_empty());
}

/// Done writes `done` to the same records.
#[tokio::test(flavor = "multi_thread")]
async fn the_owner_marks_a_project_done() {
    let r = rig();
    let id = project(&r).await;
    let said = rpc(&r.core, "project.close", json!({"project": id}))
        .await
        .unwrap();
    assert_eq!(said["how"], "done");
    assert_eq!(r.task(&id).state, TaskState::Done);
    assert!(below(&r, &id).iter().all(|k| k.state == TaskState::Done));
    assert_eq!((reminders(&r.core), routines(&r.core).await), (0, 0));
}

/// A task that is running under the project stops the close, named, and nothing ends or stops; once it is
/// cancelled the close goes through, though its record stays open to be resumed.
#[tokio::test(flavor = "multi_thread")]
async fn a_task_that_carries_part_of_the_project_stops_the_close() {
    let r = rig();
    let id = project(&r).await;
    let t = carry(&r, &id).await;
    let e = rpc(&r.core, "project.close", json!({"project": id}))
        .await
        .unwrap_err();
    assert!(
        e.contains(&t.id[t.id.len() - 6..]) && e.contains(&t.title),
        "{e}"
    );
    assert!(e.contains("cancel"), "{e}");
    assert_eq!(r.task(&id).state, TaskState::Accepted);
    assert!(below(&r, &id)
        .iter()
        .filter(|k| k.id != t.id)
        .all(|k| k.state == TaskState::Accepted));
    assert_eq!((reminders(&r.core), routines(&r.core).await), (4, 1));
    rpc(
        &r.core,
        "task.cancel",
        json!({"task": t.session.clone().unwrap()}),
    )
    .await
    .unwrap();
    rpc(&r.core, "project.close", json!({"project": id}))
        .await
        .unwrap();
    assert_eq!(r.task(&id).state, TaskState::Done);
}

/// A running task that holds a claim on one of the project's steps stops the close, and the refusal names
/// that task, not the step; once it is cancelled its claim carries nothing, though the lease has not lapsed.
#[tokio::test(flavor = "multi_thread")]
async fn a_task_that_holds_a_claim_on_a_step_stops_the_close() {
    let r = rig();
    let id = project(&r).await;
    r.model
        .hold_tasks
        .store(true, std::sync::atomic::Ordering::SeqCst);
    let started = rpc(&r.core, "task.start", json!({"brief": "Sound the channel"}))
        .await
        .unwrap();
    let session = started["session_id"].as_str().unwrap().to_string();
    let exec = session.replacen("ses_", "exe_", 1);
    let mut step = below(&r, &id)
        .into_iter()
        .find(|t| t.kind == TaskKind::Item)
        .unwrap();
    step.claim = Some(TaskClaim {
        by: exec,
        session: session.clone(),
        until_ms: r.core.kernel.now_ms() + 1_800_000,
    });
    step.version += 1;
    r.core
        .store
        .append(&[graph::record(&step).unwrap()])
        .unwrap();
    let e = rpc(&r.core, "project.close", json!({"project": id}))
        .await
        .unwrap_err();
    let tail = &session[session.len() - 6..];
    assert!(e.contains(tail) && e.contains("Sound the channel"), "{e}");
    assert!(e.contains(&format!("theseus cancel {tail}")), "{e}");
    assert!(
        !e.contains(&step.id[step.id.len() - 6..]),
        "names the holder, not the step: {e}"
    );
    assert_eq!(r.task(&id).state, TaskState::Accepted);
    rpc(&r.core, "task.cancel", json!({"task": session}))
        .await
        .unwrap();
    rpc(&r.core, "project.close", json!({"project": id}))
        .await
        .unwrap();
    assert_eq!(r.task(&id).state, TaskState::Done);
    assert!(r.task(&step.id).claim.is_none());
}

/// A shared place is turned away, and the refusal is ledgered; nothing ends.
#[tokio::test(flavor = "multi_thread")]
async fn a_shared_place_cannot_close_a_project() {
    let r = rig();
    let id = project(&r).await;
    const LAB: u64 = 314_159_265_358_979_323;
    r.core.bind_places(vec![crate::places::BoundPlace {
        target: format!("discord:channel:{LAB}"),
        name: "#lab".into(),
        private: false,
        ..Default::default()
    }]);
    let stranger = Answerer {
        label: "discord:777".into(),
        surface: Surface::Discord,
        discord: Some(theseus_protocol::DiscordOrigin {
            user_id: "777".into(),
            channel_id: LAB.to_string(),
            guild_id: Some("7".into()),
        }),
        agent: None,
    };
    let p = theseus_protocol::projects::ProjectCloseParams {
        project: id.clone(),
        ..Default::default()
    };
    let e = r.core.project_close(&p, stranger).await.unwrap_err();
    assert_eq!(e.code, theseus_protocol::error_code::REFUSED, "{e:?}");
    assert_eq!(r.task(&id).state, TaskState::Accepted);
    assert_eq!((reminders(&r.core), routines(&r.core).await), (4, 1));
    let refusals = rows(&r.core, "approval.refused");
    assert_eq!(refusals.len(), 1, "{refusals:?}");
    assert_eq!(refusals[0].data["act"], "project.close");
}

/// An MCP client has no way to close a project.
#[tokio::test(flavor = "multi_thread")]
async fn an_mcp_client_cannot_close_a_project() {
    let r = rig();
    let id = project(&r).await;
    let e = rpc_on(
        &r.core,
        Surface::Mcp,
        "project.close",
        json!({"project": id}),
    )
    .await
    .unwrap_err();
    assert!(!e.message.is_empty());
    assert_eq!(r.task(&id).state, TaskState::Accepted);
    assert_eq!((reminders(&r.core), routines(&r.core).await), (4, 1));
    assert!(!crate::mcp_server::ALLOWED.contains(&"project.close"));
    assert!(!crate::mcp_server::JUDGED.contains(&"project.close"));
}

/// A closed project is not closed again, and nothing is reopened.
#[tokio::test(flavor = "multi_thread")]
async fn a_closed_project_is_refused_a_second_close() {
    let r = rig();
    let id = project(&r).await;
    rpc(&r.core, "project.close", json!({"project": id}))
        .await
        .unwrap();
    let e = rpc(
        &r.core,
        "project.close",
        json!({"project": id, "how": "dropped"}),
    )
    .await
    .unwrap_err();
    assert!(e.contains("done already"), "{e}");
    assert_eq!(r.task(&id).state, TaskState::Done);
}

/// A reason is one short line.
#[tokio::test(flavor = "multi_thread")]
async fn a_reason_is_one_short_line() {
    let r = rig();
    let id = project(&r).await;
    let e = rpc(
        &r.core,
        "project.close",
        json!({"project": id, "reason": "a\nb"}),
    )
    .await
    .unwrap_err();
    assert!(e.contains("one line"), "{e}");
    assert_eq!(r.task(&id).state, TaskState::Accepted);
}

/// What stands in the way, read as pure rules.
#[test]
fn only_a_live_task_or_a_held_claim_carries() {
    let mk = |id: &str| TaskRecord {
        id: id.into(),
        version: 1,
        title: id.into(),
        owner: "agent".into(),
        ..Default::default()
    };
    let project = TaskRecord {
        kind: TaskKind::Project,
        ..mk("tsk_proj001")
    };
    let item = TaskRecord {
        parent: Some(project.id.clone()),
        ..mk("tsk_item001")
    };
    let all = vec![project.clone(), item.clone()];
    let carried = crate::rpc::project_close::carried_by;
    assert!(carried(&all, &project, &|_| false, &|_| false).is_none());
    assert!(carried(&all, &project, &|t| t.id == "tsk_item001", &|_| false).is_some());
    let holder = TaskRecord {
        session: Some("ses_holder1".into()),
        title: "Sound the channel".into(),
        ..mk("tsk_holder1")
    };
    let claimed = TaskRecord {
        claim: Some(TaskClaim {
            by: "exe_holder1".into(),
            session: "ses_holder1".into(),
            until_ms: 100,
        }),
        ..item
    };
    let all = vec![project.clone(), claimed, holder];
    // A claim names its holder, not the step; one whose holder ended carries nothing.
    let said = carried(&all, &project, &|_| false, &|_| true).unwrap();
    assert!(
        said.contains("older1") && said.contains("Sound the channel"),
        "{said}"
    );
    assert!(!said.contains("em001"), "{said}");
    assert!(carried(&all, &project, &|_| false, &|_| false).is_none());
}
