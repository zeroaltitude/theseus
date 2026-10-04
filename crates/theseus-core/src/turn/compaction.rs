//! Compaction in the turn (M6 step 30c, §2.5; `compiler::compaction` holds
//! the compiler's half), and the core overage.
//!
//! - **Compaction.** Where a loop's compile rang (the estimate passed the
//!   window, or the provider said so), `[memory] summary_profile` summarizes
//!   the range the ring dropped instead. The call goes the turn's own way to
//!   a provider: a kernel action, planned with its reservation and dispatched
//!   in one frame, and settled at its real cost in the frame that writes the
//!   `Summary` node and the `context.compacted` row. The compilation is then
//!   the summary and the ring's kept turns (`strategy: compaction`). A
//!   `Recall` node in the range is dropped, never summarized; a summary
//!   already there is folded into the new one, whose range starts where its
//!   did. When the profile is `off` nothing here runs; when the call cannot
//!   run, fails, or its summary would not fit, the ring runs as before and
//!   the row says why.
//! - **The core overage** (`context_overage`): when even the newest exchange
//!   alone passes the window by the estimate, the turn fails before any
//!   call, with the window, the estimate, and the overage, and nothing
//!   retries it (`session::Failing`): the same request would not fit again.

use std::collections::HashSet;
use std::sync::Arc;
use std::time::Instant;

use serde_json::json;
use theseus_kernel::{
    micros_to_usd, Completion, KernelError, Outcome as ActionOutcome, Proposal, RetryClass,
    PROVIDER_TOOL,
};
use theseus_protocol::memory::{BudgetDrop, BudgetRange};

use super::{Failure, Target, Turn, TurnRunner};
use crate::catalog::ThinkingMode;
use crate::compiler::{self, compaction, CompileInput, Compiled};
use crate::fact::compaction::{overage_words, Compacted, Overage};
use crate::node::{Body, Node};
use crate::provider::{ProviderError, ProviderRequest};

/// The failure class of a turn whose newest exchange alone does not fit the
/// model's window, by the estimate, before anything is sent (30c).
pub const OVERAGE_CLASS: &str = "context_overage";

/// The most a summary may be, in output tokens: its profile's cap, at most
/// this.
pub const SUMMARY_MAX_TOKENS: u32 = 4096;

/// The most characters of one node the summary call reads: a long tool
/// result is cut, and says how much it left out.
const NODE_CHARS: usize = 6_000;

/// The summary call's instructions.
const INSTRUCTIONS: &str = "You summarize the earlier part of a conversation between an operator \
and Theseus, a coding and operations agent, so the agent can go on without it. Keep what the \
rest of the conversation may need: the operator's requests and decisions, what was found and \
done (files, commands, results, numbers, names), what failed and why, and what is still open. \
Leave out pleasantries and repetition. Write plain prose or short lists, in the past tense, \
under 600 words. Write only the summary.";

/// What the ring dropped, as a compaction reads it.
struct Range<'n> {
    /// The messages to summarize, with their positions.
    nodes: Vec<(u64, &'n Node)>,
    /// The recall notes in the range: dropped, never summarized.
    recalls: Vec<&'n Node>,
    /// The summary there was, folded into this one.
    folded: Option<&'n Node>,
}

/// Why the ring runs instead, and what the attempt knew by then.
struct Fallback {
    why: String,
    model: Option<String>,
    messages: u64,
    /// Its row is written already (the call answered with no text).
    recorded: bool,
}

impl Fallback {
    fn new(why: impl Into<String>) -> Self {
        Fallback {
            why: why.into(),
            model: None,
            messages: 0,
            recorded: false,
        }
    }
}

