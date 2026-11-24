//! `task.ask` (issue 44, step 8; issue 46; design §7, "asking the task itself"): a conversation puts one
//! question to a task, and the task answers from its own point of view, when its records cannot say (its card
//! is read from them, `joined.rs`; "why did it drop those tests?" is not in them).
//!
//! - **The task's own view.** The request is the task's next one, as the census builds it
//!   (`TurnRunner::next_request`: the turn's one assembler and the view's compile), at the task's own place,
//!   with the question as its message, sent to the task's own model.
//! - **No tool.** The request's tools are dropped, and the calls and results its window holds are laid as
//!   text ([`toolless`]): a provider refuses calls in a request that defines no tool.
//! - **Through the turn's own books.** One provider call of the asking turn's, as `profile.ask` makes one:
//!   planned in the kernel with its worst case reserved (the spend limit, the day ceiling and a `/stop` are
//!   the kernel's), so a call past them is refused before anything is sent; made through the providers the
//!   turn's calls go through, which mask the vault's values at the exit (`exits`); settled at its price.
//!   The reservation has one owner that cannot be lost ([`crate::owed_call::Owed`]): a stop or a deadline
//!   that drops the call before its send settles it at zero, and one that drops it after settles it as
//!   unknown, its whole reservation held, as the kernel holds every unknown cost.
//! - **Outside text, always, once the task is found.** The answer is a model's testimony, never the owner's
//!   word: its result is marked External, as a fetched page is, so every act gate and the injection freeze
//!   read it as one (`judged::outside_admitted` keeps the card of a change made after it in the same
//!   exchange). So is every failure from then on (a provider's error, a timeout, a refusal of the call):
//!   it rides the failure's `external` meta, which the runtime reads as it reads a result's mark. Each says
//!   in the harness's own words whether an answer came, and lays the answer, or the provider's words, inside
//!   the same content-derived fence a later view draws (`view::text::labels::wrap_outside`). Only a refusal
//!   before the task is known (no such task, a malformed call, a task asking) is the harness's alone.
//! - **Nothing is written into the task**: no node in its session, no exchange at its place, no touch, no
//!   new version of its record. The call and its answer are the asking conversation's alone.
//! - **Bounded**: [`ASKS_PER_TURN`] asks a turn, each told plainly when used. A task does not ask a task.

use std::collections::HashMap;
use std::sync::{Arc, Mutex, OnceLock, Weak};
use std::time::Duration;

use serde::Deserialize;
use serde_json::{json, Value};
use theseus_kernel::{
    micros_to_usd, Action, Completion, KernelError, Outcome as ActionOutcome, Proposal, RetryClass,
    PROVIDER_TOOL,
};
use theseus_protocol::Plan;
use theseus_tools::{
    parse, AsyncMediaRun, Backend, Retry, Tool, ToolClass, ToolCtx, ToolFailure, ToolOutput,
};

use super::TaskRecord;
use crate::owed_call::Owed;
use crate::provider::{ModelResponse, Provider, ProviderError, ProviderRequest};
use crate::session::SessionRecord;
use crate::toolrun::TurnCtx;
use crate::turn::Target;
use crate::view::text::labels::wrap_outside;
use crate::Core;

pub const ASK: &str = "task.ask";
/// Its name, as the config's `[policy.tools]` checks it.
pub const NAMES: [&str; 1] = [ASK];
/// The asks a turn may make.
pub const ASKS_PER_TURN: u32 = 2;
/// The most a question may be, in characters.
pub const QUESTION_MAX_CHARS: usize = 4_000;
/// The most the answer may be, in tokens: as `profile.ask`'s, since a model that thinks before it speaks
/// spends some of it thinking.
const ANSWER_MAX_TOKENS: u32 = crate::profile_tool::ANSWER_MAX_TOKENS;
/// The most of the answer the result carries, in characters: about 1,500 tokens.
pub const ANSWER_MAX_CHARS: usize = 6_000;
/// The longest the call may take.
const TIME_LIMIT: Duration = Duration::from_secs(100);
/// What the call is called in the books: the action's `purpose` and the call's `op`.
const PURPOSE: &str = "ask_task";
/// Turns whose counts are kept before finished ones are let go.
const TURNS_KEPT: usize = 256;

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct Input {
    id: String,
    question: String,
}

