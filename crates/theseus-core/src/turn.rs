//! The turn runner (spec §3.3, §3.3a, §4.4a, §4.6).
//!
//! A turn is the loops run under one acquisition of the session's turn lock,
//! ended by the Advancer. A loop: the compiler renders the session (a
//! compilation plus its append tail) into a request, the provider call runs as
//! a kernel action, the model's response becomes an assistant node in the same
//! frame as the call's settlement, and every `tool_use` goes through the tool
//! runtime (gate, confirm, action, result node). The Advancer continues while
//! the model is calling tools and every call has an answer.
//!
//! A turn with no input is a **continuation** (the harness's driver runs it):
//! results that settled since the last turn become late result nodes, pending
//! tool calls are resumed (a confirm answered, a restart survived), and the
//! model runs only if it has something new to read.

use std::collections::BTreeMap;
use std::sync::Arc;
use std::time::{Duration, Instant};

use anyhow::Result;
use serde_json::{json, Value};
use theseus_kernel::{
    Action, Authority, Completion, Execution, Kernel, KernelError, Outcome as ActionOutcome,
    Proposal, RetryClass, TurnEnd, TurnGuard, Wake,
};
use theseus_protocol::{
    notify, LoopEnded, LoopStarted, ModelDelta, TurnStarted, TurnSubmitResult, Usage,
};
use theseus_store::NewRecord;

use crate::advancer::{Advancer, Decision, LoopOutcome, UntilNoToolCalls};
use crate::bus::{EventSink, SessionBus};
use crate::catalog::Catalog;
use crate::compiler::{compile, CompileInput, Compiled, Recompile, RequestSpec};
use crate::ledger::LedgerRow;
use crate::node::{Body, Node};
use crate::provider::{Delta, ModelResponse, Provider, ProviderError, ToolUse};
use crate::session::{title_from, SessionRecord, TargetRef};
use crate::store::Store;
use crate::toolrun::{CallOutcome, ToolRuntime, TurnCtx};
use crate::trace::Trace;
use crate::Config;

/// The tool name of the provider-call action (spec §3.2b).
pub const PROVIDER_TOOL: &str = "provider.messages";

/// The persona at the front of every system prompt. Frozen text: it sits at
/// the start of the cached prefix, so it never interpolates anything.
pub const PERSONA: &str = "You are Theseus, a coding and operations agent working for your operator through a harness that records everything you do. Be direct and concise; lead with what you found or did. When you are unsure, say so plainly.";

/// What a turn runs against, resolved from a profile plus any raw overrides.
#[derive(Debug, Clone)]
pub struct Target {
    pub profile: String,
    pub provider: String,
    pub model: String,
    pub max_tokens: u32,
    pub system: Option<String>,
    pub effort: Option<String>,
    pub thinking_display: String,
    pub max_loops: u32,
    pub refusal_fallbacks: bool,
}

pub struct TurnRunner {
    pub cfg: Arc<Config>,
    /// Providers by name; `cfg.model.provider` is the default.
    pub providers: BTreeMap<String, Arc<dyn Provider>>,
    pub store: Store,
    /// The durable kernel: admission, the per-execution turn lock, budgets,
    /// and the action/completion record of every provider and tool call.
    pub kernel: Arc<Kernel>,
    /// Woken whenever a turn ends or an execution changes.
    pub admission: Arc<tokio::sync::Notify>,
    pub catalog: Arc<Catalog>,
    pub tools: Arc<ToolRuntime>,
    pub bus: Arc<SessionBus>,
}

/// One turn to run.
pub struct TurnRequest {
    pub session: SessionRecord,
    /// `None`: a continuation.
    pub input: Option<String>,
    pub target: Target,
    pub sink: EventSink,
    /// The client that asked (`web#3`) or `harness` for a continuation.
    pub author: String,
    pub recompile: Option<Recompile>,
}

/// How long a turn may wait for admission before the client gets an error.
const ADMISSION_WAIT_MAX: Duration = Duration::from_secs(600);
/// The principal of every local protocol client (file permissions are the auth).
pub const OPERATOR: &str = "operator";

fn turn_error(
    class: &str,
    session: &str,
    turn: &str,
    elapsed_ms: u64,
    source: anyhow::Error,
) -> anyhow::Error {
    TurnError {
        class: class.into(),
        transient: false,
        usage_unknown: false,
        turn_id: turn.into(),
        session_id: session.into(),
        elapsed_ms,
        trace: None,
        usage: Usage::default(),
        cost_usd: Some(0.0),
        tool_calls: 0,
        source,
    }
    .into()
}

/// A turn in flight: the context it hands the tool layer, its trace, and
/// what it has done so far, which becomes its result and its session's books.
struct Turn<'a> {
    /// `loop_index` stays `None`; a loop's tool calls get their own copy.
    tc: TurnCtx<'a>,
    target: &'a Target,
    continuation: bool,
    started: Instant,
    trace: Trace,
    loops: u32,
    output: String,
    usage: Usage,
    /// `None` once any call's price is unknown.
    cost: Option<f64>,
    tool_calls: u32,
    /// The model's latest response.
    last: Option<ModelResponse>,
    /// The tool call waiting on a confirm.
    awaiting: Option<String>,
    /// Jobs this turn started or resumed in the background.
    background: Vec<String>,
    stop_reason: String,
}

