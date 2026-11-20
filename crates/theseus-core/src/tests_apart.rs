//! The self loop's container is kept out of the lists of what is planned (issue 37): `task.list` names it in
//! `apart`, its items stay in `records`, and the owner's close of a project says where it is managed.

use serde_json::json;
use theseus_protocol::tasks::{TaskKind, TaskRecord, TaskState};
use theseus_store::{kinds, NewRecord, Store as _};

use crate::approval::Surface;
use crate::task_graph as graph;
use crate::tests_mcp_memory::rpc as rpc_on;
use crate::tests_task_graph::rig;

async fn rpc(
    core: &std::sync::Arc<crate::Core>,
    method: &str,
    params: serde_json::Value,
) -> Result<serde_json::Value, String> {
    rpc_on(core, Surface::Cli, method, params)
        .await
        .map_err(|e| e.message)
}

/// The record the self loop files its items under is no project: the close says where it is managed, and
/// the plans' list leaves it out while its items stay in `records`.
#[tokio::test(flavor = "multi_thread")]
async fn the_loops_container_is_no_project_and_no_plan() {
    let r = rig();
    let container = TaskRecord {
        id: "tsk_loopbox1".into(),
        version: 1,
        title: "Self-improvement backlog".into(),
        state: TaskState::Accepted,
        owner: "agent".into(),
        created_at_ms: 1,
        updated_at_ms: 1,
        ..Default::default()
    };
    let item = TaskRecord {
        id: "tsk_loopitem1".into(),
        title: "Retry the flaky call".into(),
        parent: Some(container.id.clone()),
        state: TaskState::Proposed,
        ..container.clone()
    };
    r.core
        .store
        .append(&[
            graph::record(&container).unwrap(),
            graph::record(&item).unwrap(),
            NewRecord::json(
                kinds::META,
                Some("self.backlog"),
                &json!({"parent": container.id}),
            )
            .unwrap(),
        ])
        .unwrap();
    let listed = rpc(&r.core, "task.list", json!({})).await.unwrap();
    assert_eq!(listed["apart"], json!(["tsk_loopbox1"]), "{listed}");
    let kept: Vec<&str> = listed["records"]
        .as_array()
        .unwrap()
        .iter()
        .map(|t| t["id"].as_str().unwrap())
        .collect();
    assert!(
        kept.contains(&"tsk_loopbox1") && kept.contains(&"tsk_loopitem1"),
        "{kept:?}"
    );
    let e = rpc(&r.core, "project.close", json!({"project": "tsk_loopbox1"}))
        .await
        .unwrap_err();
    assert!(e.contains("theseus self"), "{e}");
    assert_eq!(r.task("tsk_loopbox1").state, TaskState::Accepted);
    // An item that is not the container is a plain record: not a project either.
    let e = rpc(
        &r.core,
        "project.close",
        json!({"project": "tsk_loopitem1"}),
    )
    .await
    .unwrap_err();
    assert!(e.contains("not a project"), "{e}");
    assert_eq!(r.task("tsk_loopitem1").kind, TaskKind::Item);
}

/// With no loop, nothing is kept apart.
#[tokio::test(flavor = "multi_thread")]
async fn nothing_is_kept_apart_without_the_loop() {
    let r = rig();
    rpc(
        &r.core,
        "project.create",
        json!({"title": "Harbour survey"}),
    )
    .await
    .unwrap();
    let listed = rpc(&r.core, "task.list", json!({})).await.unwrap();
    assert!(listed.get("apart").is_none(), "{listed}");
}