impl TurnRunner {
    /// After a loop's compile: a new ring becomes a compaction when the
    /// summary profile can write one that fits. Anything else, and the ring
    /// stands, with a row saying why. Only a fault of the store is an error.
    pub(super) async fn compact(
        &self,
        t: &mut Turn<'_>,
        ring: Compiled,
        base: CompileInput<'_>,
        i: u32,
    ) -> anyhow::Result<Compiled> {
        // A ring that kept only the newest exchange and still does not fit
        // is the core overage: no summary would help.
        if !ring.new_compilation
            || ring.compilation.strategy != "ring"
            || ring.budget.overage.is_some()
        {
            return Ok(ring);
        }
        let Some(profile) = self.memory.cfg().summary_profile().map(str::to_string) else {
            return Ok(ring);
        };
        let t0 = t.trace.now_us();
        let attempt = self.summarize(t, &ring, base, &profile, i).await?;
        let (summary, done) = match attempt {
            Ok(s) => s,
            Err(f) => {
                Self::rang(t, &profile, f, t0, i);
                return Ok(ring);
            }
        };
        // The compaction's compilation, as of the summary's frame.
        let nodes = t.tc.store.transcript(t.tc.session_id)?;
        let (nodes, sources) = self.recall_view(t, nodes);
        let input = CompileInput {
            nodes: &nodes,
            last_position: self.store.last_position(),
            sources: &sources,
            ..base
        };
        let c = compaction::compact(input, &ring, &summary.id, summary_drops(&ring, &done));
        if !compaction::fits(&c) {
            let why = format!(
                "the summary and the kept turns came to {} tokens, past the {} the window leaves",
                crate::narrative::thousands(c.estimate.upper),
                crate::narrative::thousands(c.budget.limit_tokens)
            );
            Self::rang(
                t,
                &profile,
                Fallback {
                    why,
                    model: Some(done.model),
                    messages: done.messages,
                    recorded: false,
                },
                t0,
                i,
            );
            return Ok(ring);
        }
        Ok(c)
    }

    /// The ring stands: its row, span, and line say why.
    fn rang(t: &mut Turn<'_>, profile: &str, f: Fallback, t0: u64, i: u32) {
        if f.recorded {
            return;
        }
        let fact = Compacted {
            outcome: "ring",
            why: Some(&f.why),
            profile,
            model: f.model.as_deref(),
            messages: f.messages,
            first: None,
            last: None,
            folded: None,
            node_id: None,
            summary_tokens: 0,
            input_tokens: 0,
            cost_usd: None,
            settled_micros: None,
            reserved_micros: None,
            correlation_id: None,
            recalls_dropped: 0,
            loop_index: i,
            t0,
            t1: t.trace.now_us(),
        };
        t.record(&fact);
    }

