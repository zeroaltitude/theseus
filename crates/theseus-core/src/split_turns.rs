//! Whole turns for a split (theseus-q9b85): a part of a session that moves to a thread holds each
//! turn it touches whole, so neither side's compile ever holds a tool call without its result, or
//! half of what a turn said.
//!
//! A turn is the nodes that share a `turn_id` (a message through its last reply, with its calls and
//! results); a call, its answer's message and its result are one unit as well, a late background
//! result included. A pick is refused when it holds some of a unit's nodes and not the rest, with
//! the positions to type instead. A turn's recall nodes are not history: they move with their turn
//! whether the pick held them or not, and stay when it does not touch the turn.

use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};

use crate::node::Body;
use crate::store::Transcript;

/// What a pick comes to once its turns are whole.
#[derive(Debug, Clone, Default)]
pub struct Closed {
    /// Every node that leaves the source's compile, in position order: the pick, and the recall
    /// notes of the turns it touches.
    pub moved: Vec<String>,
    /// The position of the first and the last of them.
    pub first: u64,
    pub last: u64,
    /// The turns touched.
    pub turns: u32,
    /// The stored size of the moved nodes.
    pub bytes: u64,
}

fn at(nodes: &Transcript, i: usize) -> String {
    let (p, n) = &nodes[i];
    let what = match &n.body {
        Body::UserMessage { .. } => "message",
        Body::AssistantMessage { .. } => "reply",
        Body::ToolCall { .. } => "call",
        Body::ToolResult { .. } => "result",
        _ => "node",
    };
    format!("@{p} ({what})")
}

/// The units of nodes that must move together besides a turn: each tool use id's reply, call and
/// results.
fn tool_units(nodes: &Transcript) -> Vec<Vec<usize>> {
    let mut by_id: BTreeMap<String, Vec<usize>> = BTreeMap::new();
    for (i, (_, n)) in nodes.iter().enumerate() {
        match &n.body {
            Body::AssistantMessage { blocks, .. } => {
                for u in crate::provider::tool_uses_in(blocks) {
                    by_id.entry(u.id).or_default().push(i);
                }
            }
            Body::ToolCall { tool_use_id, .. } | Body::ToolResult { tool_use_id, .. } => {
                by_id.entry(tool_use_id.clone()).or_default().push(i);
            }
            _ => {}
        }
    }
    by_id.into_values().filter(|u| u.len() > 1).collect()
}

/// `idx` made whole, or why not. `idx` is sorted and holds no node twice.
pub fn close(nodes: &Transcript, idx: &[usize]) -> Result<Closed, String> {
    let picked: HashSet<usize> = idx.iter().copied().collect();
    let recall = |i: usize| matches!(nodes[i].1.body, Body::Recall { .. });
    // The turns the pick touches, by the nodes that are not recall notes.
    let touched: BTreeSet<&str> = idx
        .iter()
        .filter(|i| !recall(**i))
        .filter_map(|i| nodes[*i].1.turn_id.as_deref())
        .collect();
    for (j, (_, n)) in nodes.iter().enumerate() {
        let Some(t) = n.turn_id.as_deref() else {
            continue;
        };
        if recall(j) || picked.contains(&j) || !touched.contains(t) {
            continue;
        }
        let of: Vec<u64> = nodes
            .iter()
            .filter(|(_, m)| {
                m.turn_id.as_deref() == Some(t) && !matches!(m.body, Body::Recall { .. })
            })
            .map(|(p, _)| *p)
            .collect();
        return Err(format!(
            "the part chosen cuts a turn in half: {} is a node of the turn that runs from @{} to \
             @{}, and it is outside the part. A turn moves whole, from its message through its \
             last reply with its calls and results: choose @{} to @{}",
            at(nodes, j),
            of.first().copied().unwrap_or_default(),
            of.last().copied().unwrap_or_default(),
            of.first().copied().unwrap_or_default(),
            of.last().copied().unwrap_or_default(),
        ));
    }
    for unit in tool_units(nodes) {
        let inside = unit.iter().filter(|i| picked.contains(i)).count();
        if inside == 0 || inside == unit.len() {
            continue;
        }
        let out = unit
            .iter()
            .find(|i| !picked.contains(i))
            .copied()
            .unwrap_or(0);
        return Err(format!(
            "the part chosen holds a tool call without its result, or the reverse: {} is part of \
             the same call and is outside it; choose the whole turn",
            at(nodes, out)
        ));
    }
    let moved: Vec<usize> = (0..nodes.len())
        .filter(|i| {
            picked.contains(i)
                || (recall(*i)
                    && nodes[*i]
                        .1
                        .turn_id
                        .as_deref()
                        .is_some_and(|t| touched.contains(t)))
        })
        .collect();
    let positions = |i: &usize| nodes[*i].0;
    let bytes = moved
        .iter()
        .map(|i| serde_json::to_vec(&nodes[*i].1.body).map_or(0, |b| b.len() as u64))
        .sum();
    Ok(Closed {
        first: moved.first().map(positions).unwrap_or_default(),
        last: moved.last().map(positions).unwrap_or_default(),
        turns: touched.len() as u32,
        bytes,
        moved: moved.iter().map(|i| nodes[*i].1.id.clone()).collect(),
    })
}