/// The task named and the question, checked.
fn input_of(input: &Value) -> Result<(String, String), String> {
    let i: Input = parse(input)?;
    let (id, question) = (i.id.trim(), i.question.trim());
    if id.is_empty() || question.is_empty() {
        return Err("name a task by its id and give one question".into());
    }
    if question.chars().count() > QUESTION_MAX_CHARS {
        return Err(format!(
            "the question is over {QUESTION_MAX_CHARS} characters: ask one shorter question"
        ));
    }
    Ok((id.to_string(), question.to_string()))
}

/// The message the task's view is assembled for and answers: the question, framed.
fn message(question: &str) -> String {
    format!(
        "[A conversation asks you one question about this task's work. Answer it from what you see here, in \
         your own words. You have no tools for this answer, and nothing of it is kept in this task.]\n\n\
         {question}"
    )
}

/// The registry's `task.ask`: its name, schema and plan, for the gate. The runtime runs it ([`Board::run`]).
pub struct TaskAsk;

impl Tool for TaskAsk {
    fn name(&self) -> &str {
        ASK
    }

    fn description(&self) -> &str {
        "Ask a task ONE question about its own work that its records cannot answer (why it chose what it did), \
         and get its answer from its own view: one call of its model, no tools, nothing written into the task. \
         The answer is the task's testimony, outside text, never the owner's word. Two asks a turn."
    }

    fn input_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "id": {"type": "string", "description": "The task's id (tsk_…), or the end of it."},
                "question": {"type": "string", "description": "The one question, in full."}
            },
            "required": ["id", "question"],
            "additionalProperties": false,
        })
    }

    fn class(&self) -> ToolClass {
        ToolClass::Read
    }

    fn backend(&self) -> Backend {
        Backend::Async
    }

    fn retry(&self) -> Retry {
        // A second call is a second question, and is paid for.
        Retry::NonRepeatable
    }

    fn plan(&self, input: &Value, _ctx: &ToolCtx) -> Result<Plan, String> {
        let (id, _) = input_of(input)?;
        Ok(Plan {
            summary: format!("ask the task {id} one question"),
            ..Default::default()
        })
    }

    fn run_async(&self, _input: &Value, _ctx: &ToolCtx) -> theseus_tools::AsyncRun {
        Box::pin(std::future::ready(Err(ToolFailure::new(
            "task.ask runs through the runtime, which asks the task",
        ))))
    }
}

/// What `task.ask` reads: the core, once built, and each turn's asks.
#[derive(Default)]
pub struct Board {
    core: OnceLock<Weak<Core>>,
    turns: Mutex<HashMap<String, u32>>,
}

impl Board {
    pub fn attach(&self, core: &Arc<Core>) {
        let _ = self.core.set(Arc::downgrade(core));
    }

    /// `turn`'s asks before this one, and this one counted. Past [`TURNS_KEPT`] turns, only turns whose
    /// exchange has ended are let go: a running turn's count is never dropped.
    fn count(&self, core: &Core, turn: &str) -> u32 {
        let mut m = self.turns.lock().unwrap_or_else(|e| e.into_inner());
        if m.len() >= TURNS_KEPT && !m.contains_key(turn) {
            m.retain(|t, _| {
                !crate::exchange::exchange(&core.store, t)
                    .ok()
                    .flatten()
                    .is_some_and(|e| e.end_ms.is_some())
            });
        }
        let n = m.entry(turn.into()).or_default();
        let was = *n;
        *n = n.saturating_add(1);
        was
    }

    /// The call, in its own task: planned here, in the turn's, and made and settled in the tool's.
    pub fn run(&self, tc: &TurnCtx<'_>, input: &Value) -> AsyncMediaRun {
        let fail = |f: ToolFailure| -> AsyncMediaRun { Box::pin(std::future::ready(Err(f))) };
        let Some(core) = self.core.get().and_then(Weak::upgrade) else {
            return fail(ToolFailure::new(
                "task.ask is not ready: the core is not built",
            ));
        };
        let (task, question) =
            match input_of(input).and_then(|(id, q)| Ok((found(&core, tc, &id)?, q))) {
                Ok(found) => found,
                Err(why) => return fail(ToolFailure::new(why)),
            };
        match begin(self, &core, tc, task.clone(), &question) {
            Err(why) => fail(not_asked(&task, &why)),
            Ok(p) => Box::pin(async move {
                let external = theseus_tools::External {
                    url: source(&p.task),
                };
                let output = finish(core, p).await?;
                Ok((output, Some(external), None))
            }),
        }
    }
}