    /// What the summary call would be: its range, its target, its provider
    /// and price, and its request; or why there is none, or why it would
    /// not fit.
    fn plan_summary<'n>(
        &self,
        ring: &Compiled,
        nodes: &'n [(u64, Arc<Node>)],
        profile: &str,
    ) -> Result<Planned<'n>, Fallback> {
        let c = &ring.compilation;
        let range = dropped(nodes, &c.includes, c.as_of).map_err(Fallback::new)?;
        let messages = range.nodes.len() as u64 + range.folded.map_or(0, folded_count);
        let target = self
            .resolve_target(profile, Some(profile), None, None)
            .map_err(|e| {
                Fallback::new(format!(
                    "summary_profile {profile:?} does not resolve: {e:#}"
                ))
            })?;
        let fail = |why: String| Fallback {
            why,
            model: Some(target.model.clone()),
            messages,
            recorded: false,
        };
        let Some(provider) = self.providers.get(&target.provider).cloned() else {
            return Err(fail(format!(
                "{profile}'s provider {:?} is not configured",
                target.provider
            )));
        };
        let Some(price) = self.catalog.get(&target.model).cloned() else {
            return Err(fail(format!(
                "{} has no price in the catalog",
                target.model
            )));
        };
        let max_tokens = target.max_tokens.min(SUMMARY_MAX_TOKENS);
        let n = crate::narrative::thousands;
        // Would a summary of the most it may be fit beside the kept turns?
        let limit = ring.budget.limit_tokens;
        if limit > 0 && ring.estimate.upper + u64::from(max_tokens) > limit {
            return Err(fail(format!(
                "a summary of up to {} tokens would not fit beside the {} the kept turns take, \
                 within the {} the window leaves",
                n(u64::from(max_tokens)),
                n(ring.estimate.upper),
                n(limit)
            )));
        }
        let request = request_for(&target, price.thinking, &range);
        let est = compiler::estimate(&request, price.bytes_per_token, None);
        let room = price
            .context_window
            .saturating_sub(u64::from(max_tokens))
            .saturating_sub(4_096);
        if est.upper > room {
            return Err(fail(format!(
                "the range comes to {} tokens, past what {}'s window leaves ({})",
                n(est.upper),
                target.model,
                n(room)
            )));
        }
        Ok(Planned {
            reserve: price.reserve_micros(max_tokens, est.tokens),
            range,
            messages,
            target,
            provider,
            price,
            request,
            max_tokens,
        })
    }

    /// Plan and dispatch the summary call, with its reservation, in one
    /// frame; or why it was not.
    fn dispatch_summary(
        &self,
        t: &Turn<'_>,
        p: &Planned<'_>,
        profile: &str,
        i: u32,
    ) -> Result<theseus_kernel::Action, String> {
        let target = &p.target;
        let proposal = Proposal {
            tool: PROVIDER_TOOL.into(),
            args: json!({"provider": target.provider, "model": target.model, "max_tokens": p.max_tokens,
                "loop": i, "turn_id": t.tc.turn_id, "purpose": "compaction", "digest": p.request.digest()}),
            resource: Some(target.provider.clone()),
            policy_context: json!({"profile": profile}),
        };
        t.tc.kernel
            .plan_and_dispatch(
                t.tc.guard,
                &proposal,
                RetryClass::SafeToRepeat,
                Some(self.cfg.model.timeouts.total_secs * 1000),
                p.reserve,
                |_| Ok(Vec::new()),
            )
            .map_err(|e| match e.downcast_ref::<KernelError>() {
                Some(KernelError::OverBudget { needed, .. }) => format!(
                    "its call would need {}, past what the spend limit leaves",
                    crate::narrative::dollars(*needed)
                ),
                Some(KernelError::Stopped { by, .. }) => format!("stopped by {by}"),
                _ => format!("its call was not planned: {e:#}"),
            })
    }

    /// Summarize what `ring` dropped with `profile`: the `Summary` node,
    /// written with its call's settlement and row, and what the attempt
    /// knew; or why the ring runs instead.
    async fn summarize(
        &self,
        t: &mut Turn<'_>,
        ring: &Compiled,
        base: CompileInput<'_>,
        profile: &str,
        i: u32,
    ) -> anyhow::Result<Result<(Node, Done), Fallback>> {
        let p = match self.plan_summary(ring, base.nodes, profile) {
            Ok(p) => p,
            Err(f) => return Ok(Err(f)),
        };
        let fail = |why: String| {
            Ok(Err(Fallback {
                why,
                model: Some(p.target.model.clone()),
                messages: p.messages,
                recorded: false,
            }))
        };
        let action = match self.dispatch_summary(t, &p, profile, i) {
            Ok(a) => a,
            Err(why) => return fail(why),
        };
        let (target, provider, request) = (&p.target, &p.provider, &p.request);
        let started_ms = theseus_protocol::now_unix_ms();
        let started = Instant::now();
        let mut quiet = |_: crate::provider::Delta<'_>| {};
        let answered = provider.stream_message(request, &mut quiet).await;
        let resp = match answered {
            Ok(r) => r,
            Err(e) => {
                let why = settle_failed(t, &action, &target.provider, started_ms, &e);
                return fail(format!(
                    "the summary call failed after {} ms ({why}): {e}",
                    started.elapsed().as_millis()
                ));
            }
        };
        let call = Call {
            action,
            resp,
            started,
            started_ms,
        };
        self.settle_summary(t, &p, ring, base.nodes, profile, call, i)
    }

    /// The summary call answered: its cost joins the turn's books, and its
    /// `Summary` node and row ride its settlement. An answer with no text
    /// settles all the same, and the ring runs.
    #[allow(clippy::too_many_arguments)]
    fn settle_summary(
        &self,
        t: &mut Turn<'_>,
        p: &Planned<'_>,
        ring: &Compiled,
        nodes: &[(u64, Arc<Node>)],
        profile: &str,
        call: Call,
        i: u32,
    ) -> anyhow::Result<Result<(Node, Done), Fallback>> {
        let Call {
            action,
            resp,
            started,
            started_ms,
        } = call;
        let (range, messages, target, price) = (&p.range, p.messages, &p.target, &p.price);
        let cost_usd = self
            .catalog
            .cost_usd(&resp.model, &resp.usage)
            .or_else(|| Some(price.cost_usd(&resp.usage)));
        let cost = self.catalog.get(&resp.model).map_or_else(
            || price.cost_micros(&resp.usage),
            |e| e.cost_micros(&resp.usage),
        );
        // What it cost joins the turn's books, written or not (R1).
        t.cost = t.cost.map(|c| c + micros_to_usd(cost));
        let text = resp.text.trim();
        let (first, last) = span_of(range);
        let summary = (!text.is_empty()).then(|| {
            let header = header_of(nodes, messages, first, last, profile);
            Node::summary(
                t.tc.session_id,
                t.tc.turn_id,
                Body::Summary {
                    first,
                    last,
                    nodes: messages as u32,
                    text: text.to_string(),
                    profile: profile.to_string(),
                    model: resp.model.clone(),
                    cost_usd,
                    header,
                },
            )
        });
        let fact = Compacted {
            outcome: if summary.is_some() {
                "compaction"
            } else {
                "ring"
            },
            why: summary
                .is_none()
                .then_some("the summary call answered with no text"),
            profile,
            model: Some(&resp.model),
            messages,
            first: Some(first),
            last: Some(last),
            folded: range.folded.map(|n| n.id.as_str()),
            node_id: summary.as_ref().map(|n| n.id.as_str()),
            summary_tokens: resp.usage.output_tokens,
            input_tokens: resp.usage.input_tokens,
            cost_usd,
            settled_micros: Some(cost),
            reserved_micros: Some(action.reserved_micros),
            correlation_id: Some(&action.correlation_id),
            recalls_dropped: range.recalls.len() as u64,
            loop_index: i,
            t0: t.trace.at(started),
            t1: t.trace.now_us(),
        };
        // The summary and its row ride the call's settlement.
        let mut extra = Vec::new();
        if let Some(n) = &summary {
            extra.push(n.record()?);
        }
        extra.push(t.tc.rec().row(&fact)?);
        let completion = Completion {
            correlation_id: action.correlation_id.clone(),
            outcome: ActionOutcome::Succeeded,
            result_ref: summary.as_ref().map(|n| n.id.clone()),
            external_op_id: resp.request_id.clone(),
            started_at_ms: started_ms,
            finished_at_ms: theseus_protocol::now_unix_ms(),
            producer: format!("provider:{}", target.provider),
            signature: None,
            cost_micros: Some(cost),
            detail: Some(
                json!({"served_model": resp.model, "message_id": resp.message_id,
                "purpose": "compaction"}),
            ),
        };
        t.tc.kernel.accept_completion_with(&completion, extra)?;
        t.announce_fact(&fact);
        let Some(summary) = summary else {
            return Ok(Err(Fallback {
                recorded: true,
                ..Fallback::new("the summary call answered with no text")
            }));
        };
        t.tc.node_written(&summary);
        let done = Done {
            model: resp.model.clone(),
            messages,
            range: range_drop(range, ring),
            recalls: range.recalls.iter().map(|n| recall_drop(n)).collect(),
        };
        Ok(Ok((summary, done)))
    }

    /// The core overage: the compile kept only the newest exchange (or had
    /// nothing to drop), and its estimate still passes the window. The turn
    /// fails before any call.
    pub(super) fn overage(t: &mut Turn<'_>, compiled: &Compiled, i: u32) -> Option<Failure> {
        compiled.budget.overage.as_ref()?;
        let model = t.target.model.clone();
        let window = compiled.compilation.manifest.context_window;
        let o = Overage {
            model: &model,
            window,
            estimate: compiled.estimate.tokens,
            upper: compiled.estimate.upper,
            budget: &compiled.budget,
            loop_index: i,
        };
        let message = overage_words(&o);
        t.record(&o);
        Some(Failure {
            class: OVERAGE_CLASS.into(),
            transient: false,
            usage_unknown: false,
            reason: format!("{OVERAGE_CLASS}: {message}"),
            source: anyhow::anyhow!("{message} Send something smaller, or start a new session."),
        })
    }
}

