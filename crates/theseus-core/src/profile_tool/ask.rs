//! `profile.ask`'s call: one question to a named profile's model, with no tools, as a priced
//! provider call of the asking turn's (the way compaction's summary call is), and the
//! profile's words back as the result.
//!
//! [`begin`] runs in the turn's own task, where the turn's guard is: it resolves the profile,
//! asks the data floor, prices the call and plans it in the kernel (the spend limit, the day
//! ceiling and a `/stop` are the kernel's). [`finish`] makes the call in the tool's own task
//! and settles it, so a failure settles as failed and an answer settles at its price. The
//! reservation's owner ([`crate::owed_call::Owed`]) settles a call a stop or a deadline drops: at
//! zero before its send, as unknown (held) after it.

use std::sync::Arc;
use std::time::Duration;

use serde::Deserialize;
use serde_json::{json, Value};
use theseus_kernel::{
    micros_to_usd, Action, Completion, KernelError, Outcome as ActionOutcome, Proposal, RetryClass,
    PROVIDER_TOOL,
};
use theseus_protocol::SubagentAnswer;
use theseus_tools::parse;

use crate::fact::{self, ops::ProfileAsked};
use crate::owed_call::Owed;
use crate::provider::{ModelResponse, Provider, ProviderError, ProviderRequest};
use crate::toolrun::TurnCtx;
use crate::turn::Target;
use crate::Core;

/// The most a question may be, in characters.
pub const QUESTION_MAX_CHARS: usize = 8_000;
/// The most an answer may be, in tokens: a model that thinks before it speaks spends some of it
/// thinking, so this is more than a short answer needs.
pub const ANSWER_MAX_TOKENS: u32 = 4_000;
/// The longest the call may take.
pub const TIME_LIMIT: Duration = Duration::from_secs(100);
/// What the call is called in the books: the action's `purpose` and the call's `op`.
const PURPOSE: &str = "ask_profile";

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Input {
    profile: String,
    question: String,
}

/// The profile and the question, checked.
pub(super) fn input_of(input: &Value) -> Result<(String, String), String> {
    let i: Input = parse(input)?;
    let (profile, question) = (i.profile.trim(), i.question.trim());
    if profile.is_empty() || question.is_empty() {
        return Err("name a profile and give a question".into());
    }
    if question.chars().count() > QUESTION_MAX_CHARS {
        return Err(format!(
            "the question is over {QUESTION_MAX_CHARS} characters: ask one shorter question"
        ));
    }
    Ok((profile.to_string(), question.to_string()))
}

/// Why a model with no price in the catalog is not asked, for `profile.ask` and `profile.run` alike.
pub(crate) fn unpriced(model: &str) -> String {
    format!("{model} has no price in the catalog, so it is not asked")
}

/// What a planned call carries to its making.
pub(super) struct Planned {
    action: Action,
    request: ProviderRequest,
    provider: Arc<dyn Provider>,
    target: Target,
    reserve: theseus_kernel::Micros,
    session: String,
    turn: String,
    loop_index: Option<u32>,
    /// The profile of the turn that asked.
    asked_by: String,
    /// The reservation's owner: what settles it, however the call ends.
    owed: Owed,
}

/// Resolve `profile`, ask the data floor and the books, and plan the call; or the words of why not.
pub(super) fn begin(
    core: &Core,
    tc: &TurnCtx<'_>,
    profile: &str,
    question: &str,
) -> Result<Planned, String> {
    let runner = &core.runner;
    let gone = (!core.cfg.all_profiles().contains_key(profile))
        .then(|| crate::config::renamed::renamed_profile(profile))
        .flatten();
    if let Some(why) = gone.or_else(|| runner.profile_refusal(profile)) {
        return Err(why);
    }
    let live = core.live_profile().0;
    let target = runner
        .resolve_target(&live, Some(profile), None, None)
        .map_err(|e| format!("{e:#}"))?;
    let provider = runner
        .providers
        .get(&target.provider)
        .cloned()
        .ok_or_else(|| {
            format!(
                "{profile}'s provider {:?} is not configured",
                target.provider
            )
        })?;
    let price = runner
        .catalog
        .get(&target.model)
        .cloned()
        .ok_or_else(|| unpriced(&target.model))?;
    let max_tokens = target.max_tokens.min(ANSWER_MAX_TOKENS);
    let request = request_for(core, &target, question, max_tokens);
    let est = crate::compiler::estimate(&request, price.bytes_per_token, None);
    let reserve = price.reserve_micros(max_tokens, est.upper);
    let proposal = Proposal {
        tool: PROVIDER_TOOL.into(),
        args: json!({"provider": target.provider, "model": target.model, "max_tokens": max_tokens,
            "loop": tc.loop_index, "turn_id": tc.turn_id, "purpose": PURPOSE,
            "digest": request.digest()}),
        resource: Some(target.provider.clone()),
        policy_context: json!({"profile": target.profile}),
    };
    let action = tc
        .kernel
        .plan_and_dispatch(
            tc.guard,
            &proposal,
            RetryClass::SafeToRepeat,
            Some(TIME_LIMIT.as_millis() as u64),
            reserve,
            |_| Ok(Vec::new()),
        )
        .map_err(|e| not_planned(core, tc.session_id, &e))?;
    let owed = Owed::new(
        core.kernel.clone(),
        &action,
        format!("provider:{}", target.provider),
        json!({"purpose": PURPOSE}),
    );
    Ok(Planned {
        action,
        request,
        provider,
        target,
        reserve,
        session: tc.session_id.to_string(),
        turn: tc.turn_id.to_string(),
        loop_index: tc.loop_index,
        asked_by: tc.target.map(|t| t.profile.clone()).unwrap_or_default(),
        owed,
    })
}