/// What the task's words are marked as coming from: the result's External url, and the fence's source.
fn source(t: &TaskRecord) -> String {
    format!("task:{}", t.id)
}

/// The start of every result once the task is found: which task, and which version of it.
fn head(t: &TaskRecord) -> String {
    format!("[Task {} \"{}\", v{}", t.id, t.title, t.version)
}

/// A failure once the task is found: outside text, as an answer is (`meta["external"]`, which the runtime
/// reads as a result's mark), with `meta`'s keys beside it.
fn outside_failure(t: &TaskRecord, message: String, mut meta: Value) -> ToolFailure {
    meta["external"] = json!(source(t));
    meta["task"] = json!(t.id);
    meta["version"] = json!(t.version);
    ToolFailure { message, meta }
}

/// The task found, and nothing sent: why not, in the harness's words.
fn not_asked(t: &TaskRecord, why: &str) -> ToolFailure {
    let message = format!(
        "{}: not asked; {why}]\n\n[Nothing was sent, and nothing was spent.]",
        head(t)
    );
    outside_failure(t, message, json!({}))
}

/// What a planned call carries to its making.
struct Planned {
    action: Action,
    request: ProviderRequest,
    provider: Arc<dyn Provider>,
    target: Target,
    reserve: theseus_kernel::Micros,
    task: TaskRecord,
    session: String,
    turn: String,
    loop_index: Option<u32>,
    /// The turn's asks with this one.
    asked: u32,
    /// The reservation's owner: what settles it, however the call ends.
    owed: Owed,
}

/// The task `id` names, for a turn that may ask one; or the words of why not.
fn found(core: &Core, tc: &TurnCtx<'_>, id: &str) -> Result<TaskRecord, String> {
    if tc.task.is_some() {
        return Err(
            "task.ask is not for a task's session: a task does not ask a task; report what is needed".into(),
        );
    }
    let tasks =
        super::all(&core.store).map_err(|e| format!("the tasks could not be read: {e:#}"))?;
    Ok(super::resolve(&tasks, id)?.clone())
}

