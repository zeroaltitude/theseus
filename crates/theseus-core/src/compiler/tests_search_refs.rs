//! A replayed tool search must match the request that carries it
//! (theseus-it6th). The API checks every `tool_reference` of a replayed
//! `tool_search_tool_result` against the request's tools, and answers a
//! reference to a tool it does not offer with a 400 ("Tool reference
//! 'gmail_read' not found in available tools") that fails the turn and the
//! next message after it. A detour's request has no tool search tool and a
//! deferred set can leave a family out, so the render answers for the pair:
//! with no search tool in the request the pair is left out; with one, the
//! pair stays only when every tool it names is offered.

use serde_json::{json, Value};

use super::tests_ring::{answer, spec};
use super::*;
use crate::tools_defer::{SEARCH_TOOL, SEARCH_TYPE};

fn search(id: &str) -> Value {
    json!({"type": "server_tool_use", "id": id, "name": SEARCH_TOOL,
           "input": {"query": "read my mail"}})
}

fn found(id: &str, tools: &[&str]) -> Value {
    let refs: Vec<Value> = tools
        .iter()
        .map(|t| json!({"type": "tool_reference", "tool_name": t}))
        .collect();
    json!({"type": "tool_search_tool_result", "tool_use_id": id,
           "content": {"type": "tool_search_tool_search_result", "tool_references": refs}})
}

fn text(t: &str) -> Value {
    json!({"type": "text", "text": t})
}

fn def(name: &str, deferred: bool) -> Value {
    let mut d = json!({"name": name, "description": "d", "input_schema": {"type": "object"}});
    if deferred {
        d["defer_loading"] = json!(true);
    }
    d
}

/// The request's tools: a search tool in front when `searching`, then `named`
/// (each deferred when the search tool is there to find it).
fn offered(searching: bool, named: &[&str]) -> Vec<Value> {
    let mut tools = vec![];
    if searching {
        tools.push(json!({"type": SEARCH_TYPE, "name": SEARCH_TOOL}));
    }
    tools.push(def("fs_read", false));
    tools.extend(named.iter().map(|n| def(n, searching)));
    tools
}

/// Compile a conversation whose first answer searched (the pair `blocks`,
/// then `says`) and whose last message is a new one, onto `tools`.
fn run(blocks: Vec<Value>, tools: Vec<Value>) -> Compiled {
    let nodes: Vec<(u64, crate::stub::Stub)> = vec![
        (1, Node::user("s", None, "cli", "read my mail").into()),
        (2, answer(blocks).into()),
        (3, Node::user("s", None, "cli", "what is the load?").into()),
    ];
    let mut sp = spec();
    sp.tools = tools.into();
    compile(CompileInput {
        session_id: "s",
        current: None,
        nodes: &nodes,
        last_position: 3,
        spec: &sp,
        catalog: &Catalog::builtin(),
        force: None,
        window_override: None,
        blobs: None,
        hidden: &[],
        strip: None,
        overflowed: None,
        sources: &Default::default(),
        signals: None,
        assembled: None,
        situation: &situation::Situation::Continuation,
        kept: None,
    })
}

fn kinds(c: &Compiled) -> Vec<String> {
    c.request
        .messages
        .iter()
        .flat_map(|m| m["content"].as_array().cloned().unwrap_or_default())
        .filter_map(|b| b["type"].as_str().map(str::to_string))
        .collect()
}

fn replays_a_search(c: &Compiled) -> bool {
    let k = kinds(c);
    k.iter().any(|t| t == "server_tool_use" || t == "tool_search_tool_result")
}

#[test]
fn a_request_with_no_search_tool_leaves_out_the_replayed_pair() {
    let c = run(
        vec![search("s1"), found("s1", &["gmail_read"]), text("found it")],
        offered(false, &[]),
    );
    assert!(!replays_a_search(&c), "{:?}", c.request.messages);
    // The answer's own words stay.
    assert_eq!(kinds(&c), ["text", "text", "text"]);
}

#[test]
fn a_search_request_that_does_not_offer_the_referenced_tool_leaves_out_the_pair() {
    let c = run(
        vec![search("s1"), found("s1", &["gmail_read"]), text("found it")],
        offered(true, &["aws_call"]),
    );
    assert!(!replays_a_search(&c), "{:?}", c.request.messages);
}

#[test]
fn one_missing_tool_of_several_leaves_out_the_whole_pair() {
    let c = run(
        vec![
            search("s1"),
            found("s1", &["aws_call", "gmail_read"]),
            text("found it"),
        ],
        offered(true, &["aws_call"]),
    );
    assert!(!replays_a_search(&c), "{:?}", c.request.messages);
}

#[test]
fn a_search_request_that_offers_every_referenced_tool_replays_the_pair_whole() {
    let blocks = vec![
        search("s1"),
        found("s1", &["gmail_read", "aws_call"]),
        text("found it"),
    ];
    let c = run(blocks.clone(), offered(true, &["aws_call", "gmail_read"]));
    assert_eq!(c.request.messages[1]["content"], json!(blocks));
}

#[test]
fn a_pair_is_judged_alone_a_later_search_the_request_serves_stays() {
    let c = run(
        vec![
            search("old"),
            found("old", &["gmail_read"]),
            search("new"),
            found("new", &["aws_call"]),
            text("found it"),
        ],
        offered(true, &["aws_call"]),
    );
    let ids: Vec<&str> = c.request.messages[1]["content"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|b| b["id"].as_str().or(b["tool_use_id"].as_str()))
        .collect();
    assert_eq!(ids, ["new", "new"]);
}

#[test]
fn an_answer_that_was_only_the_pair_leaves_no_empty_message() {
    let c = run(
        vec![search("s1"), found("s1", &["gmail_read"])],
        offered(false, &[]),
    );
    assert!(!replays_a_search(&c));
    for m in &c.request.messages {
        assert!(
            m["content"].as_array().is_some_and(|b| !b.is_empty()),
            "an empty message: {m}"
        );
    }
    assert_eq!(c.request.messages.len(), 1, "{:?}", c.request.messages);
}