impl<'a> Turn<'a> {
    fn start(tc: TurnCtx<'a>, target: &'a Target, continuation: bool, arrived: Instant) -> Self {
        let started = Instant::now();
        let trace = Trace::start_at(
            arrived,
            "turn",
            "turn",
            json!({
                "turn_id": tc.turn_id,
                "session_id": tc.session_id,
                "profile": target.profile,
                "provider": target.provider,
                "model": target.model,
                "continuation": continuation,
                "started_unix_ms": theseus_protocol::now_unix_ms(),
            }),
        );
        Self {
            tc,
            target,
            continuation,
            started,
            trace,
            loops: 0,
            output: String::new(),
            usage: Usage::default(),
            cost: Some(0.0),
            tool_calls: 0,
            last: None,
            awaiting: None,
            background: Vec::new(),
            stop_reason: String::new(),
        }
    }

    /// Tell the trace, the session's clients, and the ledger that the turn began.
    fn announce(&mut self, input: Option<&str>, author: &str, lock_wait_us: u64) {
        let (guard, target) = (self.tc.guard, self.target);
        self.trace.record(
            "admission.wait",
            "lock",
            0,
            lock_wait_us,
            json!({"execution_id": guard.execution_id, "turn": guard.turn, "note": "kernel admission + per-execution turn lock"}),
        );
        self.tc.sink.send(
            notify::TURN_STARTED,
            TurnStarted {
                session_id: self.tc.session_id.into(),
                turn_id: self.tc.turn_id.into(),
                execution_id: Some(guard.execution_id.clone()),
                continuation: self.continuation,
            },
        );
        self.tc.ledger(
            "turn.started",
            json!({"input_chars": input.map(|s| s.chars().count()), "profile": target.profile, "provider": target.provider, "model": target.model, "execution_id": guard.execution_id, "kernel_turn": guard.turn, "continuation": self.continuation, "author": author}),
        );
    }

    /// Count the turn in its session. The success path and both failure
    /// exits call it, so a turn that fails in a later loop keeps what its
    /// earlier loops spent.
    fn close_books(&self, session: &mut SessionRecord) {
        session.turns += 1;
        session.last_turn_id = Some(self.tc.turn_id.to_string());
        session.last_active_ms = theseus_protocol::now_unix_ms();
        session.tool_calls += self.tool_calls as u64;
        session.cost_usd += self.cost.unwrap_or(0.0);
        add_usage(&mut session.usage, &self.usage);
    }
}

/// A turn that failed after it began, in a way the client is told about:
/// the kernel would not plan the provider call (the budget ran out, the
/// execution was cancelled), or the call failed. Any other error is a fault
/// and propagates as it is.
struct Failure {
    class: String,
    transient: bool,
    usage_unknown: bool,
    /// The `turn.failed` row's reason.
    reason: String,
    source: anyhow::Error,
}

impl TurnRunner {
    /// Resolve what a turn runs against. Precedence: raw `provider`/`model`
    /// overrides > the named `profile` > the live profile.
    pub fn resolve_target(
        &self,
        live_profile: &str,
        profile: Option<&str>,
        provider: Option<&str>,
        model: Option<&str>,
    ) -> Result<Target> {
        let profiles = self.cfg.all_profiles();
        let name = profile.unwrap_or(live_profile);
        let prof = profiles.get(name).ok_or_else(|| {
            anyhow::anyhow!(
                "unknown profile {name:?}; configured: {}",
                profiles.keys().cloned().collect::<Vec<_>>().join(", ")
            )
        })?;
        let provider = provider.unwrap_or(&prof.provider).to_string();
        if !self.providers.contains_key(&provider) {
            anyhow::bail!(
                "unknown provider {provider:?}; configured: {}",
                self.providers
                    .keys()
                    .cloned()
                    .collect::<Vec<_>>()
                    .join(", ")
            );
        }
        let model = model.unwrap_or(&prof.model).to_string();
        let max_tokens = prof
            .max_output_tokens
            .or_else(|| self.catalog.get(&model).map(|e| e.max_output_tokens))
            .unwrap_or(16_384);
        Ok(Target {
            profile: name.to_string(),
            provider,
            model,
            max_tokens,
            system: prof.system.clone(),
            effort: prof.effort.clone(),
            thinking_display: prof.thinking_display.clone(),
            max_loops: prof.max_loops,
            refusal_fallbacks: prof.refusal_fallbacks,
        })
    }

    /// A continuation keeps the session's last model so its thinking blocks stay readable.
    pub fn target_for_session(&self, s: &SessionRecord, live_profile: &str) -> Result<Target> {
        if let Some(t) = &s.last_target {
            if let Ok(tg) = self.resolve_target(
                live_profile,
                Some(&t.profile),
                Some(&t.provider),
                Some(&t.model),
            ) {
                return Ok(tg);
            }
        }
        self.resolve_target(live_profile, None, None, None)
    }

    fn first_party(&self, provider: &str) -> bool {
        self.cfg
            .all_providers()
            .get(provider)
            .map(|p| p.api_base.contains("api.anthropic.com"))
            .unwrap_or(false)
    }

    /// The system prompt: persona, the tools paragraph, the profile's own text.
    /// Deterministic for a config; a change is a `system_changed` recompile.
    pub fn system_text(&self, target: &Target) -> String {
        let mut parts = vec![PERSONA.to_string()];
        let note = self.tools.system_note();
        if !note.is_empty() {
            parts.push(note);
        }
        if let Some(s) = target.system.as_ref().filter(|s| !s.trim().is_empty()) {
            parts.push(s.clone());
        }
        parts.join("\n\n")
    }