/// Build the task's next request for the question, and plan the call; or the words of why not.
fn begin(
    board: &Board,
    core: &Core,
    tc: &TurnCtx<'_>,
    task: TaskRecord,
    question: &str,
) -> Result<Planned, String> {
    let Some(sid) = task.session.clone() else {
        return Err(format!(
            "{} has not started: it has no session to ask, and its card says all its records hold",
            task.id
        ));
    };
    let rec: SessionRecord = (core.store.get_session(&sid).ok().flatten())
        .filter(|r: &SessionRecord| r.task.is_some())
        .ok_or_else(|| format!("the session of {} could not be read", task.id))?;
    let asked = board.count(core, tc.turn_id) + 1;
    if asked > ASKS_PER_TURN {
        return Err(format!(
            "This turn's allowance of asks is used: {ASKS_PER_TURN} questions put to tasks. Do not ask \
             more this turn: tell the person what the answers said, and offer to ask again."
        ));
    }
    let live = core.live_profile().0;
    let next = (core.runner)
        .next_request(Some(&rec), "task", &live, &message(question))
        .map_err(|e| format!("the task's view could not be built: {e:#}"))?;
    let target = next.target;
    let runner = &core.runner;
    let provider = (runner.providers.get(&target.provider).cloned()).ok_or_else(|| {
        format!(
            "the task's provider {:?} is not configured",
            target.provider
        )
    })?;
    let price = (runner.catalog.get(&target.model).cloned()).ok_or_else(|| {
        format!(
            "{} has no price in the catalog, so it is not asked",
            target.model
        )
    })?;
    let mut request = toolless(next.compiled.request);
    request.max_tokens = target.max_tokens.min(ANSWER_MAX_TOKENS);
    request.thinking = runner.catalog.thinking_off(&target.model);
    request.output_config = None;
    let est = crate::compiler::estimate(&request, price.bytes_per_token, None);
    let reserve = price.reserve_micros(request.max_tokens, est.upper);
    let proposal = Proposal {
        tool: PROVIDER_TOOL.into(),
        args: json!({"provider": target.provider, "model": target.model, "max_tokens": request.max_tokens,
            "loop": tc.loop_index, "turn_id": tc.turn_id, "purpose": PURPOSE, "task": task.id,
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
        json!({"purpose": PURPOSE, "task": task.id, "version": task.version}),
    );
    Ok(Planned {
        action,
        request,
        provider,
        target,
        reserve,
        task,
        session: tc.session_id.to_string(),
        turn: tc.turn_id.to_string(),
        loop_index: tc.loop_index,
        asked,
        owed,
    })
}

/// `q` with no tool offered: its definitions dropped, and every call and result in its messages laid as
/// text, since a provider refuses calls and results in a request that defines no tool.
fn toolless(mut q: ProviderRequest) -> ProviderRequest {
    q.tools.clear();
    q.extra.remove("tool_choice");
    for m in &mut q.messages {
        let Some(blocks) = m["content"].as_array_mut() else {
            continue;
        };
        for b in blocks.iter_mut() {
            let Some(text) = as_text(b) else { continue };
            let mark = b.get("cache_control").cloned();
            *b = json!({"type": "text", "text": text});
            if let Some(c) = mark {
                b["cache_control"] = c;
            }
        }
    }
    q
}

/// A call's or a result's block, as a line of text; none for any other block.
fn as_text(b: &Value) -> Option<String> {
    let kind = b["type"].as_str()?;
    if kind == "tool_use" || kind == "server_tool_use" {
        let name = b["name"].as_str().unwrap_or("a tool");
        return Some(format!("[called {name} with {}]", b["input"]));
    }
    if kind == "tool_result" || kind.ends_with("_tool_result") {
        let said = match &b["content"] {
            Value::String(s) => s.clone(),
            Value::Array(parts) => (parts.iter())
                .map(|p| p["text"].as_str().unwrap_or("[not text]"))
                .collect::<Vec<_>>()
                .join("\n"),
            _ => String::new(),
        };
        return Some(format!("[its result: {said}]"));
    }
    None
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
            core.runner.day_refused(r, "asking a task", Some(session));
            r.to_string()
        }
        _ => format!("the call was not planned: {e:#}"),
    }
}

/// Make the call and settle it: the task's words, headed as its testimony, or why there are none.
async fn finish(core: Arc<Core>, p: Planned) -> Result<ToolOutput, ToolFailure> {
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
    let answered = tokio::time::timeout(TIME_LIMIT, made).await;
    match answered {
        Ok(Ok(resp)) => Ok(settled(&core, &p, &resp, started_ms)),
        Ok(Err(e)) => Err(failed(&p, Some(&e), started_ms)),
        Err(_) => Err(failed(&p, None, started_ms)),
    }
}

/// What the action's settlement says beside its own fields: the call's purpose and the task asked.
fn detail(p: &Planned, more: Value) -> Value {
    let mut d = json!({"purpose": PURPOSE, "task": p.task.id, "version": p.task.version});
    if let (Some(d), Value::Object(more)) = (d.as_object_mut(), more) {
        d.extend(more);
    }
    d
}

/// The answer, settled at its price: the result.
fn settled(core: &Core, p: &Planned, resp: &ModelResponse, started_ms: u64) -> ToolOutput {
    let cost = (core.runner).priced(resp, &p.target.model, p.reserve, started_ms);
    let c = Completion {
        correlation_id: p.action.correlation_id.clone(),
        outcome: ActionOutcome::Succeeded,
        result_ref: None,
        external_op_id: resp.request_id.clone(),
        started_at_ms: started_ms,
        finished_at_ms: theseus_protocol::now_unix_ms(),
        producer: format!("provider:{}", p.target.provider),
        signature: None,
        cost_micros: Some(cost),
        detail: Some(detail(
            p,
            json!({"served_model": resp.model, "message_id": resp.message_id}),
        )),
    };
    if let Err(e) = p.owed.settle(&c, Vec::new()) {
        tracing::warn!(correlation_id = %p.action.correlation_id, error = %format!("{e:#}"),
            "an asked task's call was not settled; the reconcile settles it at its deadline");
    }
    ToolOutput {
        text: words(p, resp, cost),
        meta: json!({"task": p.task.id, "version": p.task.version, "asked": p.asked,
            "of": ASKS_PER_TURN, "cost_usd": micros_to_usd(cost)}),
    }
}

