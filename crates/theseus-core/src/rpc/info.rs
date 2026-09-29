//! The protocol's views of kernel and store records: an execution, an action,
//! a node, and the kernel's counts, as clients render them.

use serde_json::{json, Value};

use super::Core;
use crate::node::{Body, Node};
use theseus_kernel::Execution;

impl Core {
    /// The kernel's view for health and `execution.list`.
    pub fn kernel_status(&self) -> theseus_protocol::KernelStatus {
        let st = self.kernel.stats().unwrap_or_default();
        theseus_protocol::KernelStatus {
            accepting: st.accepting,
            admission_ceiling: st.admission_ceiling,
            turns_held: st.turns_held,
            executions_by_state: st.executions_by_state,
            actions_by_state: st.actions_by_state,
            quarantined_completions: st.quarantined_completions,
            startup: self.startup_report.clone(),
            spend_limit_usd: self.cfg.kernel.spend_limit_usd,
        }
    }

    pub fn execution_info(e: &Execution) -> theseus_protocol::ExecutionInfo {
        use theseus_kernel::micros_to_usd as usd;
        let b = &e.budget;
        theseus_protocol::ExecutionInfo {
            execution_id: e.id.clone(),
            session_id: e.session_id.clone(),
            kind: e.kind.as_str().into(),
            state: e.state.as_str().into(),
            turns: e.turns,
            interrupted: e.interrupted,
            outstanding: e.outstanding.len() as u32,
            queued_results: e.queued_results.len() as u32,
            budget: theseus_protocol::BudgetInfo {
                limit_usd: usd(b.limit_micros),
                spent_usd: usd(b.spent_micros),
                reserved_usd: usd(b.reserved_micros),
                held_unknown_usd: usd(b.held_unknown_micros),
                available_usd: usd(b.available()),
                resets: b.resets,
                question: b.question.clone(),
                units_before: b
                    .units_before
                    .as_ref()
                    .map_or(Value::Null, |u| json!({"limit": u.limit, "spent": u.spent, "reserved": u.reserved, "held_unknown": u.held_unknown})),
            },
            wake: serde_json::to_value(&e.wake).unwrap_or(Value::Null),
            reports_to: e.reports_to.clone(),
            ended_reason: e.ended_reason.clone(),
            created_at_ms: e.created_at_ms,
            updated_at_ms: e.updated_at_ms,
        }
    }

    pub fn action_info(a: &theseus_kernel::Action) -> theseus_protocol::ActionInfo {
        theseus_protocol::ActionInfo {
            correlation_id: a.correlation_id.clone(),
            execution_id: a.execution_id.clone(),
            session_id: a.session_id.clone(),
            tool: a.tool.clone(),
            state: a.state.as_str().into(),
            retry_class: match &a.retry_class {
                theseus_kernel::RetryClass::SafeToRepeat => "safe_to_repeat".into(),
                theseus_kernel::RetryClass::NonRepeatable => "non_repeatable".into(),
            },
            planned_at_ms: a.planned_at_ms,
            authorized_at_ms: a.authorized_at_ms,
            dispatched_at_ms: a.dispatched_at_ms,
            settled_at_ms: a.settled_at_ms,
            deadline_at_ms: a.deadline_at_ms,
            reserved_usd: theseus_kernel::micros_to_usd(a.reserved_micros),
            confirmed: a.confirm.is_some(),
            cancel: a
                .cancel
                .and_then(|c| serde_json::to_value(c).ok())
                .and_then(|v| v.as_str().map(str::to_string)),
            external_op_id: a.external_op_id.clone(),
            result_ref: a.result_ref.clone(),
            resolution: a.resolution.clone(),
            completions_seen: a.completions_seen,
        }
    }

    pub fn node_info(position: u64, n: &Node) -> theseus_protocol::NodeInfo {
        let (text, thinking, detail, bytes) = match &n.body {
            Body::UserMessage { text, attachments } => {
                // What the node holds: the typed text and the text kept of each file.
                let bytes = text.len() as u64
                    + attachments
                        .iter()
                        .map(|a| match &a.content {
                            crate::node::AttachmentContent::Text { text, .. } => text.len() as u64,
                            _ => 0,
                        })
                        .sum::<u64>();
                let shown = crate::attach::display_text(text, attachments, n.author.as_deref());
                (shown, String::new(), Value::Null, bytes)
            }
            Body::AssistantMessage {
                blocks,
                model,
                provider,
                stop_reason,
                usage,
                cost_usd,
                request_id,
                correlation_id,
                compilation_id,
                ..
            } => {
                let calls: Vec<Value> = crate::provider::tool_uses_in(blocks)
                    .into_iter()
                    .map(|u| json!({"id": u.id, "name": u.name, "input": u.input}))
                    .collect();
                (
                    crate::provider::text_of(blocks),
                    crate::provider::thinking_of(blocks),
                    json!({"model": model, "provider": provider, "stop_reason": stop_reason, "usage": usage, "cost_usd": cost_usd, "request_id": request_id, "correlation_id": correlation_id, "compilation_id": compilation_id, "tool_calls": calls, "blocks": blocks.len()}),
                    serde_json::to_string(blocks)
                        .map(|s| s.len() as u64)
                        .unwrap_or(0),
                )
            }
            Body::ToolCall {
                tool_use_id,
                tool,
                input,
                correlation_id,
                gate,
                ..
            } => (
                String::new(),
                String::new(),
                json!({"tool_use_id": tool_use_id, "tool": tool, "input": input, "correlation_id": correlation_id, "decision": gate.get("decision"), "result": gate.get("result"), "plan": gate.get("plan")}),
                0,
            ),
            Body::ToolResult {
                tool_use_id,
                tool,
                status,
                is_error,
                content,
                correlation_id,
                bytes_total,
                truncated,
                full_ref,
                duration_ms,
                late,
                meta,
                image,
            } => (
                match image {
                    // The web UI and the CLI show the image's header line.
                    Some(img) => format!("{content}\n{}", crate::attach::header(img, None)),
                    None => content.clone(),
                },
                String::new(),
                json!({"tool_use_id": tool_use_id, "tool": tool, "status": status.as_str(), "is_error": is_error, "correlation_id": correlation_id, "truncated": truncated, "full_ref": full_ref, "duration_ms": duration_ms, "late": late, "meta": meta}),
                *bytes_total,
            ),
        };
        theseus_protocol::NodeInfo {
            node_id: n.id.clone(),
            kind: n.kind_str().into(),
            session_id: n.session_id.clone(),
            position,
            at_unix_ms: n.created_at_ms,
            turn_id: n.turn_id.clone(),
            loop_index: n.loop_index,
            author: n.author.clone(),
            text,
            thinking,
            detail,
            bytes,
        }
    }
}
