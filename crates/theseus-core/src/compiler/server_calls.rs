//! The provider's own tool calls in a stored answer (theseus-s0s4x): a
//! `server_tool_use` block (the tool search's call) and the result block the
//! provider ran it to. The API takes the pair only whole and in order: the
//! result right after its call. A request that sends a call without its
//! result, or a result away from its call, is refused with a 400 that ends
//! the task whose first loop searched. The model's answer is stored as the
//! stream gave it, so the render closes each pair on the way out: a result
//! moves to just after its call, and a call or result with no partner is left
//! out.
//!
//! A pair also has to stand on the request that carries it (theseus-it6th):
//! the API checks each `tool_reference` of a replayed search result against
//! the request's tools, and a reference to a tool it does not offer is a 400
//! that fails the turn and every one after it. So a request with no tool
//! search tool (a detour's, a model's without one) leaves every replayed
//! search out, and one with the tool leaves out a search that names a tool it
//! does not offer.

use std::collections::{BTreeSet, HashMap};

use serde_json::Value;

use crate::tools_defer::{scan, SEARCH_TOOL};

/// What a request's tools let a replayed tool search stand on.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Offers {
    /// No tool search tool: no search is replayed.
    Without,
    /// The tool search tool, and the wire names of the tools it sits beside.
    Search(BTreeSet<String>),
}

impl Offers {
    pub(crate) fn of(tools: &[Value]) -> Offers {
        let name = |t: &Value| t.get("name").and_then(Value::as_str).map(str::to_string);
        match tools.iter().any(|t| name(t).as_deref() == Some(SEARCH_TOOL)) {
            true => Offers::Search(tools.iter().filter_map(name).collect()),
            false => Offers::Without,
        }
    }

    /// Whether a search's result can be replayed to this request.
    fn stands(&self, result: &Value) -> bool {
        match self {
            Offers::Without => false,
            Offers::Search(names) => scan::referenced(result).iter().all(|r| names.contains(r)),
        }
    }
}

fn kind(b: &Value) -> Option<&str> {
    b.get("type").and_then(Value::as_str)
}

/// A provider-run tool's result: any block but our own `tool_result` that
/// names the call it answers.
fn result_of(b: &Value) -> Option<&str> {
    match kind(b) {
        Some("tool_result") | None => None,
        Some(_) => b.get("tool_use_id").and_then(Value::as_str),
    }
}

/// `blocks`, each server call followed at once by its result, and a tool
/// search left out unless `offers` lets it stand. Untouched (and not copied)
/// when no block is a server call or result.
pub(super) fn closed(blocks: Vec<Value>, offers: &Offers) -> Vec<Value> {
    let calls = |b: &Value| kind(b) == Some("server_tool_use");
    if !blocks.iter().any(|b| calls(b) || result_of(b).is_some()) {
        return blocks;
    }
    let mut results: HashMap<String, Value> = HashMap::new();
    let mut rest = Vec::with_capacity(blocks.len());
    for b in blocks {
        match result_of(&b) {
            Some(id) => {
                results.entry(id.to_string()).or_insert(b);
            }
            None => rest.push(b),
        }
    }
    let mut out = Vec::with_capacity(rest.len() + results.len());
    for b in rest {
        if calls(&b) {
            let id = b.get("id").and_then(Value::as_str).unwrap_or_default();
            // A call the provider never answered is left out: sent alone it is a 400.
            if let Some(r) = results.remove(id) {
                let search = kind(&r) == Some("tool_search_tool_result");
                if !search || offers.stands(&r) {
                    out.push(b);
                    out.push(r);
                }
            }
        } else {
            out.push(b);
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    /// A request with the tool search tool and the tools these blocks name.
    fn all() -> Offers {
        Offers::Search(BTreeSet::new())
    }
    fn call(id: &str) -> Value {
        json!({"type": "server_tool_use", "id": id, "name": "tool_search_tool_bm25", "input": {}})
    }
    fn found(id: &str) -> Value {
        json!({"type": "tool_search_tool_result", "tool_use_id": id, "content": {}})
    }
    fn ours(id: &str) -> Value {
        json!({"type": "tool_use", "id": id, "name": "fs_read", "input": {}})
    }
    fn ids(v: &[Value]) -> Vec<String> {
        v.iter()
            .map(|b| {
                format!(
                    "{}:{}",
                    b["type"],
                    b["id"].as_str().or(b["tool_use_id"].as_str()).unwrap_or("")
                )
            })
            .collect()
    }

    #[test]
    fn a_result_that_came_after_a_call_of_ours_moves_to_its_call() {
        let out = closed(vec![call("s1"), ours("t1"), found("s1")], &all());
        assert_eq!(
            ids(&out),
            [
                "\"server_tool_use\":s1",
                "\"tool_search_tool_result\":s1",
                "\"tool_use\":t1"
            ]
        );
    }

    #[test]
    fn a_call_with_no_result_and_a_result_with_no_call_are_left_out() {
        let out = closed(vec![call("s1"), ours("t1"), found("s9")], &all());
        assert_eq!(ids(&out), ["\"tool_use\":t1"]);
    }

    #[test]
    fn a_whole_pair_in_order_is_unchanged() {
        let blocks = vec![
            json!({"type": "text", "text": "x"}),
            call("s1"),
            found("s1"),
            ours("t1"),
        ];
        assert_eq!(closed(blocks.clone(), &all()), blocks);
    }

    #[test]
    fn two_searches_each_keep_their_own_result() {
        let out = closed(vec![call("a"), call("b"), found("b"), found("a")], &all());
        assert_eq!(
            ids(&out),
            [
                "\"server_tool_use\":a",
                "\"tool_search_tool_result\":a",
                "\"server_tool_use\":b",
                "\"tool_search_tool_result\":b"
            ]
        );
    }
}
