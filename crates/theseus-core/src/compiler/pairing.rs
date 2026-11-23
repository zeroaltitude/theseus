//! Which result answers which call (issue 44): by PROVEN identity, never by a guess from the provider's id and
//! position. One association, used by every site that shows a call with its result (the render, the view's
//! receipts, an exchange's spoken form, the bounded window).
//!
//! A call is its response's `tool_use`: the assistant node that makes it and its id. A gate record (`ToolCall`)
//! names a call the same way, by its response (`assistant_node`) and id, with the kernel action's
//! `correlation_id` when the call reached one; a result carries the same `correlation_id`. A record does not
//! name WHICH use when a response repeats an id, so:
//!
//! 0. A response that gives one id to more than one use pairs NONE of them: each keeps the harness's "no result"
//!    stub, whatever records or results exist. Such a response is never run (`toolrun::calls`): each of those
//!    uses is answered "Not run", by a result that names its response (`meta.repeated_in`), and that refusal is
//!    what such a use reads ([`Pairing::refusal`]); a stored response of the shape from before reads the stub.
//! 1. A result with a `correlation_id` answers the call whose record has the same one. Nothing else.
//! 2. A result with none (a call refused before any action, a harness's own call) answers the one call the
//!    gate's records of its OWN exchange (its session and turn) name with its id and no correlation, written
//!    no later than it (a continuation writes a record and its result together for a call of the turn it
//!    continues); with no such record, the one call with its id of its own exchange standing before it, with no
//!    correlation. Either way the call has no result yet, and when more than one candidate matches, the result
//!    answers none.
//! 3. Nothing beyond that: a result whose call is not established answers no call, and that call keeps the
//!    harness's stub. Inventing an execution is the fault; a truthful "no result" is not.
//! 4. A late result (`late: true`) answers no call: it is told as what came back later.

use std::collections::{HashMap, HashSet};

use crate::node::{Body, Node};

/// The meta key of the harness's "Not run" for a use of a response that repeats its id: that response's node
/// id (`toolrun::calls`).
pub(crate) const REPEATED_IN: &str = "repeated_in";

/// A call: its assistant node's id and its provider id.
pub(crate) type Call<'n> = (&'n str, &'n str);

/// The association of results and calls among a run of nodes ([`pairing`]).
#[derive(Default)]
pub(crate) struct Pairing<'n> {
    by_call: HashMap<Call<'n>, &'n Node>,
    by_result: HashMap<&'n str, Call<'n>>,
    by_record: HashMap<&'n str, Call<'n>>,
    refused: HashMap<Call<'n>, &'n Node>,
}

impl<'n> Pairing<'n> {
    /// For a use of a response that repeats its id: the harness's own "Not run" for that response and id, which
    /// names the response it refused (rule 0).
    pub(crate) fn refusal(&self, call: Call<'_>) -> Option<&'n Node> {
        self.refused.get(&call).copied()
    }

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

/// The calls among a run of nodes, as their responses and the gate's records hold them.
#[derive(Default)]
struct Calls<'n> {
    /// Where each call stands, and its exchange (session and turn).
    at: HashMap<Call<'n>, (usize, &'n str, Option<&'n str>)>,
    /// The calls whose response gives their id to more than one use: never paired.
    repeated: HashSet<Call<'n>>,
    /// Each call's correlation, where its record has one.
    correlated: HashMap<Call<'n>, &'n str>,
    /// Each correlation's call.
    by_corr: HashMap<&'n str, Call<'n>>,
    /// Each record's call, by the record's node id.
    by_record: HashMap<&'n str, Call<'n>>,
    /// The records with no correlation: when each was written, its exchange, and its call.
    plain: Vec<(u64, &'n str, Option<&'n str>, Call<'n>)>,
}

fn calls<'n>(nodes: &[&'n Node]) -> Calls<'n> {
    let mut c = Calls::default();
    for (i, &n) in nodes.iter().enumerate() {
        let exchange = (i, n.session_id.as_str(), n.turn_id.as_deref());
        match &n.body {
            Body::AssistantMessage { blocks, .. } => {
                let mut seen = HashSet::new();
                for b in blocks.iter().filter(|b| b["type"] == "tool_use") {
                    let call = (n.id.as_str(), b["id"].as_str().unwrap_or_default());
                    if !seen.insert(call) {
                        c.repeated.insert(call);
                    }
                    c.at.entry(call).or_insert(exchange);
                }
            }
            Body::ToolCall {
                tool_use_id,
                assistant_node,
                correlation_id,
                ..
            } => {
                let call = (assistant_node.as_str(), tool_use_id.as_str());
                c.by_record.insert(n.id.as_str(), call);
                c.at.entry(call).or_insert(exchange);
                match correlation_id.as_deref() {
                    Some(corr) => {
                        c.correlated.insert(call, corr);
                        c.by_corr.insert(corr, call);
                    }
                    None => c
                        .plain
                        .push((n.created_at_ms, exchange.1, exchange.2, call)),
                }
            }
            _ => {}
        }
    }
    c
}

impl<'n> Calls<'n> {
    /// Rule 2: for result `r`, standing at `i`, with no correlation: the one call its exchange's uncorrelated
    /// records name with its id, written no later than it; else the one call with its id of its exchange
    /// standing before it, with no correlation. A call already answered or of a repeated id is no candidate;
    /// two candidates or more are no answer.
    fn own(
        &self,
        r: &Node,
        id: &str,
        i: usize,
        answered: &HashMap<Call<'n>, &'n Node>,
    ) -> Option<Call<'n>> {
        let own = (r.session_id.as_str(), r.turn_id.as_deref());
        let open = |call: &Call<'n>| {
            call.1 == id && !answered.contains_key(call) && !self.repeated.contains(call)
        };
        let recorded: HashSet<Call<'n>> = (self.plain.iter())
            .filter(|(at, session, turn, call)| {
                *at <= r.created_at_ms && (*session, *turn) == own && open(call)
            })
            .map(|(_, _, _, call)| *call)
            .collect();
        let candidates: HashSet<Call<'n>> = if recorded.is_empty() {
            (self.at.iter())
                .filter(|(call, (j, session, turn))| {
                    *j < i
                        && (*session, *turn) == own
                        && open(call)
                        && !self.correlated.contains_key(*call)
                })
                .map(|(call, _)| *call)
                .collect()
        } else {
            recorded
        };
        let mut only = candidates.into_iter();
        match (only.next(), only.next()) {
            (Some(call), None) => Some(call),
            _ => None,
        }
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
            meta,
            ..
        } = &n.body
        else {
            continue;
        };
        if let Some(response) = meta[REPEATED_IN].as_str() {
            p.refused.insert((response, tool_use_id.as_str()), n);
            continue;
        }
        let call = match correlation_id.as_deref() {
            Some(corr) => c.by_corr.get(corr).copied(),
            None => c.own(n, tool_use_id, i, &p.by_call),
        };
        let Some(call) = call.filter(|call| !c.repeated.contains(call)) else {
            continue;
        };
        // The same call answered again (its own correlation twice): its last result, as before.
        if let Some(before) = p.by_call.insert(call, n) {
            p.by_result.remove(before.id.as_str());
        }
        p.by_result.insert(n.id.as_str(), call);
    }
    p.by_record = c.by_record;
    p
}