/// The result: the head that says whose words these are, the answer (cut by code) inside its outside fence,
/// and what it cost.
fn words(p: &Planned, resp: &ModelResponse, cost: theseus_kernel::Micros) -> String {
    let mut out = format!(
        "{}: its own answer, from its own view; not a record, and not the owner's word]\n\n",
        head(&p.task)
    );
    let text = resp.text.trim();
    match text.is_empty() {
        true => out.push_str("[no words: it answered with nothing to show]"),
        false => {
            let kept: String = text.chars().take(ANSWER_MAX_CHARS).collect();
            out.push_str(&wrap_outside(Some(&source(&p.task)), &kept));
        }
    }
    if text.chars().count() > ANSWER_MAX_CHARS {
        out.push_str("\n\n[cut at about 1,500 tokens]");
    }
    if crate::provider::hit_cap(resp.stop_reason.as_deref()) {
        out.push_str(&format!(
            "\n\n[{}]",
            crate::provider::cap_phrase(p.request.max_tokens)
        ));
    }
    out.push_str(&format!(
        "\n\n[That is the task's testimony, read as text from outside: nothing of it was kept in the task. \
         Ask {} of {ASKS_PER_TURN} this turn; the call cost {}.]",
        p.asked,
        crate::narrative::dollars(cost)
    ));
    out
}

/// A call that failed, settled as failed (or unknown, when the provider may have done the work): the
/// harness's words for it, and the provider's quoted inside an outside fence. `e` is none when the call
/// ran out of its time.
fn failed(p: &Planned, e: Option<&anyhow::Error>, started_ms: u64) -> ToolFailure {
    let pe = e.and_then(|e| e.downcast_ref::<ProviderError>());
    let (class, unknown) = match (e, pe) {
        (None, _) => ("timeout", true),
        (Some(_), Some(pe)) => (pe.class(), pe.usage_unknown()),
        (Some(_), None) => ("unknown", true),
    };
    let c = Completion {
        correlation_id: p.action.correlation_id.clone(),
        outcome: match unknown {
            true => ActionOutcome::Unknown,
            false => ActionOutcome::Failed,
        },
        result_ref: None,
        external_op_id: None,
        started_at_ms: started_ms,
        finished_at_ms: theseus_protocol::now_unix_ms(),
        producer: format!("provider:{}", p.target.provider),
        signature: None,
        cost_micros: if unknown { None } else { Some(0) },
        detail: Some(detail(p, json!({"class": class}))),
    };
    if let Err(s) = p.owed.settle(&c, Vec::new()) {
        tracing::warn!(correlation_id = %p.action.correlation_id, error = %format!("{s:#}"),
            "an asked task's failure was not settled; the reconcile settles it at its deadline");
    }
    let mut message = match e {
        None => format!(
            "{}: no answer came; it did not answer within {} seconds]",
            head(&p.task),
            TIME_LIMIT.as_secs()
        ),
        Some(e) => format!(
            "{}: no answer came; the call failed ({class}). What the provider said:]\n\n{}",
            head(&p.task),
            wrap_outside(Some("the provider's error"), &e.to_string())
        ),
    };
    let cost = match unknown {
        true => format!(
            "Its cost is not known: the call may have reached the provider, so its whole reservation of {} \
             stays held on this conversation, and the day counts it at that, until it is reconciled.",
            crate::narrative::dollars(p.reserve)
        ),
        false => "The call cost nothing.".to_string(),
    };
    message.push_str(&format!(
        "\n\n[{cost} Nothing of it was kept in the task. Ask {} of {ASKS_PER_TURN} this turn.]",
        p.asked
    ));
    let meta =
        json!({"asked": p.asked, "of": ASKS_PER_TURN, "class": class, "cost_known": !unknown});
    outside_failure(&p.task, message, meta)
}

#[cfg(test)]
#[path = "tests_ask.rs"]
mod tests;

#[cfg(test)]
#[path = "tests_ask_owed.rs"]
mod tests_owed;