/// A summary call that answered.
struct Call {
    action: theseus_kernel::Action,
    resp: crate::provider::ModelResponse,
    started: Instant,
    started_ms: u64,
}

/// A summary call, planned.
struct Planned<'n> {
    range: Range<'n>,
    messages: u64,
    target: Target,
    provider: Arc<dyn crate::provider::Provider>,
    price: crate::catalog::CatalogEntry,
    request: ProviderRequest,
    max_tokens: u32,
    reserve: theseus_kernel::Micros,
}

/// Settle a summary call that failed: as failed, or as unknown when the
/// provider may have done the work. Returns its class.
fn settle_failed(
    t: &Turn<'_>,
    action: &theseus_kernel::Action,
    provider: &str,
    started_ms: u64,
    e: &anyhow::Error,
) -> &'static str {
    let pe = e.downcast_ref::<ProviderError>();
    let (class, unknown) = pe
        .map(|p| (p.class(), p.usage_unknown()))
        .unwrap_or(("unknown", true));
    let settled = t.tc.kernel.accept_completion(&Completion {
        correlation_id: action.correlation_id.clone(),
        outcome: if unknown {
            ActionOutcome::Unknown
        } else {
            ActionOutcome::Failed
        },
        result_ref: None,
        external_op_id: None,
        started_at_ms: started_ms,
        finished_at_ms: theseus_protocol::now_unix_ms(),
        producer: format!("provider:{provider}"),
        signature: None,
        cost_micros: if unknown { None } else { Some(0) },
        detail: Some(json!({"class": class, "purpose": "compaction"})),
    });
    if let Err(s) = settled {
        tracing::warn!(correlation_id = %action.correlation_id, error = %format!("{s:#}"),
            "a summary call's failure was not settled; the reconcile settles it at its deadline");
    }
    class
}

