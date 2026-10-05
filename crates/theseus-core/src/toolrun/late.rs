//! Results no call's own run writes: a background job's late result,
//! taken by a later turn (`absorb`), and the answers a cancelled
//! execution's calls get (`answer_after_cancel`, `not_run_results`).
//! Split from `toolrun.rs` (theseus-5gw9).

use std::collections::HashSet;

use anyhow::Result;
use serde_json::{json, Value};
use theseus_kernel::{
    Action, ActionState, CancelState, ExecState, Execution, Kernel, BUDGET_TOOL, PROVIDER_TOOL,
};
use theseus_tools::Backend;

use super::{not_run_answer, unanswered, ResultNode, ToolRuntime, TurnCtx};
use crate::egress;
use crate::fact;
use crate::node::{Body, Node, ResultStatus};
use crate::store::Store;

/// A background job's late result, as `absorb` wrote it: the turn traces it
/// as its call's answer (theseus-8pei).
#[derive(Debug, Clone)]
pub struct LateCall {
    pub tool_use_id: String,
    /// The call's wire name, as its span is named (`proc_run`).
    pub wire: String,
    pub status: ResultStatus,
    /// The job's run, its dispatch to its settle, in ms.
    pub run_ms: Option<u64>,
}

impl ToolRuntime {
    /// Take the settled actions queued for this execution since its last
    /// turn: a background job's real result becomes a late result node, in
    /// the frame that takes it from the queue (theseus-kol), so a crash
    /// between the two cannot lose it. Returns what was taken, and each late
    /// result written, for its span (theseus-8pei).
    pub fn absorb(&self, tc: &TurnCtx<'_>) -> Result<(Vec<Action>, Vec<LateCall>)> {
        // A late result that is outside text (a job that connected out of
        // L1, 18c) brings its session's hold in this frame too.
        let (settled, late, outputs, newly) =
            crate::external::under_hold(tc.store, tc.session_id, |hold| {
                let (mut late, mut outputs, mut newly) = (Vec::new(), Vec::new(), None);
                let settled = tc.kernel.take_results_with(tc.guard, |settled| {
                    let (nodes, mut records, raw) = self.late_results(tc, settled)?;
                    if let Some((h, more)) = hold.of(&nodes, Some(tc.turn_id))? {
                        records.extend(more);
                        newly = Some(h);
                    }
                    (late, outputs) = (nodes, raw);
                    Ok(records)
                })?;
                Ok((settled, late, outputs, newly))
            })?;
        for node in &late {
            egress::announce(&tc.rec(), &self.sandbox, node);
            Self::announce_end(tc, node);
        }
        self.held(tc, newly);
        // Their nodes are written, so the jobs' raw output goes, as for a
        // result read within a turn (`answer_job`, theseus-wz2), a stopped
        // job's included (theseus-ewev).
        for a in &outputs {
            self.remove_job_output(a);
        }
        let calls = late
            .iter()
            .zip(&outputs)
            .filter_map(|(n, a)| self.late_call(n, a));
        Ok((settled, calls.collect()))
    }

    /// A late result's call, from its node and its job's action: the job
    /// ran from its dispatch to its settle, before this turn's trace began.
    fn late_call(&self, node: &Node, a: &Action) -> Option<LateCall> {
        let Body::ToolResult {
            tool_use_id,
            tool,
            status,
            duration_ms,
            ..
        } = &node.body
        else {
            return None;
        };
        let run = a.settled_at_ms.zip(a.dispatched_at_ms);
        Some(LateCall {
            tool_use_id: tool_use_id.clone(),
            wire: self
                .registry
                .get(tool)
                .map_or_else(|| theseus_tools::wire_name(tool), |t| t.wire_name()),
            status: *status,
            run_ms: run
                .map(|(end, start)| end.saturating_sub(start))
                .or(*duration_ms),
        })
    }