    pub fn request_spec(&self, target: &Target) -> RequestSpec {
        RequestSpec {
            profile: target.profile.clone(),
            provider: target.provider.clone(),
            model: target.model.clone(),
            max_tokens: target.max_tokens,
            system_text: self.system_text(target),
            tools: self.tools.definitions(),
            effort: target.effort.clone(),
            thinking_display: target.thinking_display.clone(),
            refusal_fallbacks: target.refusal_fallbacks,
            first_party: self.first_party(&target.provider),
        }
    }

    /// Make sure the session has a kernel execution (sessions written before
    /// M2 have none) and return it.
    fn execution_for(&self, session: &mut SessionRecord) -> Result<Execution> {
        if let Some(id) = &session.execution_id {
            if let Some(e) = self.kernel.execution(id)? {
                return Ok(e);
            }
        }
        let e = self.kernel.open_execution(
            &session.session_id,
            session.kind,
            Authority {
                principal: OPERATOR.to_string(),
                ..Default::default()
            },
            None,
            None,
        )?;
        session.execution_id = Some(e.id.clone());
        self.store.put_session(&session.session_id, session)?;
        Ok(e)
    }

    /// Take the kernel turn. With `wait`, wait for admission (ceiling, or
    /// another turn on the same execution); without it (continuations), give
    /// up at once and let the driver try again.
    async fn admit(
        &self,
        exec_id: &str,
        arrived: Instant,
        wait: bool,
    ) -> Result<Option<TurnGuard>> {
        loop {
            match self.kernel.admit(exec_id) {
                Ok(g) => return Ok(Some(g)),
                Err(e) => match e.downcast_ref::<KernelError>() {
                    Some(KernelError::AdmissionFull { .. })
                    | Some(KernelError::TurnHeld { .. })
                    | Some(KernelError::NotRunnable {
                        state: "running", ..
                    }) => {
                        if !wait {
                            return Ok(None);
                        }
                    }
                    Some(KernelError::NotRunnable {
                        state: "waiting" | "blocked",
                        ..
                    }) => {
                        if !wait {
                            return Ok(None);
                        }
                        self.kernel.wake_input(exec_id)?;
                        continue;
                    }
                    Some(KernelError::NotRunnable { state, .. }) => {
                        return Err(turn_error(
                            &format!("execution_{state}"),
                            "",
                            "",
                            arrived.elapsed().as_millis() as u64,
                            e,
                        ));
                    }
                    _ => return Err(e),
                },
            }
            if arrived.elapsed() > ADMISSION_WAIT_MAX {
                anyhow::bail!("admission wait exceeded {:?}", ADMISSION_WAIT_MAX);
            }
            let _ =
                tokio::time::timeout(Duration::from_millis(50), self.admission.notified()).await;
        }
    }

    pub async fn run(&self, mut req: TurnRequest) -> Result<TurnSubmitResult> {
        let arrived = Instant::now();
        let continuation = req.input.is_none();
        let exec = self.execution_for(&mut req.session)?;
        if !continuation {
            if let Err(e) = self.kernel.wake_input(&exec.id) {
                if let Some(KernelError::NotRunnable { state, .. }) =
                    e.downcast_ref::<KernelError>()
                {
                    return Err(turn_error(
                        &format!("execution_{state}"),
                        &req.session.session_id,
                        "",
                        0,
                        e,
                    ));
                }
                return Err(e);
            }
        }
        let Some(guard) = self.admit(&exec.id, arrived, !continuation).await? else {
            anyhow::bail!("execution {} is not ready for a continuation turn", exec.id);
        };
        let admission_wait_us = arrived.elapsed().as_micros() as u64;
        let failure_sink = req.sink.clone();
        let r = self
            .run_inner(&guard, req, arrived, admission_wait_us)
            .await;
        let (end, rewake) = match &r {
            Ok((_, end, rewake)) => (end.clone(), *rewake),
            Err(_) => (TurnEnd::Wait { wake: Wake::Input }, false),
        };
        let exec_id = guard.execution_id.clone();
        if let Err(e) = self.kernel.end_turn(guard, end) {
            tracing::warn!(error = %e, "end_turn failed");
        }
        if let Err(e) = &r {
            let te = e.downcast_ref::<TurnError>();
            failure_sink.send(
                notify::TURN_FAILED,
                theseus_protocol::TurnFailed {
                    session_id: failure_sink.session_id.clone(),
                    turn_id: te.map(|t| t.turn_id.clone()).filter(|t| !t.is_empty()),
                    execution_id: Some(exec_id.clone()),
                    continuation,
                    class: te.map(|t| t.class.clone()),
                    error: format!("{e:#}"),
                },
            );
        }
        if rewake {
            // A background result landed while the turn ran; the model has not
            // read it yet, so the driver takes another turn.
            let _ = self.kernel.wake(&exec_id, "late_result");
        }
        self.admission.notify_waiters();
        r.map(|(res, _, _)| res)
    }

