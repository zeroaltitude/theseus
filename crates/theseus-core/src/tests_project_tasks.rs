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
use crate::tests_task_graph::{results, rig, session, turn, Rig, ASK, QUOTE};
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

/// The record a call wrote, or what its results said instead.
fn found(r: &Rig, chat: &str, title: &str) -> theseus_protocol::tasks::TaskRecord {
    r.tasks()
        .into_iter()
        .find(|t| t.title == title)
        .unwrap_or_else(|| {
            let said: Vec<String> = results(&r.core, chat, "task.create")
                .into_iter()
                .map(|(status, text)| {
                    format!("{status:?}: {}", text.chars().take(150).collect::<String>())
                })
                .collect();
            panic!("no record `{title}`: {said:#?}")
        })
}

/// A model's `task_create` with a brief takes its mark from the conversation that made the call, never from the
/// call: from a conversation attached to project A, with a `parent` naming a loose record and a `project` field
/// that is not the tool's, the task is marked A, listed on A as "started for it" and on no other project; from an
/// unattached conversation it carries no mark.
#[tokio::test(flavor = "multi_thread")]
async fn a_models_task_is_marked_by_its_conversation_never_by_its_call() {
    let r = rig();
    let a = project(&r, json!({"title": "Harbour survey", "review": "none"})).await;
    let b = project(&r, json!({"title": "Pier repairs", "review": "none"})).await;
    // A loose plan item no project holds, for the call's `parent`.
    let chat = session(&r.core);
    r.calls(&[("task_create", json!({"title": "Loose plan item"}))]);
    turn(&r.core, &chat, "note a loose item").await;
    let loose = found(&r, &chat, "Loose plan item").id;
    rpc(
        &r.core,
        "project.attach",
        json!({"session": chat, "project": a}),
    )
    .await
    .unwrap();
    r.model
        .hold_tasks
        .store(true, std::sync::atomic::Ordering::SeqCst);
    let brief = |title: &str, quote: &str| {
        json!({"brief": "Chart every buoy of the outer harbour and report the depths.", "title": title,
               "parent": loose, "arrangement": {"pieces": [{"quote": quote, "role": "objective"}]}})
    };
    // A field that could name a project is not the tool's: the call is refused and writes no task.
    let mut naming = brief("Named by its call", "name a project by hand, please");
    naming["project"] = json!(b);
    r.calls(&[("task_create", naming)]);
    turn(&r.core, &chat, "FIRST: name a project by hand, please").await;
    assert_eq!(
        results(&r.core, &chat, "task.create").pop().unwrap().0,
        crate::node::ResultStatus::Error
    );
    assert!(r.tasks().iter().all(|t| t.title != "Named by its call"));
    r.calls(&[(
        "task_create",
        brief("Started from the attached chat", QUOTE),
    )]);
    turn(&r.core, &chat, ASK).await;
    let t = found(&r, &chat, "Started from the attached chat");
    assert_eq!(
        t.parent.as_deref(),
        Some(loose.as_str()),
        "the call's parent is kept"
    );
    assert_eq!(t.origin.project.as_deref(), Some(a.as_str()), "{t:?}");
    let on_a = hows(&shown(&r, &a).await);
    assert_eq!(
        on_a,
        [(
            "Started from the attached chat".to_string(),
            "started for it".to_string()
        )]
    );
    assert!(
        hows(&shown(&r, &b).await).is_empty(),
        "listed on no other project"
    );
    // An unattached conversation's task carries no mark and is listed nowhere.
    let bare = session(&r.core);
    r.calls(&[("task_create", brief("Started from a bare chat", QUOTE))]);
    turn(&r.core, &bare, ASK).await;
    let u = found(&r, &bare, "Started from a bare chat");
    assert_eq!(u.origin.project, None, "{u:?}");
    assert_eq!(hows(&shown(&r, &a).await).len(), 1);
    assert!(hows(&shown(&r, &b).await).is_empty());
}
