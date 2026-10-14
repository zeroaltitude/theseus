//! A response's calls through the gate and run (`run_calls`, theseus-a60),
//! and the batch a decline ends (theseus-6i0): once one call of a batch is
//! declined, its later calls that would wait are not asked. Split from
//! `toolrun.rs`.

use std::sync::Arc;
use std::time::Instant;

use anyhow::Result;
use serde_json::json;
use theseus_tools::{Tool, ToolClass};

use super::{
    parallel, Admitted, Batch, Call, CallOutcome, Gated, Ran, ResultNode, ToolRuntime, TurnCtx,
};
use crate::fact;
use crate::node::ResultStatus;
use crate::provider::ToolUse;

/// Why a call of a declined batch is not asked: the model reads it, and the
/// operator is spared a card for a plan the model may change.
/// Why a call started after an earlier one of its response: that call's
/// index, and what both name; one per call of a group.
type Held = Vec<Option<(usize, String)>>;

/// A group's calls: index in the response, the tool, and its gate's verdict.
type Group = Vec<(usize, Arc<dyn Tool>, Gated)>;

pub(super) const NOT_ASKED: &str = "an earlier call in this batch was declined";

impl ToolRuntime {
    /// A response's `tool_use`s (theseus-a60): gated in order, then run, the
    /// `Read` calls of a group together.
    /// - An unknown tool, invalid JSON, or invalid input is answered at once,
    ///   wherever it is.
    /// - The first call whose posture is approve ends the gating: it asks
    ///   after every call before it has finished, and the calls after it wait,
    ///   ungated, for the continuation.
    /// - The rest run in groups, each group's calls at once, as futures in
    ///   the caller's task (theseus-d1hi, `parallel`'s rule): consecutive
    ///   reads; consecutive writes to different paths; consecutive programs
    ///   that share no terminal, MCP server, or file name, at most
    ///   `[tools] parallel_runs` at a time. A class change ends a group, so a
    ///   program after a write starts once the write has finished, and a read
    ///   after a program reads its effect.
    ///
    /// Every kernel call stays in the caller's task, one at a time: the
    /// kernel rewrites an execution's record from what it read.
    pub async fn run_calls(
        &self,
        tc: &TurnCtx<'_>,
        assistant_node: &str,
        calls: &[Call<'_>],
    ) -> Result<Batch> {
        self.run_batch(tc, assistant_node, calls, false).await
    }

    /// `run_calls`, for the rest of a batch an earlier call of which was
    /// `declined` (theseus-6i0): then a call whose posture is approve is not
    /// asked but answered not run (`not_asked`), and the gating goes on, so
    /// the calls that need no answer run as they would have.
    pub(super) async fn run_batch(
        &self,
        tc: &TurnCtx<'_>,
        assistant_node: &str,
        calls: &[Call<'_>],
        declined: bool,
    ) -> Result<Batch> {
        let mut ran = Vec::new();
        let mut runnable = Vec::new();
        let mut ask = None;
        for (i, c) in calls.iter().enumerate() {
            let started = Instant::now();
            let answered = match self.admit(tc, assistant_node, c)? {
                Admitted::Answered(outcome) => outcome,
                Admitted::Asks(_, g) if declined => {
                    self.not_asked(tc, assistant_node, c.call, g)?
                }
                Admitted::Asks(tool, g) => {
                    ask = Some((i, tool, g));
                    break;
                }
                Admitted::Runs(tool, g) => {
                    runnable.push((i, tool, g));
                    continue;
                }
            };
            ran.push(Ran {
                index: i,
                outcome: answered,
                started,
                ended: Instant::now(),
                group: ran.len(),
                judged: Vec::new(),
            });
        }
        let mut group = Vec::new();
        let mut next = ran.len();
        let mut open = parallel::Open::default();
        let mut held = Held::new();
        for (i, tool, g) in runnable {
            let touch = parallel::Touch::of(tool.as_ref(), &calls[i].call.input, &g.plan);
            let verdict = open.admit(i, &touch);
            if !verdict.joins {
                let (run, note) = (std::mem::take(&mut group), std::mem::take(&mut held));
                self.run_group(tc, assistant_node, calls, (run, note), &mut next, &mut ran)
                    .await?;
            }
            group.push((i, tool, g));
            held.push(verdict.held);
        }
        self.run_group(
            tc,
            assistant_node,
            calls,
            (group, held),
            &mut next,
            &mut ran,
        )
        .await?;
        let mut awaiting = None;
        if let Some((i, tool, g)) = ask {
            let started = Instant::now();
            let (outcome, judged) = self
                .start(tc, assistant_node, calls[i].call, tool, g)
                .await?;
            if let CallOutcome::AwaitingConfirm { correlation_id } = &outcome {
                awaiting = Some(correlation_id.clone());
            }
            ran.push(Ran {
                index: i,
                outcome,
                started,
                ended: Instant::now(),
                group: next,
                judged,
            });
        }
        ran.sort_by_key(|r| r.index);
        Ok(Batch { ran, awaiting })
    }

    /// A group of calls, each started at once in the caller's task, at most
    /// `[tools] parallel_reads`, `parallel_writes` or `parallel_runs` (by
    /// the group's class) at a time: the rest wait for a slot, and every
    /// result is kept in call order. Every call finishes before the first
    /// error is returned, so none is left between its dispatch and its
    /// completion. `held` is, call by call, what it started after
    /// (theseus-0hufe), which its result says.
    async fn run_group(
        &self,
        tc: &TurnCtx<'_>,
        assistant_node: &str,
        calls: &[Call<'_>],
        (group, held): (Group, Held),
        next: &mut usize,
        ran: &mut Vec<Ran>,
    ) -> Result<()> {
        if group.is_empty() {
            return Ok(());
        }
        let id = *next;
        *next += 1;
        // A group of writes or programs (theseus-d1hi): each result says how
        // many ran beside it.
        let (class, n) = (
            group[0].2.plan.class.unwrap_or(group[0].1.class()),
            group.len(),
        );
        let beside = if class == ToolClass::Read { 0 } else { n - 1 };
        let cap = match class {
            ToolClass::Read => self.parallel_reads,
            ToolClass::Write => self.parallel_writes,
            ToolClass::Run => self.parallel_runs,
        };
        let slots = &tokio::sync::Semaphore::new(cap.max(1));
        let runs = group.into_iter().zip(held).map(|((i, tool, g), held)| {
            let note = held.map(|(by, on)| parallel::held_note(by, &on));
            async move {
                let _slot = slots.acquire().await;
                let started = Instant::now();
                let call = self.start(tc, assistant_node, calls[i].call, tool, g);
                let r = parallel::BESIDE.scope(beside, call);
                let r = match note {
                    Some(note) => parallel::HELD.scope(note, r).await,
                    None => r.await,
                };
                (i, started, Instant::now(), r)
            }
        });
        let mut failed = None;
        for (index, started, ended, r) in futures_util::future::join_all(runs).await {
            match r {
                Ok((outcome, judged)) => ran.push(Ran {
                    index,
                    outcome,
                    started,
                    ended,
                    group: id,
                    judged,
                }),
                Err(e) => {
                    failed.get_or_insert(e);
                }
            }
        }
        failed.map_or(Ok(()), Err)
    }

    /// A call that would wait, in a batch an earlier call of which was
    /// declined: never planned, so no action waits and no card is posted. Its
    /// `ToolCall` node keeps the gate's record (no correlation id, as an
    /// invalid input's), and its result says why it did not run, as
    /// `Cancelled`: `Declined` would say the operator declined it, and nobody
    /// was asked.
    fn not_asked(
        &self,
        tc: &TurnCtx<'_>,
        assistant_node: &str,
        call: &ToolUse,
        g: Gated,
    ) -> Result<CallOutcome> {
        let (_, tool) = self.tool_of(call);
        let call_node = Self::tool_call_node(tc, assistant_node, call, &tool, None, g.record);
        let node = self.result_node(
            tc,
            ResultNode {
                meta: json!({"not_run": NOT_ASKED}),
                ..ResultNode::new(
                    &call.id,
                    &tool,
                    ResultStatus::Cancelled,
                    format!("Not run: {NOT_ASKED}."),
                )
            },
        );
        tc.store.append(&[call_node.record()?, node.record()?])?;
        tc.record(&fact::tool::CallNotAsked { tool: &tool });
        Self::announce_end(tc, &node);
        Ok(CallOutcome::Done {
            status: ResultStatus::Cancelled,
        })
    }
}