    /// One turn under the lock (§3.3): catch up on what happened while no
    /// turn ran, write the input, run loops while the model has something new
    /// to read, then book the turn and decide where the execution waits.
    async fn run_inner(
        &self,
        guard: &TurnGuard,
        req: TurnRequest,
        arrived: Instant,
        lock_wait_us: u64,
    ) -> Result<(TurnSubmitResult, TurnEnd, bool)> {
        let TurnRequest {
            mut session,
            input,
            target,
            sink,
            author,
            recompile,
        } = req;
        let provider = self
            .providers
            .get(&target.provider)
            .cloned()
            .ok_or_else(|| anyhow::anyhow!("unknown provider {:?}", target.provider))?;
        // The caller's copy may be stale: re-read under the lock.
        if let Some(fresh) = self
            .store
            .get_session::<SessionRecord>(&session.session_id)?
        {
            session = fresh;
        }
        let sid = session.session_id.clone();
        let turn_id = crate::new_id("turn");
        let tc = TurnCtx {
            kernel: &self.kernel,
            store: &self.store,
            guard,
            session_id: &sid,
            execution_id: &guard.execution_id,
            turn_id: &turn_id,
            loop_index: None,
            sink: &sink,
            confirm_ttl_ms: self.kernel.config().confirm_ttl_ms,
        };
        let mut t = Turn::start(tc, &target, input.is_none(), arrived);
        t.announce(input.as_deref(), &author, lock_wait_us);

        // 1. What happened while no turn was running.
        let caught_up = self.catch_up(&mut t, input.is_some()).await?;

        // 2. The new input.
        if let Some(text) = &input {
            let node = Node::user(&sid, Some(&turn_id), &author, text);
            if session.title.is_none() {
                session.title = Some(title_from(text));
            }
            self.store.append(vec![node.record()?])?;
            t.tc.node_written(&node);
        }

        // 3. The loops, while the model has something new to read.
        let mut run_model = self.has_news(&mut t, input.is_some() || caught_up > 0)?;
        let spec = self.request_spec(&target);
        let mut force = recompile.or(session.pending_recompile.take());
        while run_model {
            let i = t.loops;
            t.loops += 1;
            t.trace
                .enter(&format!("loop {i}"), "loop", json!({"loop": i}));
            let compiled = self.compile_step(&mut t, &mut session, &spec, force.take(), i)?;
            let (resp, node) = match self
                .call_model(&mut t, provider.as_ref(), &compiled, i)
                .await?
            {
                Ok(called) => called,
                Err(failure) => return Err(self.fail(t, &mut session, failure)),
            };
            let uses = resp.tool_uses();
            let answered = self.run_tools(&mut t, &resp, &uses, &node, i).await?;
            run_model = self.advance(&mut t, &resp, uses.len(), answered, i);
            t.last = Some(resp);
        }

        // 4. Results that settled while this turn ran, the books, the park.
        self.finish(t, &mut session)
    }

    /// What happened while no turn was running: results that settled become
    /// late result nodes, and pending calls resume (a confirm answered, a
    /// restart survived). Returns how many nodes that wrote.
    async fn catch_up(&self, t: &mut Turn<'_>, has_input: bool) -> Result<u32> {
        let t0 = t.trace.now_us();
        let settled = self.kernel.take_results(t.tc.guard)?;
        let absorbed = self.tools.absorb(&t.tc, &settled)?;
        let resumed = self.tools.resume(&t.tc, has_input).await?;
        t.trace.record(
            "continuation",
            "tool",
            t0,
            t.trace.now_us(),
            json!({"settled": settled.len(), "late_results": absorbed, "resumed": resumed.wrote, "awaiting": resumed.awaiting, "background": resumed.background}),
        );
        t.awaiting = resumed.awaiting;
        t.background = resumed.background;
        Ok(absorbed + resumed.wrote)
    }

    /// Does the model have anything new to read: input or results it has not
    /// seen, or a user message still waiting for a reply? If not, the turn's
    /// stop reason says why.
    fn has_news(&self, t: &mut Turn<'_>, wrote: bool) -> Result<bool> {
        if t.awaiting.is_some() {
            t.stop_reason = "awaiting_confirm".into();
            return Ok(false);
        }
        if wrote {
            return Ok(true);
        }
        let nodes = self.store.session_nodes(t.tc.session_id)?;
        let last = nodes
            .iter()
            .rev()
            .find(|(_, n)| !matches!(n.body, Body::ToolCall { .. }));
        let awaiting_reply = matches!(last.map(|(_, n)| &n.body), Some(Body::UserMessage { .. }));
        if !awaiting_reply {
            t.stop_reason = "nothing_new".into();
        }
        Ok(awaiting_reply)
    }

