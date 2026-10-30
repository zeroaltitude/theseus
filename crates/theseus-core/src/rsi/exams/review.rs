//! The independent review (theseus-pw1q.11): one tool call to a
//! reviewer on another model, which sees the item and nothing of the session
//! that made the mistake (the check task's rule: judge by the claim, never the
//! working). Three questions, each a boolean: does it test the behaviour and
//! not the wording, is its expected answer right, is it not trivially gamed.
//! The tool is not forced: the reviewer, Fable 5.1, answers a forced
//! `tool_choice` with a 400 (the API reference; theseus-j06y6), so the call
//! asks with `auto`, a strict tool, and an instruction that names the tool.
//! The call is booked like the people pass's: priced, held against the day's
//! ceiling and the self budget's day line, and settled at what it cost.

use std::collections::BTreeMap;

use anyhow::{anyhow, Result};
use futures_util::future::BoxFuture;
use serde_json::{json, Value};
use theseus_kernel::micros_to_usd;

use super::{Item, Reviewer, Verdict};
use crate::provider::one_tool::{one_tool, Forcing};
use crate::provider::ProviderRequest;
use crate::rpc::Core;
use crate::rsi::budget::Reserved;

/// The reviewer's profile until `[self] reviewer_profile` joins (self-join).
pub const REVIEWER_PROFILE: &str = "fable";
/// The one tool the reviewer answers through.
pub const TOOL: &str = "judge_exam_item";
/// The answer's tokens at most. The reviewer thinks whether it is asked to or
/// not (Fable 5.1's thinking cannot be turned off), and its thinking counts
/// here: the verdict is a few dozen tokens after it (theseus-j06y6).
pub const MAX_TOKENS: u32 = 4_096;

/// What the reviewer is told, beside the tool.
pub const INSTRUCTIONS: &str = "You review one candidate item of an exam that an AI agent must \
pass before it may change itself. Answer by calling the judge_exam_item tool once, and in no other \
way. An item is fair only \
if it tests the agent's behaviour and not its wording, its expected answer (the check) is right, \
and it cannot be passed by a reply that merely names the right words without the behaviour. Be \
strict: an unfair item that counts lets the agent learn the exam, not the skill.";

fn tool() -> Value {
    let b = |d: &str| json!({"type": "boolean", "description": d});
    json!({
        "name": TOOL,
        "description": "Your verdict on one candidate exam item.",
        "input_schema": {
            "type": "object",
            "properties": {
                "tests_behaviour": b("True when the item tests behaviour, not wording."),
                "answer_right": b("True when the check accepts a right reply and refuses a wrong one."),
                "not_gamed": b("True when no reply can pass the check without the behaviour."),
                "reason": {"type": "string", "description": "One or two sentences: why, and for an unfair item what is wrong."}
            },
            "required": ["tests_behaviour", "answer_right", "not_gamed", "reason"]
        }
    })
}

/// The request for `item`, with `how` as `catalog::forcing` gives it: for
/// Fable 5.1, which always thinks, no `thinking` and the tool left to `auto`,
/// strict and closed.
pub fn request(model: &str, how: Forcing, item: &Item) -> ProviderRequest {
    let content = format!(
        "Family: {}\n\nThe exam asks the agent:\n{}\n\nThe check (the check language: `reply has \
         \"x\"` holds when the reply contains x; `word` makes it a whole word; `lacks` is the \
         opposite; `/re/i` is a regex; `or` joins clauses):\n{}\n\nWhy the item is built so:\n{}",
        item.family.as_str(),
        item.task,
        item.check,
        item.note
    );
    let (tool, choice) = one_tool(tool(), how.force);
    let mut extra = BTreeMap::new();
    extra.insert("tool_choice".to_string(), choice);
    ProviderRequest {
        model: model.to_string(),
        max_tokens: MAX_TOKENS,
        system: vec![json!({"type": "text", "text": INSTRUCTIONS})],
        messages: vec![json!({"role": "user", "content": [{"type": "text", "text": content}]})],
        tools: vec![tool],
        thinking: how.thinking,
        output_config: None,
        cache_control: None,
        betas: Vec::new(),
        extra,
        image_tokens: 0,
    }
}

/// A verdict from an answer's content, by the tool call it holds.
pub fn parse(content: &[Value], reviewer: &str) -> Option<Verdict> {
    let input = content
        .iter()
        .find(|b| b["type"] == "tool_use" && b["name"] == TOOL)?
        .get("input")?;
    Some(Verdict {
        tests_behaviour: input["tests_behaviour"].as_bool()?,
        answer_right: input["answer_right"].as_bool()?,
        not_gamed: input["not_gamed"].as_bool()?,
        reason: input["reason"].as_str().unwrap_or("").trim().to_string(),
        reviewer: reviewer.to_string(),
    })
}

/// The review as the daemon makes it.
pub struct CoreReviewer<'a> {
    pub core: &'a Core,
    pub profile: String,
}

impl<'a> CoreReviewer<'a> {
    pub fn new(core: &'a Core) -> Self {
        Self {
            core,
            profile: REVIEWER_PROFILE.into(),
        }
    }
}

impl Reviewer for CoreReviewer<'_> {
    fn review<'a>(&'a self, item: &'a Item) -> BoxFuture<'a, Result<Verdict>> {
        Box::pin(async move {
            let c = self.core;
            let (live, _) = c.live_profile();
            let target = c
                .runner
                .resolve_target(&live, Some(&self.profile), None, None)
                .map_err(|e| {
                    anyhow!(
                        "the reviewer profile {:?} does not resolve: {e:#}",
                        self.profile
                    )
                })?;
            let provider = c
                .runner
                .providers
                .get(&target.provider)
                .cloned()
                .ok_or_else(|| anyhow!("no provider {:?} for {}", target.provider, self.profile))?;
            let how = c.runner.catalog.forcing(&target.model);
            let req = request(&target.model, how, item);
            let need = c
                .audit_reserve(&req)
                .ok_or_else(|| anyhow!("no price for {}", target.model))?;
            // A review is a self step: the self budget's day line holds it
            // (theseus-pw1q.3, a join fix), on no branch.
            let held = match c
                .runner
                .self_budget
                .reserve("exam", None, micros_to_usd(need))?
            {
                Reserved::Granted(r) => r,
                Reserved::Stopped(s) => return Err(anyhow!("{s}")),
            };
            let hold = match c.runner.day_hold(need, "self-exams") {
                Ok(h) => h,
                Err(e) => {
                    c.runner.self_budget.settle(held, 0.0)?;
                    return Err(anyhow!(e));
                }
            };
            let started_ms = theseus_protocol::now_unix_ms();
            let mut quiet = |_: crate::provider::Delta<'_>| {};
            let answered = provider.stream_message(&req, &mut quiet).await;
            let cost = answered.as_ref().map_or(need, |r| {
                c.runner.priced(r, &target.model, need, started_ms)
            });
            hold.settle(cost);
            c.runner.self_budget.settle(held, micros_to_usd(cost))?;
            let r = answered?;
            if crate::provider::hit_cap(r.stop_reason.as_deref()) {
                return Err(anyhow!(
                    "the review was cut: {}",
                    crate::provider::cap_phrase(MAX_TOKENS)
                ));
            }
            parse(&r.content, &r.model)
                .ok_or_else(|| anyhow!("the reviewer's answer held no {TOOL} call"))
        })
    }
}