    /// The late results among `settled`: for each job whose call was
    /// answered `background` and has no late result yet, its result's node
    /// and its `tool.late_result` row, for the frame that takes it from the
    /// queue, and its action, whose raw output goes once that frame is
    /// written.
    fn late_results(
        &self,
        tc: &TurnCtx<'_>,
        settled: &[Action],
    ) -> Result<(Vec<Node>, Vec<theseus_store::NewRecord>, Vec<Action>)> {
        let (mut late, mut records, mut outputs) = (Vec::new(), Vec::new(), Vec::new());
        let jobs: Vec<&Action> = settled
            .iter()
            .filter(|a| a.tool != PROVIDER_TOOL && a.tool != BUDGET_TOOL)
            .collect();
        if jobs.is_empty() {
            return Ok((late, records, outputs));
        }
        let nodes = tc.store.transcript(tc.session_id)?;
        // From the end, by kind: a stub decodes only the results it is
        // asked about (M6 step 33), and a late result follows its placeholder.
        let results = |from: usize| {
            nodes[from..]
                .iter()
                .enumerate()
                .rev()
                .filter(|(_, (_, n))| n.kind == crate::stub::Kind::ToolResult)
                .map(move |(i, (_, n))| (from + i, n))
        };
        for a in jobs {
            let placeholder = results(0).find_map(|(i, node)| match &node.body {
                Body::ToolResult {
                    tool_use_id,
                    tool,
                    status: ResultStatus::Background,
                    correlation_id: Some(c),
                    late: false,
                    ..
                } if c == &a.correlation_id => Some((i, tool_use_id.clone(), tool.clone())),
                _ => None,
            });
            let Some((at, tool_use_id, tool)) = placeholder else {
                continue;
            };
            let already = results(at + 1).any(|(_, node)| matches!(&node.body, Body::ToolResult { tool_use_id: t, late: true, .. } if *t == tool_use_id));
            if already {
                continue;
            }
            let input = call_input(&nodes, &a.correlation_id);
            let mut r = self.job_result(tc.store, a, &tool_use_id, &tool, input);
            // A batch's step that ran on in the background (theseus-7gir.3).
            if input.is_some_and(|i| i.get("steps").is_some()) {
                r.text = format!("[the batch's step that went on in the background; the steps after it were not run]\n{}", r.text);
            }
            let node = self.result_node(tc, ResultNode { late: true, ..r });
            records.push(node.record()?);
            records.extend(egress::rows(&tc.rec(), &node)?);
            records.push(tc.rec().row(&fact::tool::LateResult {
                correlation_id: &a.correlation_id,
                tool: &tool,
                state: a.state,
            })?);
            late.push(node);
            outputs.push(a.clone());
        }
        Ok((late, records, outputs))
    }

    /// Answer what a cancelled execution left unanswered in its transcript
    /// (theseus-0o8): the calls of its last assistant message that no result
    /// answers, and the jobs it ended whose placeholder never got their end.
    /// A cancelled execution takes no more turns, so no turn writes them. It
    /// runs where a cancel's last work is done, once no turn holds the
    /// execution: after the cancel has stopped what it could
    /// (`Core::cancel_execution`), and at the end of the turn that held the
    /// execution when it came, which owns its transcript until then. It
    /// runs under the execution's lock and answers each call once, so those
    /// two cannot both write one. Returns the nodes written, for the caller
    /// to announce.
    pub fn answer_after_cancel(
        &self,
        kernel: &Kernel,
        store: &Store,
        session_id: &str,
        execution_id: &str,
    ) -> Result<Vec<Node>> {
        let (mut written, mut outputs) = (Vec::new(), Vec::new());
        // What a job brought from outside brings its hold in this frame (18c).
        crate::external::under_hold(store, session_id, |hold| {
            kernel.frame(&[execution_id], |k| {
                let Some(e) = k.execution(execution_id)? else {
                    return Ok(());
                };
                if e.state != ExecState::Cancelled || k.holds_turn(execution_id) {
                    return Ok(());
                }
                let (nodes, raw) = self.cancelled_results(k, store, session_id, &e)?;
                let mut records = nodes.iter().map(Node::record).collect::<Result<Vec<_>>>()?;
                records.extend(
                    hold.of(&nodes, None)?
                        .map(|(_, more)| more)
                        .unwrap_or_default(),
                );
                k.stage(&records)?;
                (written, outputs) = (nodes, raw);
                Ok(())
            })
        })?;
        // The nodes are written, so the jobs' raw output goes, as for a
        // result read within a turn (`answer_job`, theseus-wz2).
        for a in &outputs {
            self.remove_job_output(a);
        }
        Ok(written)
    }