    /// Render the session into this loop's request: an append to the current
    /// compilation, or a recompile (persisted with the session's pointer).
    fn compile_step(
        &self,
        t: &mut Turn<'_>,
        session: &mut SessionRecord,
        spec: &RequestSpec,
        force: Option<Recompile>,
        i: u32,
    ) -> Result<Compiled> {
        let sid = t.tc.session_id;
        let c0 = t.trace.now_us();
        let nodes = self.store.session_nodes(sid)?;
        let current = match session.compilation_id.as_deref() {
            Some(id) => self.store.get_compilation(id)?,
            None => None,
        };
        let compiled = compile(CompileInput {
            session_id: sid,
            current: current.as_ref(),
            nodes: &nodes,
            last_position: self.store.last_position(),
            spec,
            catalog: &self.catalog,
            force,
            window_override: None,
        });
        if compiled.new_compilation {
            self.persist_compilation(&compiled, session, t.tc.turn_id)?;
        }
        let c1 = t.trace.now_us();
        let summary = json!({
            "session_id": sid,
            "turn_id": t.tc.turn_id,
            "loop": i,
            "decision": compiled.decision(),
            "trigger": compiled.trigger,
            "compilation_id": compiled.compilation.id,
            "strategy": compiled.compilation.strategy,
            "prefix_nodes": compiled.prefix_nodes,
            "tail_nodes": compiled.tail_nodes,
            "messages": compiled.messages,
            "est_tokens": compiled.est_tokens,
            "digest": compiled.digest,
            "repairs": compiled.repairs,
            "tools": spec.tools.len(),
            "nodes_scanned": nodes.len(),
        });
        t.trace
            .record("compile", "compile", c0, c1, summary.clone());
        t.tc.ledger("context.compiled", summary.clone());
        t.tc.sink.send(notify::CONTEXT_COMPILED, &summary);
        t.tc.sink.send(
            notify::LOOP_STARTED,
            LoopStarted {
                turn_id: t.tc.turn_id.into(),
                loop_index: i,
                model: t.target.model.clone(),
                tools_offered: spec.tools.len() as u32,
            },
        );
        t.tc.ledger("loop.started", json!({"loop": i}));
        Ok(compiled)
    }