/// The request: the question alone, as the profile's own system text (when it has one) frames
/// it, with no tool offered and the answer capped.
fn request_for(core: &Core, target: &Target, question: &str, max_tokens: u32) -> ProviderRequest {
    let system = target
        .system
        .iter()
        .map(|s| json!({"type": "text", "text": s}))
        .collect();
    ProviderRequest {
        model: target.model.clone(),
        max_tokens,
        system,
        messages: vec![json!({"role": "user", "content": [{"type": "text", "text": question}]})],
        tools: Vec::new(),
        thinking: core.runner.catalog.thinking_off(&target.model),
        output_config: None,
        cache_control: None,
        betas: Vec::new(),
        extra: Default::default(),
        image_tokens: 0,
    }
}

/// Why the kernel would not plan the call, in words for the model.
fn not_planned(core: &Core, session: &str, e: &anyhow::Error) -> String {
    match e.downcast_ref::<KernelError>() {
        Some(KernelError::OverBudget { needed, .. }) => format!(
            "the call would need {}, past what the spend limit leaves",
            crate::narrative::dollars(*needed)
        ),
        Some(KernelError::Stopped { by, .. }) => format!("stopped by {by}"),
        Some(KernelError::DayCeiling(r)) => {
            core.runner
                .day_refused(r, "asking a profile", Some(session));
            r.to_string()
        }
        _ => format!("the call was not planned: {e:#}"),
    }
}

/// Make the call and settle it: the words of the profile, or why there are none.
pub(super) async fn finish(core: Arc<Core>, p: Planned) -> Result<(String, Value), String> {
    let started_ms = theseus_protocol::now_unix_ms();
    let call = crate::exits::Call {
        op: Some(PURPOSE),
        session: Some(p.session.clone()),
        turn: Some(p.turn.clone()),
        loop_index: p.loop_index,
        correlation_id: Some(p.action.correlation_id.clone()),
    };
    let mut quiet = |_: crate::provider::Delta<'_>| {};
    // From here a drop (a stop, the runtime's deadline) may cut a call the provider has begun.
    p.owed.sending();
    let made = crate::exits::for_call(call, || p.provider.stream_message(&p.request, &mut quiet));
    let answered = match tokio::time::timeout(TIME_LIMIT, made).await {
        Ok(r) => r,
        Err(_) => Err(anyhow::anyhow!(
            "{} did not answer within {} seconds",
            p.target.profile,
            TIME_LIMIT.as_secs()
        )),
    };
    match answered {
        Ok(resp) => Ok(settled(&core, &p, resp, started_ms)),
        Err(e) => Err(failed(&core, &p, &e, started_ms)),
    }
}