/// What a written summary's compaction carries into its report.
struct Done {
    model: String,
    messages: u64,
    range: BudgetDrop,
    recalls: Vec<BudgetDrop>,
}

/// What `ring` dropped, past the session's latest summary: the messages to
/// summarize, the recall notes, and the summary to fold in.
fn dropped<'n>(
    nodes: &'n [(u64, Arc<Node>)],
    includes: &[String],
    as_of: u64,
) -> Result<Range<'n>, String> {
    let kept: HashSet<&str> = includes.iter().map(String::as_str).collect();
    let floor = compaction::floor(nodes);
    let after = floor.map_or(0, |(_, last)| last);
    let mut range = Range {
        nodes: Vec::new(),
        recalls: Vec::new(),
        folded: None,
    };
    for (p, n) in nodes {
        if *p > as_of
            || (floor.is_some() && *p <= after)
            || kept.contains(n.id.as_str())
            || !compiler::renderable(n)
        {
            continue;
        }
        match &n.body {
            Body::Recall { .. } => range.recalls.push(n),
            Body::Summary { .. } => {}
            _ => range.nodes.push((*p, n)),
        }
    }
    if range.nodes.is_empty() {
        return Err("the ring dropped nothing new to summarize".into());
    }
    range.folded = floor.map(|(e, _)| &*e.1);
    Ok(range)
}

