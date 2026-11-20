//! The tasks involved in a project (issue 37; `project/involved.rs`): listed by `project.show` and counted in
//! its info without anyone adding them by hand: a task started from a conversation attached to the project
//! (the mark `task.start` writes on its record), a task whose record hangs under the project's, a task that
//! holds a claim on one of its records, a run of its weekly review. A task from an unattached conversation is
//! not listed. A closed project keeps its list.

use std::sync::Arc;

use serde_json::{json, Value};
use theseus_protocol::tasks::{TaskClaim, TaskKind};

use crate::approval::Surface;
use crate::tests_mcp_memory::rpc as rpc_on;
use crate::tests_task_graph::{rig, Rig};
use crate::{task_graph as graph, Core};

async fn rpc(core: &Arc<Core>, method: &str, params: Value) -> Result<Value, String> {
    rpc_on(core, Surface::Cli, method, params)
        .await
        .map_err(|e| e.message)
}

async fn project(r: &Rig, extra: Value) -> String {
    let mut p = json!({"title": "Harbour survey"});
    for (k, v) in extra.as_object().unwrap() {
        p[k] = v.clone();
    }
    let made = rpc(&r.core, "project.create", p).await.unwrap();
    made["project"]["id"].as_str().unwrap().to_string()
}

/// The owner's Home, the conversation a task started with no `home` goes under.
fn home(r: &Rig) -> String {
    r.core.home().unwrap().session_id
}

async fn attach(r: &Rig, project: Option<&str>) {
    let mut p = json!({"session": home(r)});
    if let Some(id) = project {
        p["project"] = json!(id);
    }
    rpc(&r.core, "project.attach", p).await.unwrap();
}

/// Start a task that stays running; its session id.
async fn start(r: &Rig, brief: &str) -> String {
    r.model
        .hold_tasks
        .store(true, std::sync::atomic::Ordering::SeqCst);
    let said = rpc(&r.core, "task.start", json!({"brief": brief}))
        .await
        .unwrap();
    said["session_id"].as_str().unwrap().to_string()
}

async fn shown(r: &Rig, id: &str) -> Value {
    rpc(&r.core, "project.show", json!({"id": id}))
        .await
        .unwrap()
}

fn hows(shown: &Value) -> Vec<(String, String)> {
    shown["tasks"]
        .as_array()
        .map(|a| {
            a.iter()
                .map(|t| {
                    (
                        t["title"].as_str().unwrap_or("").to_string(),
                        t["how"].as_str().unwrap().to_string(),
                    )
                })
                .collect()
        })
        .unwrap_or_default()
}

/// A task started from a conversation attached to the project is listed for it, running, and counted; one
/// started from the conversation after it was detached is not; the mark is on the task's record.
#[tokio::test(flavor = "multi_thread")]
async fn a_task_started_from_an_attached_conversation_is_listed_for_the_project() {
    let r = rig();
    let id = project(&r, json!({"review": "none"})).await;
    attach(&r, Some(&id)).await;
    let session = start(&r, "Sound the channel").await;
    attach(&r, None).await;
    let other = start(&r, "Count the boats").await;
    let mark = |s: &str| {
        r.tasks()
            .into_iter()
            .find(|t| t.session.as_deref() == Some(s))
            .unwrap()
            .origin
            .project
    };
    assert_eq!(mark(&session).as_deref(), Some(id.as_str()));
    assert_eq!(
        mark(&other),
        None,
        "an unattached conversation's task carries no mark"
    );
    let s = shown(&r, &id).await;
    let listed: Vec<&str> = s["tasks"]
        .as_array()
        .unwrap()
        .iter()
        .map(|t| t["task_id"].as_str().unwrap())
        .collect();
    assert_eq!(listed, [session.as_str()], "{s}");
    let t = &s["tasks"][0];
    assert_eq!(t["how"], "started for it");
    assert_eq!(t["ended"], false);
    assert_eq!(t["short"], session[session.len() - 6..]);
    assert_eq!(s["project"]["tasks"], 1);
    assert_eq!(s["project"]["tasks_running"], 1);
    assert!(s.get("tasks_more").is_none());
    // The same count is in the list the look reads.
    let list = rpc(&r.core, "project.list", json!({})).await.unwrap();
    assert_eq!(list["projects"][0]["tasks"], 1);
    assert_eq!(list["projects"][0]["tasks_running"], 1);
}