    /// The result nodes for `answer_after_cancel`, and the jobs whose raw output
    /// they read.
    fn cancelled_results(
        &self,
        kernel: &Kernel,
        store: &Store,
        session_id: &str,
        e: &Execution,
    ) -> Result<(Vec<Node>, Vec<Action>)> {
        let nodes: crate::store::Transcript = store
            .session_nodes(session_id)?
            .into_iter()
            .map(|(pos, n)| (pos, n.into()))
            .collect();
        let (mut out, mut raw) = (Vec::new(), Vec::new());
        let mut write = |at: &Node, r: ResultNode<'_>, job: Option<&Action>| {
            out.push(self.result_node_in(session_id, at.turn_id.as_deref(), at.loop_index, r));
            raw.extend(job.cloned());
        };
        // The last assistant message's calls that nothing answers: planned and
        // ended by the cancel, dispatched and stopped by it, or never planned.
        if let Some((assistant, pending)) = unanswered(&nodes) {
            for u in pending {
                let call = nodes.iter().find_map(|(_, n)| match &n.body {
                    Body::ToolCall { tool_use_id, .. } if *tool_use_id == u.id => Some(&**n),
                    _ => None,
                });
                let corr = call.and_then(|n| match &n.body {
                    Body::ToolCall {
                        correlation_id: Some(c),
                        ..
                    } => Some(c.as_str()),
                    _ => None,
                });
                let action = corr.map(|c| kernel.action(c)).transpose()?.flatten();
                let (_, tool) = self.tool_of(&u);
                let answer = match &action {
                    Some(a) => self.cancelled_call(store, e, a, &u.id, &tool, Some(&u.input)),
                    None => Some((
                        ResultNode::new(
                            &u.id,
                            &tool,
                            ResultStatus::Cancelled,
                            format!("Not run: {}.", cancel_why(e)),
                        ),
                        false,
                    )),
                };
                if let Some((r, job)) = answer {
                    write(
                        call.unwrap_or(assistant),
                        r,
                        job.then_some(action.as_ref()).flatten(),
                    );
                }
            }
        }
        // A job answered `background` and ended by the cancel, or settled
        // before it and never taken: the end a later turn writes as a late
        // result (theseus-kol), which a cancelled execution never takes.
        let ended: HashSet<&str> = nodes
            .iter()
            .filter_map(|(_, n)| match &n.body {
                Body::ToolResult {
                    tool_use_id,
                    late: true,
                    ..
                } => Some(tool_use_id.as_str()),
                _ => None,
            })
            .collect();
        for (_, n) in &nodes {
            let Body::ToolResult {
                tool_use_id,
                tool,
                status: ResultStatus::Background,
                correlation_id: Some(c),
                late: false,
                ..
            } = &n.body
            else {
                continue;
            };
            if ended.contains(tool_use_id.as_str()) {
                continue;
            }
            let Some(a) = kernel.action(c)? else {
                continue;
            };
            let input = call_input(&nodes, c);
            if let Some((r, job)) = self.cancelled_call(store, e, &a, tool_use_id, tool, input) {
                write(n, ResultNode { late: true, ..r }, job.then_some(&a));
            }
        }
        Ok((out, raw))
    }

    /// How one call of a cancelled execution ended, as its result: the
    /// result, and whether it is a job's, whose raw output goes once its node
    /// is written. None while the cancel is still stopping it: the sweep
    /// after the call settles answers it. `input` is the call's, for a job's
    /// result (`job_result`).
    fn cancelled_call<'a>(
        &self,
        store: &Store,
        e: &Execution,
        a: &'a Action,
        tool_use_id: &'a str,
        tool: &'a str,
        input: Option<&Value>,
    ) -> Option<(ResultNode<'a>, bool)> {
        let job = self
            .registry
            .get(tool)
            .is_some_and(|t| t.backend() == Backend::Job);
        let why = cancel_why(e);
        let says = |status, text: String, cancel: &str| {
            let mut meta = json!({"cancel": cancel});
            crate::cancel::stopped_meta(&mut meta, status, a);
            ResultNode {
                correlation_id: Some(&a.correlation_id),
                meta,
                ..ResultNode::new(tool_use_id, tool, status, text)
            }
        };
        Some(match (a.state, a.cancel) {
            // Still being stopped.
            (ActionState::Dispatched, _) => return None,
            // Never sent: ended in the cancel's frame, or declined before it.
            (ActionState::Planned | ActionState::Authorized, _)
            | (ActionState::Cancelled, None) => {
                let (status, text) = if a.state == ActionState::Cancelled {
                    not_run_answer(a)
                } else {
                    (ResultStatus::Cancelled, format!("Not run: {why}."))
                };
                let mut meta = Value::Null;
                crate::cancel::stopped_meta(&mut meta, status, a);
                let r = ResultNode {
                    correlation_id: Some(&a.correlation_id),
                    meta,
                    ..ResultNode::new(tool_use_id, tool, status, text)
                };
                (r, false)
            }
            // Told to stop while it ran. What a job printed before it stopped
            // is in its result; a call that cannot be stopped, or that was
            // not verified gone, may have run: unknown, never "not sent".
            (ActionState::Cancelled, Some(c)) => {
                let (status, how, tag) = match c {
                    CancelState::TerminationVerified => (
                        ResultStatus::Cancelled,
                        // A job's head line says how; another call's line does.
                        crate::cancel::words(a)
                            .filter(|_| !job)
                            .map_or("it was stopped".into(), |w| format!("it was stopped ({w})")),
                        "stopped",
                    ),
                    CancelState::Unsupported => (
                        ResultStatus::Unknown,
                        "it cannot be stopped once started, so it may have finished: check the \
                         current state before relying on it"
                            .to_string(),
                        "unsupported",
                    ),
                    _ => (
                        ResultStatus::Unknown,
                        format!(
                            "it was told to stop, but its end was not verified{}, so it may still be \
                             running: check the current state before relying on it",
                            a.verdict.as_ref().and_then(|v| v.why.as_deref()).map_or(String::new(), |w| format!(" ({w})"))
                        ),
                        "uncertain",
                    ),
                };
                let line = format!("{} while this call was running; {how}.", sentence(&why));
                if job {
                    let mut r = self.job_result(store, a, tool_use_id, tool, input);
                    r.status = status;
                    r.text = format!("[{line}]\n{}", r.text);
                    r.meta["cancel"] = json!(tag);
                    (r, true)
                } else {
                    (says(status, line, tag), false)
                }
            }
            // Settled before the cancel, its result never read: a job's is
            // in the spool; an in-process call's was written with its
            // settle, so only a lost one reaches here.
            (ActionState::Succeeded | ActionState::Failed | ActionState::OutcomeUnknown, _) => {
                if job {
                    (self.job_result(store, a, tool_use_id, tool, input), true)
                } else {
                    let line = format!(
                        "{}. This call settled as {} and its result was never recorded: check \
                         the current state before relying on it.",
                        sentence(&why),
                        a.state.as_str()
                    );
                    (says(ResultStatus::Unknown, line, "settled"), false)
                }
            }
        })
    }
}