/// A summary's header: its count, and the dates of its range's first and
/// last node.
fn header_of(
    nodes: &[(u64, Arc<Node>)],
    messages: u64,
    first: u64,
    last: u64,
    profile: &str,
) -> String {
    let at = |p: u64| {
        nodes
            .iter()
            .find(|(q, _)| *q == p)
            .map_or(0, |(_, n)| n.created_at_ms)
    };
    compaction::header(messages as u32, at(first), at(last), profile)
}

/// How many messages a summary stands for.
fn folded_count(n: &Node) -> u64 {
    match &n.body {
        Body::Summary { nodes, .. } => u64::from(*nodes),
        _ => 0,
    }
}

/// The positions of a range's first and last node: a folded summary's
/// first, when there is one.
fn span_of(range: &Range<'_>) -> (u64, u64) {
    let first = match range.folded.map(|n| &n.body) {
        Some(Body::Summary { first, .. }) => *first,
        _ => range.nodes.first().map_or(0, |(p, _)| *p),
    };
    (first, range.nodes.last().map_or(first, |(p, _)| *p))
}

/// The range, in the report: summarized, tier `compaction`.
fn range_drop(range: &Range<'_>, ring: &Compiled) -> BudgetDrop {
    let tokens = ring
        .budget
        .dropped
        .iter()
        .filter(|d| d.tier == "ring")
        .map(|d| d.tokens)
        .sum();
    let first = range.folded.or(range.nodes.first().map(|(_, n)| *n));
    BudgetDrop {
        node_id: None,
        range: first
            .zip(range.nodes.last())
            .map(|(a, (_, b))| BudgetRange {
                first: a.id.clone(),
                last: b.id.clone(),
                nodes: range.nodes.len() as u64 + u64::from(range.folded.is_some()),
            }),
        reason: "summarized".into(),
        tokens,
        tier: compaction::STRATEGY.into(),
    }
}

/// A recall note in the range: dropped, never summarized (§2.4: testimony
/// that can be rebuilt).
fn recall_drop(n: &Node) -> BudgetDrop {
    let tokens = match &n.body {
        Body::Recall { items, .. } => items.iter().map(|r| r.tokens).sum(),
        _ => 0,
    };
    BudgetDrop {
        node_id: Some(n.id.clone()),
        range: None,
        reason: "recall_note".into(),
        tokens,
        tier: compaction::STRATEGY.into(),
    }
}

/// The compaction's drops: the ring's report's own but its cut, then the
/// range summarized and the recall notes dropped.
fn summary_drops(ring: &Compiled, done: &Done) -> Vec<BudgetDrop> {
    let mut out: Vec<BudgetDrop> = ring
        .budget
        .dropped
        .iter()
        .filter(|d| d.tier != "ring")
        .cloned()
        .collect();
    out.push(done.range.clone());
    out.extend(done.recalls.iter().cloned());
    out
}

/// The summary call's request: the instructions, then the range as text,
/// a folded summary first. No tools, and no thinking unless the model
/// cannot turn it off.
fn request_for(target: &Target, thinking: ThinkingMode, range: &Range<'_>) -> ProviderRequest {
    let mut text = String::new();
    if let Some(Body::Summary {
        header, text: t, ..
    }) = range.folded.map(|n| &n.body)
    {
        text.push_str(&format!(
            "The earlier summary:\n{header}\n{t}\n\nWhat came after it:\n"
        ));
    }
    for (_, n) in &range.nodes {
        if let Some(line) = line_of(n) {
            text.push_str(&line);
            text.push_str("\n\n");
        }
    }
    let content = format!(
        "Summarize this part of the conversation:\n\n<conversation>\n{}</conversation>",
        text
    );
    ProviderRequest {
        model: target.model.clone(),
        max_tokens: target.max_tokens.min(SUMMARY_MAX_TOKENS),
        system: vec![json!({"type": "text", "text": INSTRUCTIONS})],
        messages: vec![json!({"role": "user", "content": [{"type": "text", "text": content}]})],
        tools: Vec::new(),
        thinking: (thinking == ThinkingMode::Always)
            .then(|| json!({"type": "adaptive", "display": "omitted"})),
        output_config: None,
        cache_control: None,
        betas: Vec::new(),
        extra: Default::default(),
        image_tokens: 0,
    }
}

