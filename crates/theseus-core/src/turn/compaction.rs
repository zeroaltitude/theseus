//! Compaction in the turn (M6 step 30c, §2.5; `compiler::compaction` holds
//! the compiler's half), and the core overage.
//!
//! - **Compaction.** Where a loop's compile rang (the estimate passed the
//!   window, or the provider said so), `[memory] summary_profile` summarizes
//!   the range the ring dropped instead: by default `session`, the turn's
//!   own profile, provider and model. The call goes the turn's own way to
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

pub(crate) mod gate;

/// The profile the `[model]` section builds: where a restricted background call falls back.
pub(crate) use super::data_step::DEFAULT_PROFILE;
use super::{Failure, Target, Turn, TurnRunner};
use crate::catalog::ThinkingMode;
use crate::compiler::{self, compaction, CompileInput, Compiled};
use crate::config::memory::SUMMARY_SESSION;
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

/// The least a node's cut may be when a long range is read shorter to fit
/// the summary model's window (theseus-3ki0).
const MIN_NODE_CHARS: usize = 400;

/// The summary call's instructions. Its clause on format, length and tone is
/// why "for now, be brief" outlives a compaction (theseus-slvl.4).
const INSTRUCTIONS: &str = "You summarize the earlier part of a conversation between an operator \
and Theseus, a coding and operations agent, so the agent can go on without it. Keep what the \
rest of the conversation may need: the operator's requests and decisions, what was found and \
done (files, commands, results, numbers, names), what failed and why, and what is still open. \
Keep any instruction the operator gave about format, length or tone, in their words. \
Leave out pleasantries and repetition. Write plain prose or short lists, in the past tense, \
under 600 words. Write only the summary.";