/// A task whose own record hangs under the project, and one that holds a claim on one of its steps, are listed
/// with how; neither is listed twice.
#[tokio::test(flavor = "multi_thread")]
async fn a_task_under_the_project_and_a_task_holding_a_step_are_listed() {
    let r = rig();
    let id = project(&r, json!({"review": "none"})).await;
    rpc(
        &r.core,
        "project.item.add",
        json!({"project": id, "title": "Order the sounding line"}),
    )
    .await
    .unwrap();
    let under = start(&r, "Sound the channel").await;
    let holder = start(&r, "Book the boat").await;
    let mut rec = r
        .tasks()
        .into_iter()
        .find(|t| t.session.as_deref() == Some(under.as_str()))
        .unwrap();
    rec.parent = Some(id.clone());
    rec.version += 1;
    r.core
        .store
        .append(&[graph::record(&rec).unwrap()])
        .unwrap();
    let mut step = r
        .tasks()
        .into_iter()
        .find(|t| {
            t.parent.as_deref() == Some(id.as_str())
                && t.kind == TaskKind::Item
                && t.session.is_none()
        })
        .unwrap();
    step.claim = Some(TaskClaim {
        by: holder.replacen("ses_", "exe_", 1),
        session: holder.clone(),
        until_ms: r.core.kernel.now_ms() + 1_800_000,
    });
    step.version += 1;
    r.core
        .store
        .append(&[graph::record(&step).unwrap()])
        .unwrap();
    let s = shown(&r, &id).await;
    let mut got = hows(&s);
    got.sort();
    assert_eq!(
        got,
        [
            ("Book the boat".to_string(), "holds a step".to_string()),
            ("Sound the channel".to_string(), "under it".to_string()),
        ],
        "{s}"
    );
    assert_eq!(s["project"]["tasks"], 2);
}

/// A run of the project's weekly review is listed, and still is once the project is closed and its routine
/// is gone.
#[tokio::test(flavor = "multi_thread")]
async fn a_review_run_is_listed_and_a_closed_project_keeps_its_list() {
    let r = rig();
    let id = project(&r, json!({})).await;
    let review = r.task(&id).review.unwrap();
    rpc(&r.core, "routine.run_now", json!({"id": review}))
        .await
        .unwrap();
    let s = shown(&r, &id).await;
    let got = hows(&s);
    assert_eq!(got.len(), 1, "{s}");
    assert_eq!(got[0].1, "weekly review");
    rpc(&r.core, "project.close", json!({"project": id}))
        .await
        .unwrap();
    assert_eq!(routines(&r).await, 0, "its review is gone");
    let again = shown(&r, &id).await;
    assert_eq!(hows(&again), got, "a closed project keeps its list");
    assert_eq!(again["project"]["state"], "done");
}

async fn routines(r: &Rig) -> usize {
    rpc(&r.core, "routine.list", json!({})).await.unwrap()["routines"]
        .as_array()
        .unwrap()
        .len()
}

/// Nothing is listed for a project nobody worked on.
#[tokio::test(flavor = "multi_thread")]
async fn a_project_nobody_worked_on_lists_no_tasks() {
    let r = rig();
    let id = project(&r, json!({"review": "none"})).await;
    start(&r, "Count the boats").await;
    let s = shown(&r, &id).await;
    assert!(s.get("tasks").is_none(), "{s}");
    assert!(s["project"].get("tasks").is_none(), "{s}");
}
