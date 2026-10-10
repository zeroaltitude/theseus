//! States rebuilt from the record (M5 25d; design §2.5, §2.9): a judged
//! point's builder input read back from what the session wrote, with the
//! live point's own input function, at the event's time as `now`. A replay
//! rebuilds a judgment's state when the candidate's builder, its version, or
//! its cap differ from the judgment's; a backfill rebuilds every event in its
//! window.
//!
//! `loop.v1`'s point is rebuilt from the turn's `turn.ended` row (its loops,
//! cost, tool calls, and stop reason) and the session's nodes up to the
//! turn's end (the ask, this turn's calls and their results, and the turn's
//! reply: its assistant messages' text, joined as the turn joins them), given
//! to `judge::loop_end::input`, the function the live point calls. Every
//! other builder's input is refused with the reason ([`unrebuildable`]).

use serde_json::Value;
use theseus_judge::builders::LoopInput;
use theseus_judge::pack::Builder;
use theseus_protocol::LedgerKind;

use crate::judge::loop_end::{self, LoopEnd};
use crate::ledger::LedgerRow;
use crate::node::{Body, Node};
use crate::rpc::Core;

/// Why a builder's input can't be rebuilt from the record; none when it
/// can.
pub fn unrebuildable(b: Builder) -> Option<&'static str> {
    match b {
        Builder::Loop => None,
        Builder::Security | Builder::Security2 => Some(
            "a gate state's input is the gate's decision at the call (posture and reason) and the \
             session's hold at that moment, which the record keeps only in part",
        ),
        Builder::Inbound => Some(
            "an inbound state's input reads the live tasks and the roles table as they stood at the \
             message, which the record does not keep by time",
        ),
        Builder::Continue => Some(
            "CONTINUE's input is one compile's signals beside the tail's tokens and the provider's \
             cache reads at that moment, which are not kept",
        ),
        Builder::Categorize => Some(
            "categorize's input reads the ontology's topics and memberships as they stood, which \
             the snapshot does not keep by time",
        ),
        Builder::Rerank => Some(
            "rerank's input is recall's candidates at that turn, read from the index as it stood \
             then, which is not kept",
        ),
        Builder::Probe => Some("the probe's state is synthetic: no point records it"),
        Builder::Memory | Builder::Attribution => Some(
            "the memory pass's states (a labeled node, or a reply and the notes recall admitted \
             to it) are not rebuilt from the record: only loop.v1's input is, so far",
        ),
        Builder::Citation => Some(
            "a citation check's input is a synthesis's text before it was kept, and a rejected \
             one is kept only in its row",
        ),
        Builder::People => Some(
            "a people state's candidate is an extractor's answer and the held people as they \
             stood, which the record keeps only in the judgment's context",
        ),
    }
}

/// A turn's end, as its `turn.ended` row records it.
#[derive(Debug, Clone, PartialEq)]
pub struct TurnEnd {
    pub session_id: String,
    pub turn_id: String,
    /// When its row was written: the event's time.
    pub at_ms: u64,
    pub loops: u32,
    pub cost_usd: Option<f64>,
    pub tool_calls: u32,
    pub stop_reason: String,
}

impl TurnEnd {
    /// From a `turn.ended` row, when it is one and names its turn.
    pub fn of(row: &LedgerRow) -> Option<TurnEnd> {
        if row.kind != LedgerKind::TurnEnded.as_str() {
            return None;
        }
        let d = &row.data;
        Some(TurnEnd {
            session_id: row.session_id.clone()?,
            turn_id: row.turn_id.clone()?,
            at_ms: row.at_unix_ms,
            loops: d["loops"].as_u64().unwrap_or(0) as u32,
            cost_usd: d["cost_usd"].as_f64(),
            tool_calls: d["tool_calls"].as_u64().unwrap_or(0) as u32,
            stop_reason: d["stop_reason"].as_str().unwrap_or_default().to_string(),
        })
    }
}