/// What the summary call is also told when its range is inside one long
/// task (theseus-3ki0): the range holds no message of the operator's, whose
/// instruction stays above the summary, pinned.
const IN_TURN: &str = "This part is inside one long task. The operator's instruction stays \
above your summary word for word, so don't restate it. Say the work in progress: what was \
tried, what worked, what failed and why, and what is left to do.";

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
        let Some(word) = self.memory.cfg().summary_profile().map(str::to_string) else {
            return Ok(ring);
        };
        // `session`, the default: the turn's own profile, provider and model,
        // so the range goes nowhere the session's turns don't. The row, the
        // node and its header name that profile.
        let own = (word == SUMMARY_SESSION).then(|| t.target.clone());
        let profile = own.as_ref().map_or(word, |o| o.profile.clone());
        let t0 = t.trace.now_us();
        // A restricted summary profile gets the range only if the session's
        // labels allow it (theseus-a1wvq.10, `gate.rs`).
        let route = match self.summary_route(t, (profile.clone(), own)).await {
            Ok(r) => r,
            Err(why) => {
                Self::rang(t, &profile, Fallback::new(why), t0, i);
                return Ok(ring);
            }
        };
        let profile = route.profile.clone();
        let attempt = self.summarize(t, &ring, base, route, i).await?;
        let (summary, done) = match attempt {
            Ok(s) => s,
            Err(f) => {
                Self::rang(t, &profile, f, t0, i);
                return Ok(ring);
            }
        };
        // The assembled prefix (30c): its recall section, the summary, the
        // kept turns, as of the summary's frame; without the section when
        // the section is what would not fit.
        let fresh = t.recall.pending.is_none();
        let section = self.recall_assembled(t).await;
        let drops = summary_drops(&ring, &done);
        let mut c =
            self.compaction_of(t, &ring, base, &summary, section.as_deref(), drops.clone())?;
        if !compaction::fits(&c) && section.is_some() {
            Self::recall_unassembled(t, fresh);
            c = self.compaction_of(t, &ring, base, &summary, None, drops)?;
        }
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
            Self::recall_unassembled(t, fresh);
            return Ok(ring);
        }
        Ok(c)
    }

    /// The compaction's compilation over the transcript as it stands, with
    /// the turn's pending recall, `section` its assembled recall section.
    fn compaction_of(
        &self,
        t: &Turn<'_>,
        ring: &Compiled,
        base: CompileInput<'_>,
        summary: &Node,
        section: Option<&str>,
        drops: Vec<BudgetDrop>,
    ) -> anyhow::Result<Compiled> {
        let nodes = t.tc.store.transcript(t.tc.session_id)?;
        let (nodes, sources) = self.recall_view(t, nodes, &base.spec.profile);
        let input = CompileInput {
            nodes: &nodes,
            last_position: self.store.last_position(),
            sources: &sources,
            assembled: section,
            ..base
        };
        Ok(compaction::compact(input, ring, &summary.id, drops))
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

    /// What the summary call would be: its range, its target (`own`, the
    /// turn's, when the profile is `session`), its provider and price, and
    /// its request; or why there is none, or why it would not fit.
    fn plan_summary<'n>(
        &self,
        ring: &Compiled,
        nodes: &'n [(u64, crate::stub::Stub)],
        profile: &str,
        own: Option<Target>,
    ) -> Result<Planned<'n>, Fallback> {
        let c = &ring.compilation;
        let range = dropped(nodes, &c.includes, c.as_of).map_err(Fallback::new)?;
        let messages = range.nodes.len() as u64 + range.folded.map_or(0, folded_count);
        let target = match own {
            Some(own) => own,
            None => self
                .resolve_target(profile, Some(profile), None, None)
                .map_err(|e| {
                    Fallback::new(format!(
                        "summary_profile {profile:?} does not resolve: {e:#}"
                    ))
                })?,
        };
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
        let thinking = thinking_for(&self.catalog, &target.model, price.thinking);
        let ask = |cut| request_for((&target.model, max_tokens), thinking.clone(), &range, cut);
        let mut request = ask(NODE_CHARS);
        let mut est = compiler::estimate(&request, price.bytes_per_token, None);
        let room = price
            .context_window
            .saturating_sub(u64::from(max_tokens))
            .saturating_sub(4_096);
        // A long turn's range can pass the window read at the usual cut
        // (theseus-3ki0: 284 messages came to 128,249 tokens against 93,904):
        // each node is cut shorter, in proportion, to a floor.
        let mut cut = NODE_CHARS;
        for _ in 0..4 {
            let Some(c) = shorter_cut(cut, est.upper, room) else {
                break;
            };
            (cut, request) = (c, ask(c));
            est = compiler::estimate(&request, price.bytes_per_token, None);
        }
        if est.upper > room {
            return Err(fail(format!(
                "the range comes to {} tokens, past what {}'s window leaves ({})",
                n(est.upper),
                target.model,
                n(room)
            )));
        }
        Ok(Planned {
            // On the estimate's upper bound (theseus-6fn.8): a summary's
            // request is all estimated, from its bytes, and a provider that
            // counts more than the estimate, writing the most it may, would
            // settle past a reservation on `tokens`, which has no margin.
            reserve: price.reserve_micros(max_tokens, est.upper),
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
                Some(self.cfg.model.timeouts.whole_call().as_millis() as u64),
                p.reserve,
                |_| Ok(Vec::new()),
            )
            .map_err(|e| match e.downcast_ref::<KernelError>() {
                Some(KernelError::OverBudget { needed, .. }) => format!(
                    "its call would need {}, past what the spend limit leaves",
                    crate::narrative::dollars(*needed)
                ),
                Some(KernelError::Stopped { by, .. }) => format!("stopped by {by}"),
                // The day ceiling (theseus-kp20): the summary is skipped,
                // and the ring runs; the loop's own call meets it next.
                Some(KernelError::DayCeiling(r)) => {
                    self.day_refused(r, "compaction", Some(t.tc.session_id));
                    r.to_string()
                }
                _ => format!("its call was not planned: {e:#}"),
            })
    }

    /// Summarize what `ring` dropped with `profile` (on `own`, the turn's
    /// target, for `session`): the `Summary` node, written with its call's
    /// settlement and row, and what the attempt knew; or why the ring runs
    /// instead.
    async fn summarize(
        &self,
        t: &mut Turn<'_>,
        ring: &Compiled,
        base: CompileInput<'_>,
        route: gate::Route,
        i: u32,
    ) -> anyhow::Result<Result<(Node, Done), Fallback>> {
        let profile = route.profile.as_str();
        let p = match self.plan_summary(ring, base.nodes, profile, route.own.clone()) {
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
        let (target, provider) = (&p.target, &p.provider);
        let request = &p.request;
        let started_ms = theseus_protocol::now_unix_ms();
        let started = Instant::now();
        let mut quiet = |_: crate::provider::Delta<'_>| {};
        // A summary of a session is that session's request too.
        let call = crate::exits::Call {
            op: Some("compaction"),
            session: Some(t.tc.session_id.to_string()),
            turn: Some(t.tc.turn_id.to_string()),
            loop_index: Some(i),
            correlation_id: Some(action.correlation_id.clone()),
        };
        let answered = crate::exits::for_call(call, || {
            crate::data_gate::for_session(t.tc.session_id, || {
                provider.stream_message(request, &mut quiet)
            })
        })
        .await;
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
            note: route.note,
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
        nodes: &[(u64, crate::stub::Stub)],
        profile: &str,
        call: Call,
        i: u32,
    ) -> anyhow::Result<Result<(Node, Done), Fallback>> {
        let Call {
            action,
            resp,
            started,
            started_ms,
            note,
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
        // A summary cut at its cap is a fragment of the range, never its
        // replacement (theseus-2c6iz): it settles like an answer with no text.
        let cut = crate::provider::hit_cap(resp.stop_reason.as_deref());
        let text = resp.text.trim();
        let (first, last) = span_of(range);
        let no_summary = match cut {
            true => format!(
                "the summary call was cut: {}",
                crate::provider::cap_phrase(p.request.max_tokens)
            ),
            false => "the summary call answered with no text".to_string(),
        };
        let summary = (!text.is_empty() && !cut).then(|| {
            let header = header_of(nodes, messages, (first, last), profile, &resp.model);
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
            why: match summary.is_none() {
                true => Some(no_summary.as_str()),
                false => note.as_deref(),
            },
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
        let completion = settlement(&action, &resp, started_ms, (&target.provider, cost));
        let completion = Completion {
            result_ref: summary.as_ref().map(|n| n.id.clone()),
            ..completion
        };
        t.tc.kernel.accept_completion_with(&completion, extra)?;
        t.announce_fact(&fact);
        let Some(summary) = summary else {
            return Ok(Err(Fallback {
                recorded: true,
                ..Fallback::new(no_summary)
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

/// The settlement of a summary call that answered, with no result yet.
fn settlement(
    action: &theseus_kernel::Action,
    resp: &crate::provider::ModelResponse,
    started_ms: u64,
    (provider, cost): (&str, u64),
) -> Completion {
    Completion {
        correlation_id: action.correlation_id.clone(),
        outcome: ActionOutcome::Succeeded,
        result_ref: None,
        external_op_id: resp.request_id.clone(),
        started_at_ms: started_ms,
        finished_at_ms: theseus_protocol::now_unix_ms(),
        producer: format!("provider:{provider}"),
        signature: None,
        cost_micros: Some(cost),
        detail: Some(
            json!({"served_model": resp.model, "message_id": resp.message_id,
            "purpose": "compaction"}),
        ),
    }
}

/// A summary call that answered.
struct Call {
    action: theseus_kernel::Action,
    resp: crate::provider::ModelResponse,
    started: Instant,
    started_ms: u64,
    /// Why the data gate moved the call, for its row.
    note: Option<String>,
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
    nodes: &'n [(u64, crate::stub::Stub)],
    includes: &[String],
    as_of: u64,
) -> Result<Range<'n>, String> {
    let kept: HashSet<&str> = includes.iter().map(String::as_str).collect();
    let floor = compaction::floor(nodes);
    // A message before the floor's range (a pinned instruction, 3ki0) the
    // ring left out is summarized with the rest.
    let hidden = compaction::hidden(nodes);
    let mut range = Range {
        nodes: Vec::new(),
        recalls: Vec::new(),
        folded: None,
    };
    for (p, n) in nodes {
        if *p > as_of
            || !compaction::outside(hidden, *p, n)
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

/// A summary's header: its count, the dates and positions of its range's
/// first and last node, its profile, and the model that wrote it.
fn header_of(
    nodes: &[(u64, crate::stub::Stub)],
    messages: u64,
    (first, last): (u64, u64),
    profile: &str,
    model: &str,
) -> String {
    let at = |p: u64| {
        nodes
            .iter()
            .find(|(q, _)| *q == p)
            .map_or(0, |(_, n)| n.created_at_ms)
    };
    let dates = (at(first), at(last));
    compaction::header(messages as u32, dates, (first, last), profile, model)
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
    let node = range.nodes.first().map(|(p, _)| *p);
    let first = match range.folded.map(|n| &n.body) {
        Some(Body::Summary { first, .. }) => node.map_or(*first, |p| p.min(*first)),
        _ => node.unwrap_or(0),
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

/// The `thinking` of a summary call (theseus-2c6iz): off where the model lets
/// it (`catalog::thinking_off`: an adaptive model thinks by default, and its
/// thinking would spend the summary's cap), and omitted-display adaptive for
/// a model that always thinks.
fn thinking_for(
    catalog: &crate::catalog::Catalog,
    model: &str,
    mode: ThinkingMode,
) -> Option<serde_json::Value> {
    match mode {
        ThinkingMode::Always => Some(json!({"type": "adaptive", "display": "omitted"})),
        _ => catalog.thinking_off(model),
    }
}

/// The summary call's request: the instructions, then the range as text,
/// a folded summary first. No tools, and `thinking` as [`thinking_for`] says.
fn request_for(
    (model, max_tokens): (&str, u32),
    thinking: Option<serde_json::Value>,
    range: &Range<'_>,
    node_chars: usize,
) -> ProviderRequest {
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
        if let Some(line) = line_of(n, node_chars) {
            text.push_str(&line);
            text.push_str("\n\n");
        }
    }
    let content = format!(
        "Summarize this part of the conversation:\n\n<conversation>\n{}</conversation>",
        text
    );
    let in_turn = !range
        .nodes
        .iter()
        .any(|(_, n)| matches!(n.body, Body::UserMessage { .. }));
    let system = match in_turn {
        true => format!("{INSTRUCTIONS} {IN_TURN}"),
        false => INSTRUCTIONS.to_string(),
    };
    ProviderRequest {
        model: model.to_string(),
        max_tokens,
        system: vec![json!({"type": "text", "text": system})],
        messages: vec![json!({"role": "user", "content": [{"type": "text", "text": content}]})],
        tools: Vec::new(),
        thinking,
        output_config: None,
        cache_control: None,
        betas: Vec::new(),
        extra: Default::default(),
        image_tokens: 0,
    }
}

/// The shorter cut a range read at `cut` needs to fit `room`, when its
/// estimate's bound `upper` passes it: in proportion, with a tenth to spare
/// (each node's header does not shrink, so a few rounds may be needed);
/// none under `MIN_NODE_CHARS`, where the ring runs instead.
fn shorter_cut(cut: usize, upper: u64, room: u64) -> Option<usize> {
    let next = (cut as u64 * room * 9 / 10 / upper.max(1)) as usize;
    (upper > room && next >= MIN_NODE_CHARS).then_some(next)
}

/// One node as the summary call reads it, cut at `cut_at` characters.
fn line_of(n: &Node, cut_at: usize) -> Option<String> {
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
    let body = if chars > cut_at {
        let cut: String = body.chars().take(cut_at).collect();
        format!("{cut}\n[… {} more characters left out]", chars - cut_at)
    } else {
        body
    };
    Some(format!("[{who}]\n{body}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A long turn's range (theseus-3ki0), read at the usual cut, passes
    /// the window; read at the shorter cut it fits, every node still in it.
    #[test]
    fn a_range_past_the_window_is_read_shorter_to_fit() {
        let nodes: Vec<Node> = (0..300)
            .map(|k| {
                let mut n = Node::user("ses_1", Some("turn_1"), "cli", "x");
                n.body = Body::ToolResult {
                    tool_use_id: format!("t{k}"),
                    tool: "fs.read".into(),
                    status: crate::node::ResultStatus::Ok,
                    is_error: false,
                    content: format!("log {k} ") + &"the tide was logged. ".repeat(250),
                    correlation_id: None,
                    bytes_total: 0,
                    truncated: false,
                    full_ref: None,
                    duration_ms: None,
                    late: false,
                    meta: json!({}),
                    image: None,
                    external: None,
                };
                n
            })
            .collect();
        let range = Range {
            nodes: nodes
                .iter()
                .enumerate()
                .map(|(i, n)| (i as u64 + 2, n))
                .collect(),
            recalls: vec![],
            folded: None,
        };
        let rates = crate::catalog::TokenRates::of("claude-sonnet-5-5");
        let at = |cut| {
            let q = request_for(
                ("claude-sonnet-5-5", 4096),
                Some(json!({"type": "between_tools"})),
                &range,
                cut,
            );
            (compiler::estimate(&q, rates, None).upper, q)
        };
        let room = 93_904;
        let (whole, mut q) = at(NODE_CHARS);
        assert!(whole > room, "{whole}");
        let (mut cut, mut upper, mut rounds) = (NODE_CHARS, whole, 0);
        while let Some(c) = shorter_cut(cut, upper, room) {
            (cut, rounds) = (c, rounds + 1);
            (upper, q) = at(c);
        }
        assert!(upper <= room, "{upper} at {cut}");
        assert!((1..=4).contains(&rounds), "{rounds} rounds");
        let text = q.messages[0]["content"][0]["text"].as_str().unwrap();
        assert!(text.contains("log 0 ") && text.contains("log 299 "));
        assert!(text.contains("more characters left out]"));
        // The work in progress, told: the range holds no operator message.
        assert!(q.system[0]["text"]
            .as_str()
            .unwrap()
            .contains("what is left to do"));
        assert_eq!(
            shorter_cut(NODE_CHARS, room, room),
            None,
            "it fits as it is"
        );
        assert_eq!(
            shorter_cut(NODE_CHARS, room * 100, room),
            None,
            "past the floor"
        );
    }

    /// The range a ring dropped: its messages, to summarize; a recall note
    /// in it, dropped; the floor's summary, folded; nothing kept, nothing
    /// after the compile, and nothing the floor already covers.
    #[test]
    fn a_recall_in_the_range_is_dropped_and_the_floor_is_folded() {
        let user =
            |t: &str| -> crate::stub::Stub { Node::user("ses_1", Some("turn_1"), "cli", t).into() };
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
        let nodes: Vec<(u64, crate::stub::Stub)> = vec![
            (1, user("a")),
            (2, user("b")),
            (3, user("c")),
            (4, floor.clone().into()),
            (5, recall.clone().into()),
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