/// A gist as a line: the whitespace of `s` as single spaces, cut to `max` characters.
pub fn one_line(s: &str, max: usize) -> String {
    let line = crate::arrangement::normalize(s);
    if line.chars().count() > max {
        format!("{}…", line.chars().take(max).collect::<String>())
    } else {
        line
    }
}

/// What a range is about when its owner did not say: its first human message, else its first
/// message with text, cut to 80 characters.
pub fn default_gist(nodes: &Transcript, idx: &[usize]) -> String {
    let human = idx.iter().find_map(|i| match &nodes[*i].1.body {
        Body::UserMessage { text, .. } if nodes[*i].1.origin == crate::node::Origin::Operator => {
            Some(text.as_str())
        }
        _ => None,
    });
    let text = human.map(str::to_string).or_else(|| {
        idx.iter()
            .find_map(|i| crate::arrangement::text_of(&nodes[*i].1))
    });
    one_line(&text.unwrap_or_default(), 80)
}

/// The nodes of `nodes` by id, to position: for a caller that has the ids of a record.
pub fn positions_of(nodes: &Transcript, ids: &[String]) -> HashMap<String, u64> {
    let want: HashSet<&str> = ids.iter().map(String::as_str).collect();
    nodes
        .iter()
        .filter(|(_, n)| want.contains(n.id.as_str()))
        .map(|(p, n)| (n.id.clone(), *p))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::node::{Node, ResultStatus};
    use crate::stub::Stub;
    use serde_json::{json, Value};

    fn put(nodes: &mut Transcript, n: Node) {
        let p = nodes.len() as u64 + 1;
        nodes.push((p, Stub::hydrated(p, std::sync::Arc::new(n))));
    }

    /// Two turns: the second calls a tool; and a recall note in the second.
    fn transcript() -> Transcript {
        let sid = "ses_h";
        let mut t = Transcript::new();
        put(&mut t, Node::user(sid, Some("t1"), "owner", "one"));
        let reply = |turn: &str, blocks: Vec<Value>| {
            Node::assistant(
                sid,
                turn,
                0,
                Body::AssistantMessage {
                    blocks,
                    model: "m".into(),
                    provider: "p".into(),
                    stop_reason: None,
                    usage: Default::default(),
                    cost_usd: None,
                    catalog_version: None,
                    request_id: None,
                    correlation_id: None,
                    compilation_id: None,
                    request_digest: None,
                    effort: None,
                },
            )
        };
        put(
            &mut t,
            reply("t1", vec![json!({"type": "text", "text": "ok"})]),
        );
        put(&mut t, Node::user(sid, Some("t2"), "owner", "two"));
        put(&mut t, Node::recall(sid, "t2", "rcl", "baseline", vec![]));
        let a = reply(
            "t2",
            vec![json!({"type": "tool_use", "id": "tu1", "name": "fs_list", "input": {}})],
        );
        let call = Node::tool_call(
            sid,
            Some("t2"),
            Some(0),
            Body::ToolCall {
                tool_use_id: "tu1".into(),
                tool: "fs.list".into(),
                wire_name: "fs_list".into(),
                input: json!({}),
                assistant_node: a.id.clone(),
                correlation_id: None,
                gate: None,
            },
        );
        put(&mut t, a);
        put(&mut t, call);
        put(
            &mut t,
            Node::tool_result(
                sid,
                Some("t2"),
                Some(0),
                Body::ToolResult {
                    tool_use_id: "tu1".into(),
                    tool: "fs.list".into(),
                    status: ResultStatus::Ok,
                    is_error: false,
                    content: "x".into(),
                    correlation_id: None,
                    bytes_total: 0,
                    truncated: false,
                    full_ref: None,
                    duration_ms: None,
                    late: false,
                    meta: Value::Null,
                    image: None,
                    external: None,
                },
            ),
        );
        put(
            &mut t,
            reply("t2", vec![json!({"type": "text", "text": "done"})]),
        );
        t
    }

    #[test]
    fn a_whole_turn_moves_with_its_recall_note_and_a_cut_one_is_refused() {
        let t = transcript();
        // Turn 2 whole: nodes 3 to 8 (the recall note is among them).
        let c = close(&t, &[2, 4, 5, 6, 7]).unwrap();
        assert_eq!((c.turns, c.moved.len(), c.first, c.last), (1, 6, 3, 8));
        assert!(c.bytes > 0);
        // Turn 1 whole leaves turn 2's recall note, which is not its.
        let c = close(&t, &[0, 1]).unwrap();
        assert_eq!(c.moved.len(), 2);
        // A turn without its last reply, and a call without its result.
        let e = close(&t, &[2, 4, 5, 6]).unwrap_err();
        assert!(
            e.contains("cuts a turn in half") && e.contains("choose @3 to @8"),
            "{e}"
        );
        let e = close(&t, &[5]).unwrap_err();
        assert!(e.contains("cuts a turn in half"), "{e}");
    }

    #[test]
    fn a_gist_is_the_first_human_message_cut_to_a_line() {
        let t = transcript();
        assert_eq!(default_gist(&t, &[0, 1]), "one");
        assert_eq!(
            default_gist(&t, &[1]),
            "ok",
            "no human message: the first with text"
        );
        let long = "word ".repeat(40);
        let g = one_line(&format!("{long}\nmore"), 80);
        assert_eq!(g.chars().count(), 81);
        assert!(!g.contains('\n') && g.ends_with('…'));
    }
}