/// The turn's reply as the turn made it: each of its assistant messages'
/// text that is not empty, in order, joined by a blank line.
pub fn reply_of(nodes: &[Node], turn_id: &str) -> String {
    nodes
        .iter()
        .filter(|n| n.turn_id.as_deref() == Some(turn_id))
        .filter_map(|n| match &n.body {
            Body::AssistantMessage { blocks, .. } => {
                Some(crate::provider::text_of(blocks)).filter(|t| !t.is_empty())
            }
            _ => None,
        })
        .collect::<Vec<_>>()
        .join("\n\n")
}

/// `loop.v1`'s input for a turn, from its session's nodes (oldest first)
/// and its end, with the end's time as `now`: what the live point built at
/// the turn's end.
pub fn loop_input(nodes: &[Node], end: &TurnEnd, task: bool) -> LoopInput {
    let upto: Vec<Node> = nodes
        .iter()
        .filter(|n| n.created_at_ms <= end.at_ms)
        .cloned()
        .collect();
    let live = LoopEnd {
        session_id: end.session_id.clone(),
        execution_id: String::new(),
        turn_id: end.turn_id.clone(),
        task,
        output: reply_of(&upto, &end.turn_id),
        loops: end.loops,
        cost_usd: end.cost_usd,
        tool_calls: end.tool_calls,
    };
    loop_end::input(&upto, &live, end.at_ms)
}

impl Core {
    /// A session's `turn.ended` rows, oldest first, by one ledger page per
    /// hundred rows through the index. `Err` while the index's shape is
    /// built after a start.
    pub(crate) fn turn_ends(
        &self,
        session: Option<&str>,
        since_ms: u64,
    ) -> anyhow::Result<Vec<TurnEnd>> {
        let tags = crate::rpc::ledger_tags(Some(LedgerKind::TurnEnded.as_str()), session);
        let mut after = None;
        let mut out = Vec::new();
        loop {
            let page = theseus_store::Page {
                kind: theseus_store::kinds::LEDGER,
                tags: tags.clone(),
                after: Some(after.unwrap_or(0)),
                before: None,
                since_ms: Some(since_ms),
                until_ms: None,
                limit: 100,
            };
            let Some(got) = self.store.ledger_page(&page)? else {
                anyhow::bail!(
                    "the ledger's index is still being built after the start; try again in a minute"
                );
            };
            for r in &got.records {
                if let Ok(row) = r.decode::<LedgerRow>() {
                    out.extend(TurnEnd::of(&row));
                }
            }
            if !got.more || got.last.is_none() {
                return Ok(out);
            }
            after = got.last;
        }
    }

    /// Whether a session is a task's (its turns' class is `task`).
    pub(crate) fn is_task_session(&self, session: &str) -> bool {
        self.store
            .get_session::<crate::session::SessionRecord>(session)
            .ok()
            .flatten()
            .is_some_and(|r| r.task.is_some())
    }

    /// `loop.v1`'s input for a judged turn, rebuilt from the record: the
    /// turn's `turn.ended` row and its session's nodes. `Err`: why not.
    pub(crate) fn rebuild_loop(&self, context: &Value) -> Result<LoopInput, String> {
        let s = |k: &str| context.get(k).and_then(Value::as_str);
        let (Some(session), Some(turn)) = (s("session"), s("turn")) else {
            return Err("its judgment names no session and turn".into());
        };
        let end = self
            .turn_ends(Some(session), 0)
            .map_err(|e| format!("{e:#}"))?
            .into_iter()
            .find(|e| e.turn_id == turn)
            .ok_or_else(|| format!("no turn.ended row is recorded for turn {turn}"))?;
        let nodes: Vec<Node> = self
            .store
            .session_nodes(session)
            .map_err(|e| format!("its session's nodes were not read: {e:#}"))?
            .into_iter()
            .map(|(_, n)| n)
            .collect();
        Ok(loop_input(&nodes, &end, self.is_task_session(session)))
    }
}