/// The answer, settled at its price: the result and its meta.
fn settled(core: &Core, p: &Planned, resp: ModelResponse, started_ms: u64) -> (String, Value) {
    let cost = core
        .runner
        .priced(&resp, &p.target.model, p.reserve, started_ms);
    let cut = crate::provider::hit_cap(resp.stop_reason.as_deref());
    let fact = ProfileAsked {
        profile: &p.target.profile,
        provider: &p.target.provider,
        model: &resp.model,
        correlation_id: &p.action.correlation_id,
        outcome: if cut { "cut" } else { "answered" },
        class: None,
        input_tokens: resp.usage.input_tokens,
        output_tokens: resp.usage.output_tokens,
        cost_usd: micros_to_usd(cost),
    };
    let done = settle(
        core,
        p,
        &fact,
        Completion {
            correlation_id: p.action.correlation_id.clone(),
            outcome: ActionOutcome::Succeeded,
            result_ref: None,
            external_op_id: resp.request_id.clone(),
            started_at_ms: started_ms,
            finished_at_ms: theseus_protocol::now_unix_ms(),
            producer: format!("provider:{}", p.target.provider),
            signature: None,
            cost_micros: Some(cost),
            detail: Some(
                json!({"served_model": resp.model, "message_id": resp.message_id,
            "purpose": PURPOSE}),
            ),
        },
    );
    if let Err(e) = done {
        tracing::warn!(correlation_id = %p.action.correlation_id, error = %format!("{e:#}"),
            "an asked profile's call was not settled; the reconcile settles it at its deadline");
    }
    // The answer is a message of the conversation (issue 28): its words, who gave them, and the
    // call's own cost and time ride the result node, and the turn posts it before its reply.
    let text = resp.text.trim();
    let answer = SubagentAnswer {
        profile: p.target.profile.clone(),
        provider: p.target.provider.clone(),
        model: resp.model.clone(),
        asked_by: p.asked_by.clone(),
        text: text.to_string(),
        cost_usd: Some(micros_to_usd(cost)),
        elapsed_ms: theseus_protocol::now_unix_ms().saturating_sub(started_ms),
        output_tokens: resp.usage.output_tokens,
        cut,
        call: p.action.correlation_id.clone(),
        outcome: String::new(),
    };
    let meta = match text.is_empty() {
        true => json!({}),
        false => {
            core.tools.profile.give(&p.session, &p.turn, answer.clone());
            json!({"subagent": answer})
        }
    };
    (words(p, &resp, cost), meta)
}

/// Settle the call, with its `profile.asked` row in the same frame, and say it.
fn settle(
    core: &Core,
    p: &Planned,
    fact: &ProfileAsked<'_>,
    completion: Completion,
) -> anyhow::Result<()> {
    let rec = fact::Rec {
        narrator: &core.narrator,
        session: Some(&p.session),
        turn: Some(&p.turn),
        to: fact::To::Everyone(&core.bus),
        store: &core.store,
    };
    let row = rec.row(fact)?;
    p.owed.settle(&completion, vec![row])?;
    rec.announce(fact);
    Ok(())
}

/// What the model reads: the profile's words, whose they are, and what they cost.
fn words(p: &Planned, resp: &ModelResponse, cost: theseus_kernel::Micros) -> String {
    let (profile, text) = (&p.target.profile, resp.text.trim());
    let mut out = format!(
        "{profile} (model {}, from {}) answered:\n\n",
        resp.model, p.target.provider
    );
    match text.is_empty() {
        true => out.push_str("[no words: it answered with nothing to show]"),
        false => out.push_str(text),
    }
    if crate::provider::hit_cap(resp.stop_reason.as_deref()) {
        out.push_str(&format!(
            "\n\n[{}]",
            crate::provider::cap_phrase(p.request.max_tokens)
        ));
    }
    // Where the owner sees it is not the model's to say: it is shown as a message of its own, and
    // nothing of the kind is said when the profile had no words to show.
    let shown = match text.is_empty() {
        true => String::new(),
        false => format!(
            " Theseus shows it to the owner as {profile}'s own message, its model ({}) named, so \
             your reply need not repeat the words.",
            resp.model
        ),
    };
    out.push_str(&format!(
        "\n\n[That is {profile}'s answer, not yours.{shown} The call cost {}.]",
        crate::narrative::dollars(cost)
    ));
    out
}

/// A call that failed, settled as failed (or unknown, when the provider may have done the work);
/// the provider layer's own words.
fn failed(core: &Core, p: &Planned, e: &anyhow::Error, started_ms: u64) -> String {
    let pe = e.downcast_ref::<ProviderError>();
    let (class, unknown) = pe.map_or(("unknown", true), |p| (p.class(), p.usage_unknown()));
    let fact = ProfileAsked {
        profile: &p.target.profile,
        provider: &p.target.provider,
        model: &p.target.model,
        correlation_id: &p.action.correlation_id,
        outcome: "failed",
        class: Some(class),
        input_tokens: 0,
        output_tokens: 0,
        cost_usd: 0.0,
    };
    let settled = settle(
        core,
        p,
        &fact,
        Completion {
            correlation_id: p.action.correlation_id.clone(),
            outcome: if unknown {
                ActionOutcome::Unknown
            } else {
                ActionOutcome::Failed
            },
            result_ref: None,
            external_op_id: None,
            started_at_ms: started_ms,
            finished_at_ms: theseus_protocol::now_unix_ms(),
            producer: format!("provider:{}", p.target.provider),
            signature: None,
            cost_micros: if unknown { None } else { Some(0) },
            detail: Some(json!({"class": class, "purpose": PURPOSE})),
        },
    );
    if let Err(s) = settled {
        tracing::warn!(correlation_id = %p.action.correlation_id, error = %format!("{s:#}"),
            "an asked profile's failure was not settled; the reconcile settles it at its deadline");
    }
    format!("{} could not answer ({class}): {e}", p.target.profile)
}
