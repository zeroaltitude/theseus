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
            spend_limit_mode: Some(self.cfg.kernel.spend_limit_mode.as_str().into()),
            lingering_wrappers: self.spool.lingering().len() as u64,
        }
    }

    /// The context files every session gets, and the persona in play with
    /// its own (theseus-c48), as the config names them.
    pub fn context_status(&self) -> theseus_protocol::ContextStatus {
        let persona = self.cfg.persona();
        theseus_protocol::ContextStatus {
            system_files: paths(&self.cfg.context.files),
            persona: persona.map(str::to_string),
            persona_files: persona
                .and_then(|p| self.cfg.personas.get(p))
                .map(|p| paths(&p.files))
                .unwrap_or_default(),
            personas: self.cfg.personas.keys().cloned().collect(),
        }
    }

    /// The daemon's children (theseus-z4b): its job wrappers, running or
    /// lingering (a wrapper whose `spool/lingering` mark names it), the
    /// orphans it adopted, and its zombies, 0 in steady state.
    pub fn children_status(&self) -> theseus_protocol::ChildrenStatus {
        let c = theseus_kernel::children::census();
        let marked: std::collections::HashSet<u32> = self
            .spool
            .lingering()
            .into_iter()
            .map(|(_, pid)| pid)
            .collect();
        let lingering = c
            .wrappers
            .iter()
            .filter(|(pid, _)| marked.contains(pid))
            .count() as u64;
        theseus_protocol::ChildrenStatus {
            subreaper: c.subreaper,
            wrappers_running: c.wrappers.len() as u64 - lingering,
            wrappers_lingering: lingering,
            orphans: c.orphans,
            zombies: c.zombies,
            owned: c.owned,
            reaped_wrappers: c.reaped_wrappers,
            reaped_orphans: c.reaped_orphans,
            tenders: self.index.status().into_iter().collect(),
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
            waiting_on: e.wake.as_ref().map(crate::push::waiting_on),
            reports_to: e.reports_to.clone(),
            ended_reason: e.ended_reason.clone(),
            created_at_ms: e.created_at_ms,
            updated_at_ms: e.updated_at_ms,
            attention: None,
        }
    }

    pub fn action_info(a: &theseus_kernel::Action) -> theseus_protocol::ActionInfo {
        theseus_protocol::ActionInfo {
            correlation_id: a.correlation_id.clone(),
            execution_id: a.execution_id.clone(),
            session_id: a.session_id.clone(),
            tool: a.tool.clone(),
            state: a.state.as_str().into(),
            retry_class: a.retry_class.as_str().into(),
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
            verdict: a.verdict.as_ref().map(|v| crate::cancel::wire(a, v)),
            external_op_id: a.external_op_id.clone(),
            // A node's id; a job's is the path of its raw output in the
            // daemon's spool, which stays out of what clients get
            // (theseus-wz2).
            result_ref: a.result_ref.clone().filter(|r| !r.starts_with('/')),
            resolution: a.resolution.clone(),
            completions_seen: a.completions_seen,
            egress: None,
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
            Body::Arrangement { .. } => arrangement_info(n),
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
                with_egress(
                    json!({"tool_use_id": tool_use_id, "tool": tool, "input": input, "correlation_id": correlation_id,
                        "decision": gate.as_ref().and_then(|g| g.decision.as_ref()),
                        "result": gate.as_ref().map(|g| &g.result),
                        "plan": gate.as_ref().and_then(|g| g.plan.as_ref())}),
                    gate.as_deref(),
                ),
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
                duration_ms,
                late,
                meta,
                image,
                external,
                // An older node's `full_ref` is a path in the daemon's spool:
                // it stays out of what clients get (theseus-wz2).
                ..
            } => (
                match image {
                    // The web UI and the CLI show the image's header line.
                    Some(img) => format!("{content}\n{}", crate::attach::header(img, None)),
                    None => content.clone(),
                },
                String::new(),
                json!({"tool_use_id": tool_use_id, "tool": tool, "status": status.as_str(), "is_error": is_error, "correlation_id": correlation_id, "truncated": truncated, "duration_ms": duration_ms, "late": late, "meta": meta, "external": external}),
                *bytes_total,
            ),
            // References, never copies (M6 30b): the sources, each with its
            // header and range.
            Body::Recall { .. } => (n.preview(80), String::new(), recall_detail(n), 0),
            // A compaction's summary (30c): its testimony, and its range.
            Body::Summary { .. } => summary_info(n),
            // Consolidation's synthesis (31b), and the import's (theseus-0lrr.6).
            _ => later_info(n),
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

/// Context files' paths, as health names them.
fn paths(files: &[crate::context_files::ContextEntry]) -> Vec<String> {
    files.iter().map(|f| f.path().to_string()).collect()
}

/// Consolidation's synthesis (M6 31b): its cited text, and its sources,
/// check, and stage as its detail.
fn synthesis_info(n: &Node) -> (String, String, Value, u64) {
    let Body::Synthesis {
        text,
        sources,
        check,
        stage,
        cluster,
        profile,
        model,
        cost_usd,
    } = &n.body
    else {
        return (n.preview(80), String::new(), Value::Null, 0);
    };
    (
        text.clone(),
        String::new(),
        json!({"sources": sources, "check": check, "stage": stage, "cluster": cluster,
               "profile": profile, "model": model, "cost_usd": cost_usd}),
        0,
    )
}

/// An imported node (theseus-0lrr.6): its text and where it came from; a
/// summary's citations; a tombstone's receipt (§5.6), and what it replaced.
/// With a synthesis, `node_info`'s last arm, so it stays within clippy's
/// length.
fn later_info(n: &Node) -> (String, String, Value, u64) {
    match &n.body {
        // Consolidation's synthesis (31b): its text, its sources and check.
        Body::Synthesis { .. } => synthesis_info(n),
        Body::Imported {
            text,
            integrity,
            source,
            unit,
            sha256,
            idx,
        } => (
            text.clone(),
            String::new(),
            json!({"integrity": integrity.as_str(), "source": source, "unit": unit, "sha256": sha256, "idx": idx}),
            text.len() as u64,
        ),
        Body::ImportedSummary { text, cites, model } => (
            text.clone(),
            String::new(),
            json!({"cites": cites, "model": model}),
            text.len() as u64,
        ),
        Body::Erased { was, at_ms, why } => (
            n.preview(80),
            String::new(),
            json!({"was": was, "at_ms": at_ms, "why": why}),
            0,
        ),
        _ => (n.preview(80), String::new(), Value::Null, 0),
    }
}

/// A compaction's summary (M6 30c): its testimony as its text, and its
/// range as its detail.
fn summary_info(n: &Node) -> (String, String, Value, u64) {
    let Body::Summary {
        first,
        last,
        nodes,
        text,
        profile,
        model,
        cost_usd,
        header,
    } = &n.body
    else {
        return (String::new(), String::new(), Value::Null, 0);
    };
    (
        format!("{header}\n{text}"),
        String::new(),
        json!({"first": first, "last": last, "nodes": nodes, "profile": profile,
            "model": model, "cost_usd": cost_usd}),
        text.len() as u64,
    )
}

/// A `Recall` node's detail (M6 30b): its references, never copies: each
/// source with its header and range.
fn recall_detail(n: &Node) -> Value {
    let Body::Recall {
        recall_id,
        arm,
        items,
    } = &n.body
    else {
        return Value::Null;
    };
    let items: Vec<Value> = items
        .iter()
        .map(|r| {
            json!({"node_id": r.node_id, "session_id": r.session_id, "position": r.position,
                   "chunk": [r.chunk.0, r.chunk.1], "header": r.header, "tokens": r.tokens})
        })
        .collect();
    json!({"recall_id": recall_id, "arm": arm, "items": items})
}

/// A tool call's detail with an L1 call's egress list, as its proposal binds
/// it (18c): the web UI's L1 pill names its hosts.
fn with_egress(
    mut d: serde_json::Value,
    gate: Option<&theseus_protocol::GateRecord>,
) -> serde_json::Value {
    let egress = gate.map(|g| theseus_protocol::sandbox::egress_in(&g.proposal.policy_context));
    if let Some(e) = egress.filter(|e| !e.is_empty()) {
        d["egress"] = json!(e);
    }
    d
}

/// A task's arrangement node, as `node_info` shows it (M5 27): its render,
/// and its pieces by reference.
fn arrangement_info(n: &Node) -> (String, String, Value, u64) {
    let Body::Arrangement {
        pieces,
        fidelity_ack,
        claim,
    } = &n.body
    else {
        return (String::new(), String::new(), Value::Null, 0);
    };
    let text = crate::check::render(pieces, claim.as_ref());
    let bytes = text.len() as u64;
    let mut detail =
        json!({"pieces": crate::arrangement::meta(pieces), "fidelity_ack": fidelity_ack});
    // A check's claim, by reference (M5 28a).
    if let Some(c) = claim {
        detail["claim"] = json!({"task": c.task, "node": c.node, "at_ms": c.at_ms});
    }
    (text, String::new(), detail, bytes)
}
