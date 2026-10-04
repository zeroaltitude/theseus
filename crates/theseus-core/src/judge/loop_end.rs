//! `loop.v1`'s input (design §2.4): a turn the baseline ended with no tool
//! calls, as the crate's plain `LoopInput`, from the turn's result and its
//! session's nodes. Read after the turn, off its path.

use theseus_judge::builders::{CallOutcome, LoopInput, SessionKind, ToolCallInput};

use crate::node::{Body, Node, Origin, ResultStatus};

/// What the turn's end hands the judge: its ids and its result's numbers.
#[derive(Debug, Clone)]
pub struct LoopEnd {
    pub session_id: String,
    pub execution_id: String,
    pub turn_id: String,
    pub task: bool,
    /// The turn's reply.
    pub output: String,
    pub loops: u32,
    pub cost_usd: Option<f64>,
    pub tool_calls: u32,
}

impl LoopEnd {
    /// The turn's workload class (§2.2), set deterministically.
    pub fn class(&self) -> &'static str {
        class(self.task, self.tool_calls)
    }
}

/// A turn's workload class (§2.2) from whether it is a task's, and its
/// tool calls.
pub fn class(task: bool, tool_calls: u32) -> &'static str {
    match (task, tool_calls) {
        (true, _) => "task",
        (false, 0) => "reply",
        (false, _) => "tools",
    }
}

/// The input, from the session's nodes, oldest first. The ask is the
/// newest message the operator wrote (a task's is its brief, its first
/// message); the calls are this turn's, each with its result's outcome.
pub fn input(nodes: &[Node], end: &LoopEnd, now_ms: u64) -> LoopInput {
    let user = |n: &&Node| matches!(n.body, Body::UserMessage { .. });
    let ask = if end.task {
        nodes.iter().find(user)
    } else {
        nodes
            .iter()
            .rev()
            .filter(user)
            .find(|n| n.origin == Origin::Operator)
            .or_else(|| nodes.iter().rev().find(user))
    };
    let ask_text = match ask.map(|n| &n.body) {
        Some(Body::UserMessage { text, .. }) => text.clone(),
        _ => String::new(),
    };
    let minutes_since_ask = ask.map_or(0, |n| now_ms.saturating_sub(n.created_at_ms) / 60_000);
    let turn = Some(end.turn_id.as_str());
    let mut tool_calls = Vec::new();
    for n in nodes.iter().filter(|n| n.turn_id.as_deref() == turn) {
        let Body::ToolCall {
            tool_use_id,
            tool,
            input,
            ..
        } = &n.body
        else {
            continue;
        };
        let result = nodes.iter().rev().find_map(|r| match &r.body {
            Body::ToolResult {
                tool_use_id: id,
                status,
                ..
            } if id == tool_use_id => Some(*status),
            _ => None,
        });
        let (outcome, error_class) = match result {
            Some(ResultStatus::Ok) => (CallOutcome::Ok, None),
            Some(ResultStatus::Error) => (CallOutcome::Error, Some("error".to_string())),
            Some(ResultStatus::Unknown) => (CallOutcome::Error, Some("unknown".to_string())),
            Some(ResultStatus::Declined) => (CallOutcome::Held, None),
            Some(ResultStatus::Cancelled) => (CallOutcome::Error, Some("cancelled".to_string())),
            Some(ResultStatus::Background) | None => (CallOutcome::Running, None),
        };
        tool_calls.push(ToolCallInput {
            tool: tool.clone(),
            args: input.clone(),
            outcome,
            error_class,
        });
    }
    LoopInput {
        session_kind: if end.task {
            SessionKind::Task
        } else {
            SessionKind::Conversation
        },
        ask: ask_text,
        final_text: end.output.clone(),
        tool_calls,
        loops: end.loops,
        spend_usd: end.cost_usd.unwrap_or(0.0),
        minutes_since_ask,
    }
}