    /// The provider call, as a kernel action (§3.16): planned (its budget
    /// reservation can end the turn), dispatched, streamed, and settled. The
    /// outer error is a fault; the inner one is a failure the turn reports.
    async fn call_model(
        &self,
        t: &mut Turn<'_>,
        provider: &dyn Provider,
        compiled: &Compiled,
        i: u32,
    ) -> Result<Result<(ModelResponse, Node), Failure>> {
        let target = t.target;
        let proposal = Proposal {
            tool: PROVIDER_TOOL.into(),
            args: json!({"provider": target.provider, "model": target.model, "max_tokens": target.max_tokens, "loop": i, "turn_id": t.tc.turn_id, "digest": compiled.digest}),
            resource: Some(target.provider.clone()),
            policy_context: json!({"profile": target.profile}),
        };
        let reserve = target.max_tokens as u64 + compiled.est_tokens;
        let o0 = t.trace.now_us();
        let action = match self.kernel.plan_action(
            t.tc.guard,
            &proposal,
            RetryClass::SafeToRepeat,
            Some(self.cfg.model.timeouts.total_secs * 1000),
            reserve,
        ) {
            Ok(a) => a,
            Err(e) => {
                let exhausted = matches!(
                    e.downcast_ref::<KernelError>(),
                    Some(KernelError::BudgetExhausted { .. })
                );
                let class = if exhausted {
                    "budget_exhausted"
                } else {
                    "kernel"
                };
                return Ok(Err(Failure {
                    class: class.into(),
                    transient: false,
                    usage_unknown: false,
                    reason: format!("{class}: {e}"),
                    source: e,
                }));
            }
        };
        self.kernel
            .authorize(&action.correlation_id, &proposal, None)?;
        self.kernel.dispatch(&action.correlation_id, None)?;
        t.trace.record(
            "action.outbox",
            "store",
            o0,
            t.trace.now_us(),
            json!({"correlation_id": action.correlation_id, "tool": action.tool, "reserved_units": reserve}),
        );
        let started_ms = theseus_protocol::now_unix_ms();

        let (sink, tid) = (t.tc.sink.clone(), t.tc.turn_id.to_string());
        let mut on_delta = move |d: Delta<'_>| match d {
            Delta::Text(text) => sink.send(
                notify::MODEL_DELTA,
                ModelDelta {
                    turn_id: tid.clone(),
                    loop_index: i,
                    text: text.to_string(),
                },
            ),
            Delta::Thinking(text) => sink.send(
                notify::MODEL_THINKING,
                json!({"turn_id": tid, "loop_index": i, "text": text}),
            ),
            Delta::ToolUseStart { .. } => {}
        };
        let call_started = Instant::now();
        t.trace.enter(
            "provider.call",
            "provider",
            json!({"provider": target.provider, "model": target.model, "max_tokens": target.max_tokens, "digest": compiled.digest}),
        );
        let call_t0 = t.trace.now_us();
        match provider
            .stream_message(&compiled.request, &mut on_delta)
            .await
        {
            Ok(resp) => {
                if let Some(fb) = resp.timing.first_byte_ms {
                    t.trace
                        .mark_at(call_t0 + fb * 1000, "first_byte", "mark", Value::Null);
                }
                if let Some(ft) = resp.timing.first_token_ms {
                    t.trace
                        .mark_at(call_t0 + ft * 1000, "first_token", "mark", Value::Null);
                }
                t.trace.exit(json!({
                    "request_id": resp.request_id,
                    "served_model": resp.model,
                    "usage": resp.usage,
                    "stop_reason": resp.stop_reason,
                    "blocks": resp.content.len(),
                    "output_chars": resp.text.chars().count(),
                    "rate_limit_tokens_remaining": resp.rate_limit.tokens_remaining,
                }));
                let node = self.settle_call(t, &action, compiled, &resp, started_ms, i)?;
                Ok(Ok((resp, node)))
            }
            Err(e) => Ok(Err(self.settle_failed(
                t,
                &action,
                started_ms,
                call_started,
                e,
                i,
            ))),
        }
    }

    /// Settle a call that answered: the assistant node rides in the frame of
    /// the call's completion, and the call's usage, cost, and text join the
    /// turn's books.
    fn settle_call(
        &self,
        t: &mut Turn<'_>,
        action: &Action,
        compiled: &Compiled,
        resp: &ModelResponse,
        started_ms: u64,
        i: u32,
    ) -> Result<Node> {
        let target = t.target;
        let call_cost = self
            .catalog
            .cost_usd(&resp.model, &resp.usage)
            .or_else(|| self.catalog.cost_usd(&target.model, &resp.usage));
        t.cost = match (t.cost, call_cost) {
            (Some(a), Some(b)) => Some(a + b),
            _ => None,
        };
        let node = Node::assistant(
            t.tc.session_id,
            t.tc.turn_id,
            i,
            Body::AssistantMessage {
                blocks: resp.content.clone(),
                model: resp.model.clone(),
                provider: target.provider.clone(),
                stop_reason: resp.stop_reason.clone(),
                usage: resp.usage.clone(),
                cost_usd: call_cost,
                catalog_version: Some(self.catalog.version.clone()),
                request_id: resp.request_id.clone(),
                correlation_id: Some(action.correlation_id.clone()),
                compilation_id: Some(compiled.compilation.id.clone()),
                request_digest: Some(compiled.digest.clone()),
            },
        );
        let units = resp.usage.input_tokens
            + resp.usage.output_tokens
            + resp.usage.cache_read_input_tokens
            + resp.usage.cache_creation_input_tokens;
        let s0 = t.trace.now_us();
        self.kernel.accept_completion_with(
            &Completion {
                correlation_id: action.correlation_id.clone(),
                outcome: ActionOutcome::Succeeded,
                result_ref: Some(node.id.clone()),
                external_op_id: resp.request_id.clone(),
                started_at_ms: started_ms,
                finished_at_ms: theseus_protocol::now_unix_ms(),
                producer: format!("provider:{}", target.provider),
                signature: None,
                usage_units: Some(units),
                detail: Some(json!({"served_model": resp.model, "message_id": resp.message_id})),
            },
            vec![node.record()?],
        )?;
        t.trace.record(
            "action.settle",
            "store",
            s0,
            t.trace.now_us(),
            json!({"correlation_id": action.correlation_id, "outcome": "succeeded", "units": units, "node_id": node.id}),
        );
        t.tc.node_written(&node);
        add_usage(&mut t.usage, &resp.usage);
        if !resp.text.is_empty() {
            if !t.output.is_empty() {
                t.output.push_str("\n\n");
            }
            t.output.push_str(&resp.text);
        }
        t.tc.ledger(
            "provider.call",
            json!({
                "loop": i,
                "provider": target.provider,
                "model": resp.model,
                "request_id": resp.request_id,
                "usage": resp.usage,
                "cost_usd": call_cost,
                "catalog_version": self.catalog.version,
                "timing": resp.timing,
                "rate_limit": resp.rate_limit,
                "stop_reason": resp.stop_reason,
                "stop_details": resp.stop_details,
                "blocks": resp.content.len(),
                "input_transformations": resp.input_transformations,
                "node_id": node.id,
            }),
        );
        if resp.stop_reason.as_deref() == Some("refusal") {
            t.tc.ledger(
                "provider.refusal",
                json!({"stop_details": resp.stop_details, "model": resp.model}),
            );
        }
        Ok(node)
    }

    /// Settle a call that failed: as failed, or as unknown when the provider
    /// may have done the work, and tell the ledger why.
    fn settle_failed(
        &self,
        t: &mut Turn<'_>,
        action: &Action,
        started_ms: u64,
        call_started: Instant,
        e: anyhow::Error,
        i: u32,
    ) -> Failure {
        let target = t.target;
        let pe = e.downcast_ref::<ProviderError>();
        let (class, transient, unknown) = pe
            .map(|p| (p.class(), p.is_transient(), p.usage_unknown()))
            .unwrap_or(("unknown", false, true));
        t.trace
            .exit(json!({"error": class, "message": e.to_string()}));
        let s0 = t.trace.now_us();
        let settled = self.kernel.accept_completion(&Completion {
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
            producer: format!("provider:{}", target.provider),
            signature: None,
            usage_units: if unknown { None } else { Some(0) },
            detail: Some(json!({"class": class})),
        });
        t.trace.record(
            "action.settle",
            "store",
            s0,
            t.trace.now_us(),
            json!({"correlation_id": action.correlation_id, "outcome": if unknown {"unknown"} else {"failed"}, "result": settled.as_ref().map(|a| format!("{a:?}")).unwrap_or_else(|e| e.to_string())}),
        );
        t.tc.ledger(
            "provider.error",
            json!({
                "loop": i,
                "provider": target.provider,
                "model": target.model,
                "class": class,
                "transient": transient,
                "usage_unknown": unknown,
                "elapsed_ms": call_started.elapsed().as_millis() as u64,
                "detail": pe.map(|p| serde_json::to_value(p).unwrap_or(Value::Null)),
                "message": e.to_string(),
            }),
        );
        Failure {
            class: class.into(),
            transient,
            usage_unknown: unknown,
            reason: format!("provider:{class}"),
            source: e,
        }
    }

    /// Gate and run the model's tool calls in order, until one waits on a
    /// confirm. A response that did not stop for tools runs none of them.
    /// Returns how many calls have an answer.
    async fn run_tools(
        &self,
        t: &mut Turn<'_>,
        resp: &ModelResponse,
        uses: &[ToolUse],
        node: &Node,
        i: u32,
    ) -> Result<u32> {
        let tc = TurnCtx {
            loop_index: Some(i),
            ..t.tc
        };
        let stop = resp.stop_reason.as_deref();
        if stop != Some("tool_use") {
            let why = format!(
                "the response ended with stop reason `{}`",
                stop.unwrap_or("none")
            );
            for u in uses {
                self.tools.not_run(&tc, u, &why)?;
            }
            return Ok(0);
        }
        let mut answered = 0;
        for u in uses {
            t.tool_calls += 1;
            let t0 = t.trace.now_us();
            let invalid = resp.invalid_tool_inputs.get(&u.id).map(String::as_str);
            let outcome = self.tools.process(&tc, &node.id, u, invalid).await?;
            t.trace.record(
                &format!("tool {}", u.name),
                "tool",
                t0,
                t.trace.now_us(),
                json!({"tool_use_id": u.id, "outcome": format!("{outcome:?}")}),
            );
            match outcome {
                CallOutcome::AwaitingConfirm { correlation_id } => {
                    t.awaiting = Some(correlation_id);
                    break;
                }
                CallOutcome::Background { correlation_id } => {
                    t.background.push(correlation_id);
                    answered += 1;
                }
                CallOutcome::Done { .. } => answered += 1,
            }
        }
        Ok(answered)
    }

    /// The Advancer decides whether the turn continues; the loop's end is
    /// recorded either way.
    fn advance(
        &self,
        t: &mut Turn<'_>,
        resp: &ModelResponse,
        uses: usize,
        answered: u32,
        i: u32,
    ) -> bool {
        let advancer = UntilNoToolCalls {
            max_loops: t.target.max_loops,
        };
        let stop = resp.stop_reason.as_deref();
        let outcome = LoopOutcome {
            loop_index: i,
            provider_stop_reason: resp.stop_reason.clone(),
            tool_calls: answered,
            output_chars: resp.text.chars().count(),
        };
        let a0 = t.trace.now_us();
        let decision = if t.awaiting.is_some() {
            Decision::EndTurn("awaiting_confirm".into())
        } else if matches!(stop, Some("refusal") | Some("max_tokens")) {
            Decision::EndTurn(stop.unwrap_or_default().to_string())
        } else {
            advancer.decide(&outcome)
        };
        t.trace.record(
            "advancer",
            "advancer",
            a0,
            t.trace.now_us(),
            json!({"advancer": advancer.name(), "decision": decision.label()}),
        );
        t.tc.sink.send(
            notify::LOOP_ENDED,
            LoopEnded {
                turn_id: t.tc.turn_id.into(),
                loop_index: i,
                provider_stop_reason: resp.stop_reason.clone(),
                tool_calls: uses as u32,
                advancer: advancer.name().into(),
                decision: decision.label(),
            },
        );
        t.tc.ledger(
            "loop.ended",
            json!({"loop": i, "outcome": outcome, "advancer": advancer.name(), "decision": decision, "usage": resp.usage}),
        );
        t.trace
            .exit(json!({"decision": decision.label(), "usage": resp.usage}));
        match decision {
            Decision::Continue => true,
            Decision::EndTurn(reason) => {
                t.stop_reason = reason;
                false
            }
        }
    }

    /// End a turn that failed after it began: book what its finished loops
    /// spent, close its trace, and write `turn.failed`.
    fn fail(&self, t: Turn<'_>, session: &mut SessionRecord, f: Failure) -> anyhow::Error {
        t.close_books(session);
        let trace = t
            .trace
            .finish(json!({"outcome": "failed", "class": f.class}));
        t.tc.ledger(
            "turn.failed",
            json!({"loops": t.loops, "reason": f.reason, "usage_so_far": t.usage, "cost_usd": t.cost, "tool_calls": t.tool_calls}),
        );
        let _ = self.store.put_session(t.tc.session_id, session);
        TurnError {
            class: f.class,
            transient: f.transient,
            usage_unknown: f.usage_unknown,
            turn_id: t.tc.turn_id.into(),
            session_id: t.tc.session_id.into(),
            elapsed_ms: t.started.elapsed().as_millis() as u64,
            trace: Some(trace),
            usage: t.usage,
            cost_usd: t.cost,
            tool_calls: t.tool_calls,
            source: f.source,
        }
        .into()
    }

    /// Absorb results that settled while the turn ran, book the turn, write
    /// its result, and park the execution. The `bool` asks for another turn:
    /// a late result the model has not read.
    fn finish(
        &self,
        mut t: Turn<'_>,
        session: &mut SessionRecord,
    ) -> Result<(TurnSubmitResult, TurnEnd, bool)> {
        let settled = self.kernel.take_results(t.tc.guard)?;
        let late = self.tools.absorb(&t.tc, &settled)?;
        t.close_books(session);
        let target = t.target;
        session.last_target = Some(TargetRef {
            profile: target.profile.clone(),
            provider: target.provider.clone(),
            model: target.model.clone(),
        });
        let w0 = t.trace.now_us();
        self.store.put_session(t.tc.session_id, session)?;
        t.trace
            .record("session.write", "store", w0, t.trace.now_us(), Value::Null);

        let last = t.last.as_ref();
        let mut result = TurnSubmitResult {
            session_id: t.tc.session_id.into(),
            turn_id: t.tc.turn_id.into(),
            loops: t.loops,
            output: std::mem::take(&mut t.output),
            stop_reason: if t.stop_reason.is_empty() {
                "end_turn".into()
            } else {
                t.stop_reason.clone()
            },
            provider_stop_reason: last.and_then(|r| r.stop_reason.clone()),
            model: last.map_or_else(|| target.model.clone(), |r| r.model.clone()),
            provider: target.provider.clone(),
            profile: target.profile.clone(),
            usage: t.usage.clone(),
            elapsed_ms: t.started.elapsed().as_millis() as u64,
            first_token_ms: last.and_then(|r| r.timing.first_token_ms),
            request_id: last.and_then(|r| r.request_id.clone()),
            trace: None,
            execution_id: Some(t.tc.execution_id.into()),
            cost_usd: t.cost,
            tool_calls: t.tool_calls,
            awaiting_confirm: t.awaiting.clone(),
            stop_details: last.and_then(|r| r.stop_details.clone()),
            continuation: t.continuation,
        };
        t.tc.ledger(
            "turn.ended",
            json!({"loops": result.loops, "stop_reason": result.stop_reason, "usage": result.usage, "cost_usd": result.cost_usd, "tool_calls": result.tool_calls, "session_usage": session.usage, "elapsed_ms": result.elapsed_ms, "first_token_ms": result.first_token_ms, "provider": result.provider, "model": result.model, "awaiting_confirm": result.awaiting_confirm, "continuation": result.continuation, "late_results": late}),
        );
        result.trace = Some(t.trace.finish(json!({
            "outcome": "complete",
            "loops": result.loops,
            "stop_reason": result.stop_reason,
            "usage": result.usage,
        })));
        t.tc.ledger(
            "turn.trace",
            serde_json::to_value(&result.trace).unwrap_or(Value::Null),
        );
        t.tc.sink.send(notify::TURN_ENDED, &result);
        let end = self.park(t.tc.execution_id, t.awaiting, &t.background)?;
        Ok((result, end, late > 0))
    }

    /// Where the execution waits: on the confirm, on outstanding jobs, or on
    /// the next input.
    fn park(
        &self,
        exec_id: &str,
        awaiting: Option<String>,
        background: &[String],
    ) -> Result<TurnEnd> {
        let outstanding: Vec<String> = self
            .kernel
            .execution(exec_id)?
            .map(|e| e.outstanding)
            .unwrap_or_default()
            .into_iter()
            .filter(|c| {
                background.contains(c)
                    || self
                        .kernel
                        .action(c)
                        .ok()
                        .flatten()
                        .map(|a| a.tool != PROVIDER_TOOL)
                        .unwrap_or(false)
            })
            .collect();
        let wake = if let Some(c) = awaiting {
            Wake::Confirm { confirm_id: c }
        } else if !outstanding.is_empty() {
            Wake::Actions {
                correlation_ids: outstanding,
            }
        } else {
            Wake::Input
        };
        Ok(TurnEnd::Wait { wake })
    }

    /// A new compilation (it carries its own `derived_from`) and the
    /// session's pointer, in one frame.
    fn persist_compilation(
        &self,
        compiled: &Compiled,
        session: &mut SessionRecord,
        turn_id: &str,
    ) -> Result<()> {
        let c = &compiled.compilation;
        let mut records = vec![
            NewRecord::json(theseus_store::kinds::COMPILATION, Some(&c.id), c)?
                .scoped(&c.session_id),
        ];
        session.compilation_id = Some(c.id.clone());
        records.push(NewRecord::json(
            theseus_store::kinds::SESSION,
            Some(&session.session_id),
            &*session,
        )?);
        records.push(NewRecord::json(
            theseus_store::kinds::LEDGER,
            None,
            &LedgerRow::new(
                "context.recompiled",
                Some(&c.session_id),
                Some(turn_id),
                json!({"compilation_id": c.id, "trigger": c.trigger, "strategy": c.strategy, "as_of": c.as_of, "includes": c.includes.len(), "derived_from": c.derived_from, "strip_thinking": c.manifest.strip_thinking, "model": c.manifest.model}),
            ),
        )?);
        self.store.append(records)?;
        Ok(())
    }
}

/// A failed turn, with the classification the protocol reports in `error.data`.
#[derive(Debug, thiserror::Error)]
#[error("turn {turn_id} failed ({class}): {source}")]
pub struct TurnError {
    pub class: String,
    pub transient: bool,
    pub usage_unknown: bool,
    pub turn_id: String,
    pub session_id: String,
    pub elapsed_ms: u64,
    pub trace: Option<theseus_protocol::Span>,
    /// What the turn's finished loops spent before it failed.
    pub usage: Usage,
    pub cost_usd: Option<f64>,
    pub tool_calls: u32,
    #[source]
    pub source: anyhow::Error,
}

pub fn add_usage(into: &mut Usage, u: &Usage) {
    into.input_tokens += u.input_tokens;
    into.output_tokens += u.output_tokens;
    into.cache_read_input_tokens += u.cache_read_input_tokens;
    into.cache_creation_input_tokens += u.cache_creation_input_tokens;
}