/// The input of the call `correlation_id` names, from its tool-call node in
/// `nodes`: what a job's result reads to mark a listed program's
/// (theseus-b5cl).
fn call_input<'n>(nodes: &'n crate::store::Transcript, correlation_id: &str) -> Option<&'n Value> {
    let mut calls = nodes
        .iter()
        .rev()
        .filter(|(_, n)| n.kind == crate::stub::Kind::ToolCall);
    calls.find_map(|(_, n)| match &n.body {
        Body::ToolCall {
            correlation_id: Some(c),
            input,
            ..
        } if c == correlation_id => Some(input),
        _ => None,
    })
}

/// Tell a session's clients about the results `ToolRuntime::answer_after_cancel`
/// wrote, as a turn tells them of its own (`announce_end`).
pub(crate) fn announce_cancelled(rec: &crate::fact::Rec<'_>, session_id: &str, nodes: &[Node]) {
    for node in nodes {
        rec.record(&fact::tool::ToolEnded {
            session_id,
            turn_id: node.turn_id.as_deref().unwrap_or_default(),
            node,
        });
        rec.record(&fact::turn::NodeWritten { session_id, node });
    }
}

/// Why a cancelled execution ended, as its calls' results say it: "the
/// execution was cancelled by operator".
fn cancel_why(e: &Execution) -> String {
    format!(
        "the execution was {}",
        e.ended_reason.as_deref().unwrap_or("cancelled")
    )
}

