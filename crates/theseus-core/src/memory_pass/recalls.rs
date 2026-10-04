//! The pass's attribution of recalled items (M6 §2.6, step 31a): which of a
//! session's `Recall` nodes still have items without a `memory.used` row,
//! what their turn did, and what the session said next; then a row for each
//! item whose answer is known. An item not used is written at once (it is
//! no review); a used one waits for the session's next input, which gives
//! its outcome. Only recalls of turns that have ended are read: the job's
//! own turn, and those before it.

use std::collections::BTreeSet;
use std::sync::Arc;

use super::attribution::{self, Next, Turned};
use super::{Done, MemoryPass, Unit};
use crate::fact::memory::MemoryUsed;
use crate::node::{Body, Node, Origin, RecalledRef};
use crate::store::Transcript;

/// A recall with items still to attribute.
pub(super) struct Pending {
    pub recall_id: String,
    pub arm: String,
    pub session_id: String,
    pub turn_id: Option<String>,
    pub items: Vec<(RecalledRef, String)>,
    /// The turn's reply, and its calls' inputs.
    pub reply: String,
    pub calls: String,
    /// The session's next input after the turn; `None`: none yet.
    pub next: Option<NextInput>,
}

pub(super) enum NextInput {
    Operator(String),
    Other,
}

impl Pending {
    /// The texts whose entities attribution needs, in the order
    /// `units` reads them: each item's excerpt, the reply, the
    /// calls, and the operator's next message.
    pub fn texts(&self, out: &mut Vec<String>) {
        out.extend(self.items.iter().map(|(_, e)| e.clone()));
        out.push(self.reply.clone());
        out.push(self.calls.clone());
        if let Some(NextInput::Operator(t)) = &self.next {
            out.push(t.clone());
        }
    }
}

/// The recalls of `nodes` (ended turns only) whose items are not all in
/// `done`, with the items left: read under the pass's lock, so no store
/// read here.
pub(super) fn waiting(
    nodes: &Transcript,
    turn_id: &str,
    done: &Done,
) -> Vec<(usize, Arc<Node>, Vec<RecalledRef>)> {
    let first_of_job = nodes
        .iter()
        .find(|(_, n)| n.turn_id.as_deref() == Some(turn_id))
        .map(|(p, _)| *p);
    let mut out = Vec::new();
    for (i, (pos, n)) in nodes.iter().enumerate() {
        let Body::Recall {
            recall_id, items, ..
        } = &n.body
        else {
            continue;
        };
        let ended = n.turn_id.as_deref() == Some(turn_id) || first_of_job.is_some_and(|f| *pos < f);
        if !ended {
            continue;
        }
        let left: Vec<RecalledRef> = items
            .iter()
            .filter(|r| !done.used.contains(&(recall_id.clone(), r.node_id.clone())))
            .cloned()
            .collect();
        if !left.is_empty() {
            out.push((i, n.clone(), left));
        }
    }
    out
}

impl MemoryPass {
    /// Read each waiting recall's sources and its turn.
    pub(super) fn resolve(
        &self,
        nodes: &Transcript,
        waiting: Vec<(usize, Arc<Node>, Vec<RecalledRef>)>,
    ) -> Vec<Pending> {
        let mut out = Vec::new();
        for (i, n, left) in waiting {
            let Body::Recall { recall_id, arm, .. } = &n.body else {
                continue;
            };
            let turn = n.turn_id.clone();
            let mut items = Vec::new();
            for r in left {
                let excerpt = theseus_store::blocking(|| self.memory.read_source(&self.store, &r))
                    .map(|src| excerpt_of(&crate::recall::text_of(&src), r.chunk))
                    .unwrap_or_default();
                items.push((r, excerpt));
            }
            let in_turn = |m: &Node| turn.is_some() && m.turn_id == turn;
            let mut reply = Vec::new();
            let mut calls = Vec::new();
            for (_, m) in nodes {
                if !in_turn(m) {
                    continue;
                }
                match &m.body {
                    Body::AssistantMessage { blocks, .. } => {
                        reply.push(crate::provider::text_of(blocks))
                    }
                    Body::ToolCall { input, .. } => calls.push(input.to_string()),
                    _ => {}
                }
            }
            let last = nodes.iter().rposition(|(_, m)| in_turn(m)).unwrap_or(i);
            let next = nodes[last + 1..].iter().find_map(|(_, m)| match &m.body {
                Body::UserMessage { text, .. } if m.turn_id != turn => Some(match m.origin {
                    Origin::Operator => NextInput::Operator(text.clone()),
                    _ => NextInput::Other,
                }),
                _ => None,
            });
            out.push(Pending {
                recall_id: recall_id.clone(),
                arm: arm.clone(),
                session_id: n.session_id.clone(),
                turn_id: turn,
                items,
                reply: reply.join("\n"),
                calls: calls.join("\n"),
                next,
            });
        }
        out
    }
}

/// The rows of `r`'s items whose answer is known, reading their entities
/// from `entities` at `*at` (moved past `r`'s texts).
pub(super) fn units(
    r: &Pending,
    entities: &[Vec<String>],
    at: &mut usize,
    unavailable: Option<&str>,
) -> Vec<Unit> {
    let set = |i: usize| -> BTreeSet<String> {
        entities
            .get(i)
            .map(|e| e.iter().cloned().collect())
            .unwrap_or_default()
    };
    let base = *at;
    let n = r.items.len();
    let reply_e = set(base + n);
    let call_e = set(base + n + 1);
    let next_e = set(base + n + 2);
    *at += n + 2 + usize::from(matches!(r.next, Some(NextInput::Operator(_))));
    let turned = Turned {
        reply: &r.reply,
        reply_entities: &reply_e,
        call_entities: &call_e,
    };
    let mut units = Vec::new();
    for (k, (item, excerpt)) in r.items.iter().enumerate() {
        let mine = set(base + k);
        let u = attribution::used(excerpt, &mine, &turned);
        let outcome = match (&u.used, &r.next) {
            (false, _) => None,
            (true, None) => continue,
            (true, Some(NextInput::Other)) => {
                Some(attribution::outcome(excerpt, &mine, &Next::Other))
            }
            (true, Some(NextInput::Operator(t))) => Some(attribution::outcome(
                excerpt,
                &mine,
                &Next::Operator(t, &next_e),
            )),
        };
        let f = MemoryUsed {
            recall_id: &r.recall_id,
            arm: &r.arm,
            node_id: &item.node_id,
            source_session: &item.session_id,
            used: &u,
            outcome,
            entities_unavailable: unavailable,
        };
        match crate::fact::row(&f, Some(&r.session_id), r.turn_id.as_deref()) {
            Ok(row) => units.push(Unit {
                session_id: r.session_id.clone(),
                records: vec![row.scoped(&crate::fact::recall::scope(&r.session_id))],
                labeled: None,
                used: Some((r.recall_id.clone(), item.node_id.clone())),
            }),
            Err(e) => {
                tracing::warn!(error = %format!("{e:#}"), "memory: a used row cannot be encoded")
            }
        }
    }
    units
}

/// The text a recall showed of its source: the frozen byte range, or the
/// whole text when the range does not fit it.
fn excerpt_of(text: &str, (start, end): (u32, u32)) -> String {
    let (s, e) = (start as usize, end as usize);
    if s < e && e <= text.len() && text.is_char_boundary(s) && text.is_char_boundary(e) {
        text[s..e].to_string()
    } else {
        text.to_string()
    }
}
