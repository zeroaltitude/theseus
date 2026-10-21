//! A task whose first loop calls the provider's tool search (theseus-s0s4x): the
//! model's answer holds the search call, its result, and a call of ours in one
//! message, and the next request must send all three back in that order. A
//! search call sent without its result is a 400 that ends the task.

use serde_json::{json, Value};
use theseus_kernel::ExecState;
use theseus_protocol::task_start::TaskStartParams;

use crate::approval::{Answerer, Surface};
use crate::provider::{ProviderRequest, Scripted};
use crate::tests_tasks::{answers_a_call, exec, first_user, rig, until};
use crate::tools_defer::SEARCH_TOOL;

/// A task whose first answer searches and calls a tool in the same message,
/// the search's result after the call as the stream can give it, and whose
/// second says it is done.
fn script(req: &ProviderRequest) -> Scripted {
    if !first_user(req).contains("Search first") {
        return Scripted::text("ok");
    }
    if answers_a_call(req) {
        return Scripted::text("Done.");
    }
    Scripted::Blocks {
        blocks: vec![
            json!({"type": "server_tool_use", "id": "srvtoolu_1", "name": SEARCH_TOOL,
                   "input": {"query": "ask the owner"}}),
            json!({"type": "tool_use", "id": "t1", "name": "fs_read",
                   "input": {"path": "missing.md"}}),
            json!({"type": "tool_search_tool_result", "tool_use_id": "srvtoolu_1",
                   "content": {"type": "tool_search_tool_search_result",
                               "tool_references": [{"type": "tool_reference", "tool_name": "owner_ask"}]}}),
        ],
        stop_reason: "tool_use".into(),
    }
}

fn kinds(m: &Value) -> Vec<String> {
    m["content"]
        .as_array()
        .map(|b| {
            b.iter()
                .filter_map(|b| b["type"].as_str().map(str::to_string))
                .collect()
        })
        .unwrap_or_default()
}

#[tokio::test]
async fn a_task_that_searches_in_its_first_loop_sends_the_result_back_with_its_call() {
    let r = rig(script, |_| {});
    let started = r
        .core
        .task_start(
            &TaskStartParams {
                brief: "Search first, then read missing.md".into(),
                ..Default::default()
            },
            Answerer {
                label: "the CLI".into(),
                surface: Surface::Cli,
                discord: None,
                agent: None,
            },
        )
        .unwrap();
    let id = started.session_id.replacen("ses_", "exe_", 1);
    until("the task's turn", || {
        exec(&r.core, &id).state == ExecState::Complete
    })
    .await;
    let req = r
        .model
        .requests
        .lock()
        .unwrap()
        .iter()
        .find(|q| first_user(q).contains("Search first") && answers_a_call(q))
        .cloned()
        .expect("the call's answer went back to the model");
    let sent: Vec<String> = req
        .messages
        .iter()
        .filter(|m| m["role"] == "assistant")
        .flat_map(kinds)
        .collect();
    assert_eq!(
        sent,
        ["server_tool_use", "tool_search_tool_result", "tool_use"],
        "{:?}",
        req.messages
    );
    // Each search call has its result in the message that holds it.
    for m in req.messages.iter().filter(|m| m["role"] == "assistant") {
        for b in m["content"].as_array().unwrap() {
            if b["type"] == "server_tool_use" {
                assert!(
                    m["content"].as_array().unwrap().iter().any(|x| {
                        x["type"] == "tool_search_tool_result" && x["tool_use_id"] == b["id"]
                    }),
                    "{m}"
                );
            }
        }
    }
}
