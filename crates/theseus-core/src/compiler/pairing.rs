//! Which result answers which call (issue 44): by PROVEN identity, never by a guess from the provider's id and
//! position. One association, used by every site that shows a call with its result (the render, the view's
//! receipts, an exchange's spoken form, the bounded window).
//!
//! A call is its response's `tool_use`: the assistant node that makes it, its id, and its place among that
//! response's uses with the same id (so two uses with one id in ONE response are two calls). The gate writes
//! a `ToolCall` record for each, in order, with the kernel action's `correlation_id`; a result carries the
//! same `correlation_id`.
//!
//! 1. A result with a `correlation_id` answers the call whose record has the same one. Nothing else.
//! 2. A result with none (a call refused before any action, a harness's own call) answers the call the
//!    gate's record beside it names: the earliest record with its id and no correlation of its OWN exchange
//!    (its session and turn) standing before it, whose call has no result yet (the gate writes the record and
//!    the result together, and a continuation writes them for a call of the turn it continues). With no such
//!    record, the earliest call with its id of its own exchange standing before it, that carries no
//!    correlation and no result yet.
//! 3. Nothing beyond that: a result whose call is not established answers no call, and that call keeps the
//!    harness's "no result" stub. Inventing an execution is the fault; a truthful "no result" is not.
//! 4. A late result (`late: true`) answers no call: it is told as what came back later.

use std::collections::HashMap;

use crate::node::{Body, Node};

/// A call: its assistant node's id, its provider id, and its place among that response's uses with the id.
pub(crate) type Call<'n> = (&'n str, &'n str, usize);

/// The association of results and calls among a run of nodes ([`pairing`]).
#[derive(Default)]
pub(crate) struct Pairing<'n> {
    by_call: HashMap<Call<'n>, &'n Node>,
    by_result: HashMap<&'n str, Call<'n>>,
    by_record: HashMap<&'n str, Call<'n>>,
}

impl<'n> Pairing<'n> {
    /// The result that answers `call`, if one is established.
    pub(crate) fn result(&self, call: Call<'_>) -> Option<&'n Node> {
        self.by_call.get(&call).copied()
    }

    /// The call a result (by its node id) answers, if one is established.
    pub(crate) fn call_of(&self, result: &str) -> Option<Call<'n>> {
        self.by_result.get(result).copied()
    }

    /// The call a `ToolCall` record (by its node id) is the gate's record of.
    pub(crate) fn record(&self, record: &str) -> Option<Call<'n>> {
        self.by_record.get(record).copied()
    }
}

/// The calls of an assistant message's blocks, in order: each `tool_use` with its place among the uses with
/// its id.
pub(crate) fn uses_of<'b>(
    blocks: impl Iterator<Item = &'b serde_json::Value>,
) -> Vec<(&'b str, usize)> {
    places(
        (blocks.filter(|b| b["type"] == "tool_use")).map(|b| b["id"].as_str().unwrap_or_default()),
    )
}

/// Each of a response's call ids, in order, with its place among the uses with the same id.
pub(crate) fn places<'a>(ids: impl Iterator<Item = &'a str>) -> Vec<(&'a str, usize)> {
    let mut seen: HashMap<&str, usize> = HashMap::new();
    ids.map(|id| {
        let k = seen.entry(id).or_default();
        *k += 1;
        (id, *k - 1)
    })
    .collect()
}

/// The calls among a run of nodes, as their responses and the gate's records hold them.
#[derive(Default)]
struct Calls<'n> {
    /// Where each call stands, and its exchange (session and turn).
    at: HashMap<Call<'n>, (usize, &'n str, Option<&'n str>)>,
    /// Each call's correlation, where its record has one.
    correlated: HashMap<Call<'n>, &'n str>,
    /// Each correlation's call.
    by_corr: HashMap<&'n str, Call<'n>>,
    /// Each record's call, by the record's node id.
    by_record: HashMap<&'n str, Call<'n>>,
    /// The records with no correlation: where each stands, its exchange, and its call.
    plain: Vec<(usize, &'n str, Option<&'n str>, Call<'n>)>,
}

fn calls<'n>(nodes: &[&'n Node]) -> Calls<'n> {
    let mut c = Calls::default();
    let mut records: HashMap<(&'n str, &'n str), usize> = HashMap::new();
    for (i, &n) in nodes.iter().enumerate() {
        let exchange = (i, n.session_id.as_str(), n.turn_id.as_deref());
        match &n.body {
            Body::AssistantMessage { blocks, .. } => {
                for (id, k) in uses_of(blocks.iter()) {
                    c.at.entry((n.id.as_str(), id, k)).or_insert(exchange);
                }
            }
            Body::ToolCall {
                tool_use_id,
                assistant_node,
                correlation_id,
                ..
            } => {
                let k =
                    (records.entry((assistant_node.as_str(), tool_use_id.as_str()))).or_default();
                let call = (assistant_node.as_str(), tool_use_id.as_str(), *k);
                *k += 1;
                c.by_record.insert(n.id.as_str(), call);
                c.at.entry(call).or_insert(exchange);
                match correlation_id.as_deref() {
                    Some(corr) => {
                        c.correlated.insert(call, corr);
                        c.by_corr.insert(corr, call);
                    }
                    None => c.plain.push((exchange.0, exchange.1, exchange.2, call)),
                }
            }
            _ => {}
        }
    }
    c
}

impl<'n> Calls<'n> {
    /// Rule 2: for a result with no correlation, standing at `i` in exchange `own`, the call of the earliest
    /// record with its id and no correlation of the same exchange standing before it; else the earliest call
    /// with its id of the same exchange standing before it; either with no correlation and no result in
    /// `answered`.
    fn own(
        &self,
        id: &str,
        i: usize,
        own: (&str, Option<&str>),
        answered: &HashMap<Call<'n>, &'n Node>,
    ) -> Option<Call<'n>> {
        let recorded = (self.plain.iter())
            .filter(|(j, session, turn, call)| {
                call.1 == id && *j < i && (*session, *turn) == own && !answered.contains_key(call)
            })
            .map(|(j, _, _, call)| (*j, *call))
            .min();
        if let Some((_, call)) = recorded {
            return Some(call);
        }
        (self.at.iter())
            .filter(|(call, (j, session, turn))| call.1 == id && *j < i && (*session, *turn) == own)
            .filter(|(call, _)| {
                !self.correlated.contains_key(*call) && !answered.contains_key(*call)
            })
            .map(|(call, (j, _, _))| (*j, *call))
            .min()
            .map(|(_, call)| call)
    }
}

/// The association among `nodes`, in their order.
pub(crate) fn pairing<'n>(nodes: impl IntoIterator<Item = &'n Node>) -> Pairing<'n> {
    let nodes: Vec<&'n Node> = nodes.into_iter().collect();
    let c = calls(&nodes);
    let mut p = Pairing::default();
    for (i, &n) in nodes.iter().enumerate() {
        let Body::ToolResult {
            tool_use_id,
            correlation_id,
            late: false,
            ..
        } = &n.body
        else {
            continue;
        };
        let call = match correlation_id.as_deref() {
            Some(corr) => c.by_corr.get(corr).copied(),
            None => c.own(
                tool_use_id,
                i,
                (n.session_id.as_str(), n.turn_id.as_deref()),
                &p.by_call,
            ),
        };
        let Some(call) = call else { continue };
        // The same call answered again (its own correlation twice): its last result, as before.
        if let Some(before) = p.by_call.insert(call, n) {
            p.by_result.remove(before.id.as_str());
        }
        p.by_result.insert(n.id.as_str(), call);
    }
    p.by_record = c.by_record;
    p
}