/// `text` with a capital first letter, for the start of a sentence.
fn sentence(text: &str) -> String {
    let mut chars = text.chars();
    chars
        .next()
        .map(|c| c.to_uppercase().chain(chars).collect())
        .unwrap_or_default()
}

/// The results of the tool calls a cancel ended before they ran
/// (theseus-w98), as the session's next turn would have written them: a
/// node for each call in `not_run` whose tool-call node the session holds and
/// no result answers yet. A cancelled execution takes no more turns, so the
/// cancel writes them, in its own frame. A provider call and the budget
/// question have no tool-call node, and get none.
pub(crate) fn not_run_results(
    store: &Store,
    session_id: &str,
    not_run: &[Action],
) -> Result<Vec<theseus_store::NewRecord>> {
    let calls: Vec<&Action> = not_run
        .iter()
        .filter(|a| a.tool != PROVIDER_TOOL && a.tool != BUDGET_TOOL)
        .collect();
    if calls.is_empty() {
        return Ok(vec![]);
    }
    let nodes = store.session_nodes(session_id)?;
    let answered: HashSet<&str> = nodes
        .iter()
        .filter_map(|(_, n)| match &n.body {
            Body::ToolResult { tool_use_id, .. } => Some(tool_use_id.as_str()),
            _ => None,
        })
        .collect();
    let mut out = Vec::new();
    for a in calls {
        let call = nodes.iter().find_map(|(_, n)| match &n.body {
            Body::ToolCall {
                tool_use_id,
                tool,
                correlation_id: Some(c),
                ..
            } if *c == a.correlation_id => Some((n, tool_use_id, tool)),
            _ => None,
        });
        let Some((call, tool_use_id, tool)) = call else {
            continue;
        };
        if answered.contains(tool_use_id.as_str()) {
            continue;
        }
        let (status, text) = not_run_answer(a);
        let mut meta = Value::Null;
        crate::cancel::stopped_meta(&mut meta, status, a);
        let node = Node::tool_result(
            session_id,
            call.turn_id.as_deref(),
            call.loop_index,
            Body::ToolResult {
                tool_use_id: tool_use_id.clone(),
                tool: tool.clone(),
                status,
                is_error: true,
                bytes_total: text.len() as u64,
                content: text,
                correlation_id: Some(a.correlation_id.clone()),
                truncated: false,
                full_ref: None,
                duration_ms: None,
                late: false,
                meta,
                image: None,
                external: None,
            },
        );
        out.push(node.record()?);
    }
    Ok(out)
}