/// One node as the summary call reads it, cut at `NODE_CHARS`.
fn line_of(n: &Node) -> Option<String> {
    let (who, body) = match &n.body {
        Body::UserMessage { text, attachments } => (
            n.author.clone().unwrap_or_else(|| "operator".into()),
            crate::attach::display_text(text, attachments, n.author.as_deref()),
        ),
        Body::AssistantMessage { blocks, .. } => {
            let mut parts = vec![crate::provider::text_of(blocks)];
            for u in crate::provider::tool_uses_in(blocks) {
                parts.push(format!("(calls {} {})", u.name, u.input));
            }
            let s = parts
                .into_iter()
                .filter(|p| !p.is_empty())
                .collect::<Vec<_>>();
            ("theseus".into(), s.join("\n"))
        }
        Body::ToolResult {
            tool,
            status,
            content,
            ..
        } => (
            format!("{tool} result, {}", status.as_str()),
            content.clone(),
        ),
        _ => return None,
    };
    let chars = body.chars().count();
    let body = if chars > NODE_CHARS {
        let cut: String = body.chars().take(NODE_CHARS).collect();
        format!("{cut}\n[… {} more characters left out]", chars - NODE_CHARS)
    } else {
        body
    };
    Some(format!("[{who}]\n{body}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The range a ring dropped: its messages, to summarize; a recall note
    /// in it, dropped; the floor's summary, folded; nothing kept, nothing
    /// after the compile, and nothing the floor already covers.
    #[test]
    fn a_recall_in_the_range_is_dropped_and_the_floor_is_folded() {
        let user = |t: &str| Arc::new(Node::user("ses_1", Some("turn_1"), "cli", t));
        let floor = Arc::new(Node::summary(
            "ses_1",
            "turn_1",
            Body::Summary {
                first: 1,
                last: 2,
                nodes: 2,
                text: "the gist".into(),
                profile: "glm".into(),
                model: "glm-5.3-flash".into(),
                cost_usd: None,
                header: String::new(),
            },
        ));
        let recall = Arc::new(Node::recall("ses_1", "turn_2", "rcl_1", "baseline", vec![]));
        let nodes: Vec<(u64, Arc<Node>)> = vec![
            (1, user("a")),
            (2, user("b")),
            (3, user("c")),
            (4, floor.clone()),
            (5, recall.clone()),
            (6, user("d")),
            (7, user("e")),
            (9, user("pending")),
        ];
        let kept = vec![nodes[6].1.id.clone()];
        let r = dropped(&nodes, &kept, 8).unwrap();
        let positions: Vec<u64> = r.nodes.iter().map(|(p, _)| *p).collect();
        assert_eq!(positions, vec![3, 6]);
        assert_eq!(r.recalls.len(), 1);
        assert_eq!(r.recalls[0].id, recall.id);
        assert_eq!(r.folded.map(|n| n.id.as_str()), Some(floor.id.as_str()));
        assert_eq!(span_of(&r), (1, 6));
        let d = recall_drop(r.recalls[0]);
        assert_eq!(
            (d.tier.as_str(), d.reason.as_str()),
            ("compaction", "recall_note")
        );
        // Nothing new: the ring kept every message after the floor.
        let all: Vec<String> = nodes[5..].iter().map(|(_, n)| n.id.clone()).collect();
        let mut keep = all;
        keep.push(nodes[2].1.id.clone());
        assert!(dropped(&nodes, &keep, 8).is_err());
    }
}
