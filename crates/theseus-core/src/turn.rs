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
    run_gate, AllowAll, Authority, Completion, Execution, Kernel, KernelError,
    Outcome as ActionOutcome, Proposal, RetryClass, SessionKind as KSessionKind, TurnEnd,
    TurnGuard, Wake,
};
use theseus_protocol::{
    notify, LoopEnded, LoopStarted, ModelDelta, TurnStarted, TurnSubmitResult, Usage,
};
use theseus_store::NewRecord;

use crate::advancer::{Advancer, Decision, LoopOutcome, UntilNoToolCalls};
use crate::bus::{EventSink, SessionBus};
use crate::catalog::Catalog;
use crate::compiler::{compile, CompileInput, Compiled, Recompile, RequestSpec};
use crate::hooks::{HookEvent, Hooks, Outcome};
use crate::ledger::LedgerRow;
use crate::node::{Body, Node};
use crate::provider::{Delta, Provider, ProviderError};
use crate::session::{title_from, SessionRecord, TargetRef};
use crate::store::Store;
use crate::toolrun::{CallOutcome, ToolRuntime, TurnCtx};
use crate::trace::Trace;
use crate::Config;

/// The persona at the front of every system prompt. Frozen text: it sits at
/// the start of the cached prefix, so it never interpolates anything.
/// The tool name of the provider-call action (spec §3.2b).
pub const PROVIDER_TOOL: &str = "provider.messages";

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
    pub hooks: Hooks,
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

fn kernel_kind(k: theseus_protocol::SessionKind) -> KSessionKind {
    match k {
        theseus_protocol::SessionKind::Conversation => KSessionKind::Conversation,
        theseus_protocol::SessionKind::Task => KSessionKind::Task,
    }
}

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
        source,
    }
    .into()
}

impl TurnRunner {
    fn ledger(&self, kind: &str, session: &str, turn: Option<&str>, data: Value) {
        if let Err(e) = self
            .store
            .append_ledger(&LedgerRow::new(kind, Some(session), turn, data))
        {
            tracing::warn!(error = %e, "ledger append failed");
        }
    }

    /// Visit a hook site, ledger the visit, and record it as a trace span.
    fn site(
        &self,
        trace: &mut Trace,
        event: HookEvent,
        session: &str,
        turn: &str,
        payload: Value,
    ) -> Outcome {
        let t0 = trace.now_us();
        let (outcome, visit) = self
            .hooks
            .dispatch(event, Some(turn), Some(session), payload);
        let t1 = trace.now_us();
        trace.record(
            event.name(),
            "hook",
            t0,
            t1,
            json!({"kind": event.kind().as_str(), "handlers": visit.handlers, "outcome": visit.outcome}),
        );
        self.ledger(
            "hook.site",
            session,
            Some(turn),
            serde_json::to_value(&visit).unwrap_or(Value::Null),
        );
        outcome
    }

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
            kernel_kind(session.kind),
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

    pub async fn run(&self, req: TurnRequest) -> Result<TurnSubmitResult> {
        let arrived = Instant::now();
        let TurnRequest {
            mut session,
            input,
            target,
            sink,
            author,
            recompile,
        } = req;
        let continuation = input.is_none();
        let exec = self.execution_for(&mut session)?;
        if !continuation {
            if let Err(e) = self.kernel.wake_input(&exec.id) {
                if let Some(KernelError::NotRunnable { state, .. }) =
                    e.downcast_ref::<KernelError>()
                {
                    return Err(turn_error(
                        &format!("execution_{state}"),
                        &session.session_id,
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
        let failure_sink = sink.clone();
        let r = self
            .run_inner(
                &guard,
                session,
                input,
                target,
                sink,
                author,
                recompile,
                arrived,
                admission_wait_us,
            )
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

    #[allow(clippy::too_many_arguments)]
    async fn run_inner(
        &self,
        guard: &TurnGuard,
        mut session: SessionRecord,
        input: Option<String>,
        target: Target,
        sink: EventSink,
        author: String,
        recompile: Option<Recompile>,
        arrived: Instant,
        lock_wait_us: u64,
    ) -> Result<(TurnSubmitResult, TurnEnd, bool)> {
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
        let execution = self
            .kernel
            .execution(&guard.execution_id)?
            .ok_or_else(|| anyhow::anyhow!("execution vanished"))?;
        let authority = execution.authority.clone();
        let started = Instant::now();
        let sid = session.session_id.clone();
        let turn_id = crate::new_id("turn");
        let continuation = input.is_none();
        let confirm_ttl_ms = self.kernel.config().confirm_ttl_ms;
        let mut trace = Trace::start_at(
            arrived,
            "turn",
            "turn",
            json!({
                "turn_id": turn_id,
                "session_id": sid,
                "profile": target.profile,
                "provider": target.provider,
                "model": target.model,
                "continuation": continuation,
                "started_unix_ms": theseus_protocol::now_unix_ms(),
            }),
        );
        trace.record(
            "admission.wait",
            "lock",
            0,
            lock_wait_us,
            json!({"execution_id": guard.execution_id, "turn": guard.turn, "note": "kernel admission + per-execution turn lock"}),
        );
        sink.send(
            notify::TURN_STARTED,
            TurnStarted {
                session_id: sid.clone(),
                turn_id: turn_id.clone(),
                execution_id: Some(guard.execution_id.clone()),
                continuation,
            },
        );
        self.ledger(
            "turn.started",
            &sid,
            Some(&turn_id),
            json!({"input_chars": input.as_ref().map(|t| t.chars().count()), "profile": target.profile, "provider": target.provider, "model": target.model, "execution_id": guard.execution_id, "kernel_turn": guard.turn, "continuation": continuation, "author": author}),
        );
        if let Outcome::Blocked { reason } = self.site(
            &mut trace,
            HookEvent::TurnStarting,
            &sid,
            &turn_id,
            json!({"continuation": continuation}),
        ) {
            anyhow::bail!("turn blocked: {reason}");
        }
        let input = match input {
            None => None,
            Some(text) => Some(
                match self.site(
                    &mut trace,
                    HookEvent::InputReceived,
                    &sid,
                    &turn_id,
                    json!({"input": text}),
                ) {
                    Outcome::Proceed(v) => v
                        .get("input")
                        .and_then(Value::as_str)
                        .unwrap_or_default()
                        .to_string(),
                    Outcome::Blocked { reason } => anyhow::bail!("input blocked: {reason}"),
                    Outcome::Claimed { .. } => text,
                },
            ),
        };
        let ctx = |loop_index: Option<u32>| TurnCtx {
            kernel: &self.kernel,
            store: &self.store,
            guard,
            session_id: &sid,
            execution_id: &guard.execution_id,
            turn_id: &turn_id,
            loop_index,
            sink: &sink,
            authority: &authority,
            confirm_ttl_ms,
        };

        // 1. What happened while no turn was running.
        let t0 = trace.now_us();
        let settled = self.kernel.take_results(guard)?;
        let absorbed = self.tools.absorb(&ctx(None), &settled)?;
        let resumed = self.tools.resume(&ctx(None), input.is_some()).await?;
        trace.record(
            "continuation",
            "tool",
            t0,
            trace.now_us(),
            json!({"settled": settled.len(), "late_results": absorbed, "resumed": resumed.wrote, "awaiting": resumed.awaiting, "background": resumed.background}),
        );
        let mut background = resumed.background.clone();

        // 2. The new input.
        if let Some(text) = &input {
            let node = Node::user(&sid, Some(&turn_id), &author, text);
            if session.title.is_none() {
                session.title = Some(title_from(text));
            }
            self.store.append(vec![node.record()?])?;
            sink.send(
                notify::NODE_WRITTEN,
                json!({"session_id": sid, "node_id": node.id, "kind": node.kind_str()}),
            );
        }

        let mut loop_index: u32 = 0;
        let mut loops_run: u32 = 0;
        let mut output = String::new();
        let mut usage = Usage::default();
        let mut cost: Option<f64> = Some(0.0);
        let mut tool_calls: u32 = 0;
        let mut provider_stop: Option<String> = None;
        let mut stop_details: Option<Value> = None;
        let mut model_used = target.model.clone();
        let mut first_token_ms: Option<u64> = None;
        let mut request_id: Option<String> = None;
        let mut awaiting: Option<String> = resumed.awaiting.clone();
        let mut stop_reason = String::new();

        // 3. Does the model have anything new to read?
        let mut run_model = if awaiting.is_some() {
            stop_reason = "awaiting_confirm".into();
            false
        } else if input.is_some() || absorbed > 0 || resumed.wrote > 0 {
            true
        } else {
            let nodes = self.store.session_nodes(&sid)?;
            let last = nodes
                .iter()
                .rev()
                .find(|(_, n)| !matches!(n.body, Body::ToolCall { .. }));
            let awaiting_reply =
                matches!(last.map(|(_, n)| &n.body), Some(Body::UserMessage { .. }));
            if !awaiting_reply {
                stop_reason = "nothing_new".into();
            }
            awaiting_reply
        };

        let spec = self.request_spec(&target);
        let mut force = recompile.or(session.pending_recompile.take());
        let advancer = UntilNoToolCalls {
            max_loops: target.max_loops,
        };
        while run_model {
            loops_run += 1;
            trace.enter(
                &format!("loop {loop_index}"),
                "loop",
                json!({"loop": loop_index}),
            );
            // --- compile: append or recompile
            let c0 = trace.now_us();
            let nodes = self.store.session_nodes(&sid)?;
            let current = match session.compilation_id.as_deref() {
                Some(id) => self.store.get_compilation(id)?,
                None => None,
            };
            let compiled = compile(CompileInput {
                session_id: &sid,
                current: current.as_ref(),
                nodes: &nodes,
                last_position: self.store.last_position(),
                spec: &spec,
                catalog: &self.catalog,
                force: force.take(),
                window_override: None,
            });
            if compiled.new_compilation {
                self.persist_compilation(&compiled, &mut session, &turn_id)?;
            }
            let c1 = trace.now_us();
            let summary = json!({
                "session_id": sid,
                "turn_id": turn_id,
                "loop": loop_index,
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
            trace.record("compile", "compile", c0, c1, summary.clone());
            self.ledger("context.compiled", &sid, Some(&turn_id), summary.clone());
            sink.send(notify::CONTEXT_COMPILED, &summary);
            self.site(
                &mut trace,
                HookEvent::ContextBuilt,
                &sid,
                &turn_id,
                json!({"loop": loop_index, "messages": compiled.messages, "tools": spec.tools.len(), "decision": compiled.decision()}),
            );
            sink.send(
                notify::LOOP_STARTED,
                LoopStarted {
                    turn_id: turn_id.clone(),
                    loop_index,
                    model: target.model.clone(),
                    tools_offered: spec.tools.len() as u32,
                },
            );
            self.ledger(
                "loop.started",
                &sid,
                Some(&turn_id),
                json!({"loop": loop_index}),
            );
            if let Outcome::Blocked { reason } = self.site(
                &mut trace,
                HookEvent::PreModelCall,
                &sid,
                &turn_id,
                json!({"loop": loop_index, "profile": target.profile, "provider": target.provider, "model": target.model}),
            ) {
                anyhow::bail!("model call blocked: {reason}");
            }

            // --- the provider call is an action (§3.16)
            let mut proposal = Proposal {
                tool: PROVIDER_TOOL.into(),
                args: json!({"provider": target.provider, "model": target.model, "max_tokens": target.max_tokens, "loop": loop_index, "turn_id": turn_id, "digest": compiled.digest}),
                resource: Some(target.provider.clone()),
                policy_context: json!({"profile": target.profile}),
            };
            let (gate, gate_trace) = run_gate(&AllowAll, &mut proposal, &authority);
            if let theseus_kernel::GateResult::Deny { reason } = gate {
                anyhow::bail!("provider call denied by policy: {reason}");
            }
            let reserve = target.max_tokens as u64 + compiled.est_tokens;
            let o0 = trace.now_us();
            let action = match self.kernel.plan_action(
                guard,
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
                    let failed_trace = trace.finish(json!({"outcome": "failed", "class": class}));
                    self.ledger(
                        "turn.failed",
                        &sid,
                        Some(&turn_id),
                        json!({"loops": loop_index + 1, "reason": format!("{class}: {e}"), "usage_so_far": usage}),
                    );
                    return Err(TurnError {
                        class: class.into(),
                        transient: false,
                        usage_unknown: false,
                        turn_id: turn_id.clone(),
                        session_id: sid.clone(),
                        elapsed_ms: started.elapsed().as_millis() as u64,
                        trace: Some(failed_trace),
                        source: e,
                    }
                    .into());
                }
            };
            self.kernel
                .authorize(&action.correlation_id, &proposal, None)?;
            self.kernel.dispatch(&action.correlation_id, None)?;
            trace.record(
                "action.outbox",
                "store",
                o0,
                trace.now_us(),
                json!({"correlation_id": action.correlation_id, "tool": action.tool, "reserved_units": reserve, "gate": gate_trace}),
            );
            let call_started_ms = theseus_protocol::now_unix_ms();

            // --- one provider call, streamed
            let (sink2, tid) = (sink.clone(), turn_id.clone());
            let mut on_delta = move |d: Delta<'_>| match d {
                Delta::Text(t) => sink2.send(
                    notify::MODEL_DELTA,
                    ModelDelta {
                        turn_id: tid.clone(),
                        loop_index,
                        text: t.to_string(),
                    },
                ),
                Delta::Thinking(t) => sink2.send(
                    notify::MODEL_THINKING,
                    json!({"turn_id": tid, "loop_index": loop_index, "text": t}),
                ),
                Delta::ToolUseStart { .. } => {}
            };
            let call_started = Instant::now();
            trace.enter(
                "provider.call",
                "provider",
                json!({"provider": target.provider, "model": target.model, "max_tokens": target.max_tokens, "digest": compiled.digest}),
            );
            let call_t0 = trace.now_us();
            let resp = match provider
                .stream_message(&compiled.request, &mut on_delta)
                .await
            {
                Ok(r) => r,
                Err(e) => {
                    let pe = e.downcast_ref::<ProviderError>();
                    let (class, transient, unknown) = pe
                        .map(|p| (p.class(), p.is_transient(), p.usage_unknown()))
                        .unwrap_or(("unknown", false, true));
                    trace.exit(json!({"error": class, "message": e.to_string()}));
                    let s0 = trace.now_us();
                    let settled = self.kernel.accept_completion(&Completion {
                        correlation_id: action.correlation_id.clone(),
                        outcome: if unknown {
                            ActionOutcome::Unknown
                        } else {
                            ActionOutcome::Failed
                        },
                        result_ref: None,
                        external_op_id: None,
                        started_at_ms: call_started_ms,
                        finished_at_ms: theseus_protocol::now_unix_ms(),
                        producer: format!("provider:{}", target.provider),
                        signature: None,
                        usage_units: if unknown { None } else { Some(0) },
                        detail: Some(json!({"class": class})),
                    });
                    trace.record(
                        "action.settle",
                        "store",
                        s0,
                        trace.now_us(),
                        json!({"correlation_id": action.correlation_id, "outcome": if unknown {"unknown"} else {"failed"}, "result": settled.as_ref().map(|a| format!("{a:?}")).unwrap_or_else(|e| e.to_string())}),
                    );
                    let failed_trace = trace.finish(json!({"outcome": "failed", "class": class}));
                    self.ledger(
                        "provider.error",
                        &sid,
                        Some(&turn_id),
                        json!({
                            "loop": loop_index,
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
                    self.ledger(
                        "turn.failed",
                        &sid,
                        Some(&turn_id),
                        json!({"loops": loop_index + 1, "reason": format!("provider:{class}"), "usage_so_far": usage}),
                    );
                    session.turns += 1;
                    session.last_turn_id = Some(turn_id.clone());
                    session.last_active_ms = theseus_protocol::now_unix_ms();
                    add_usage(&mut session.usage, &usage);
                    let _ = self.store.put_session(&sid, &session);
                    return Err(TurnError {
                        class: class.to_string(),
                        transient,
                        usage_unknown: unknown,
                        turn_id: turn_id.clone(),
                        session_id: sid.clone(),
                        elapsed_ms: started.elapsed().as_millis() as u64,
                        trace: Some(failed_trace),
                        source: e,
                    }
                    .into());
                }
            };
            if let Some(fb) = resp.timing.first_byte_ms {
                trace.mark_at(call_t0 + fb * 1000, "first_byte", "mark", Value::Null);
            }
            if let Some(ft) = resp.timing.first_token_ms {
                trace.mark_at(call_t0 + ft * 1000, "first_token", "mark", Value::Null);
            }
            trace.exit(json!({
                "request_id": resp.request_id,
                "served_model": resp.model,
                "usage": resp.usage,
                "stop_reason": resp.stop_reason,
                "blocks": resp.content.len(),
                "output_chars": resp.text.chars().count(),
                "rate_limit_tokens_remaining": resp.rate_limit.tokens_remaining,
            }));
            let call_cost = self
                .catalog
                .cost_usd(&resp.model, &resp.usage)
                .or_else(|| self.catalog.cost_usd(&target.model, &resp.usage));
            cost = match (cost, call_cost) {
                (Some(a), Some(b)) => Some(a + b),
                _ => None,
            };
            let node = Node::assistant(
                &sid,
                &turn_id,
                loop_index,
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
            let s0 = trace.now_us();
            self.kernel.accept_completion_with(
                &Completion {
                    correlation_id: action.correlation_id.clone(),
                    outcome: ActionOutcome::Succeeded,
                    result_ref: Some(node.id.clone()),
                    external_op_id: resp.request_id.clone(),
                    started_at_ms: call_started_ms,
                    finished_at_ms: theseus_protocol::now_unix_ms(),
                    producer: format!("provider:{}", target.provider),
                    signature: None,
                    usage_units: Some(units),
                    detail: Some(
                        json!({"served_model": resp.model, "message_id": resp.message_id}),
                    ),
                },
                vec![node.record()?],
            )?;
            trace.record(
                "action.settle",
                "store",
                s0,
                trace.now_us(),
                json!({"correlation_id": action.correlation_id, "outcome": "succeeded", "units": units, "node_id": node.id}),
            );
            sink.send(
                notify::NODE_WRITTEN,
                json!({"session_id": sid, "node_id": node.id, "kind": node.kind_str()}),
            );
            add_usage(&mut usage, &resp.usage);
            provider_stop = resp.stop_reason.clone();
            stop_details = resp.stop_details.clone();
            model_used = resp.model.clone();
            first_token_ms = resp.timing.first_token_ms;
            request_id = resp.request_id.clone();
            if !resp.text.is_empty() {
                if !output.is_empty() {
                    output.push_str("\n\n");
                }
                output.push_str(&resp.text);
            }
            self.ledger(
                "provider.call",
                &sid,
                Some(&turn_id),
                json!({
                    "loop": loop_index,
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
                self.ledger(
                    "provider.refusal",
                    &sid,
                    Some(&turn_id),
                    json!({"stop_details": resp.stop_details, "model": resp.model}),
                );
            }
            self.site(
                &mut trace,
                HookEvent::PostModelCall,
                &sid,
                &turn_id,
                json!({"loop": loop_index, "stop_reason": resp.stop_reason, "output_tokens": resp.usage.output_tokens}),
            );

            // --- tools
            let uses = resp.tool_uses();
            let stop = resp.stop_reason.as_deref();
            let mut answered = 0u32;
            if !uses.is_empty() {
                if stop == Some("tool_use") {
                    for u in &uses {
                        tool_calls += 1;
                        self.site(
                            &mut trace,
                            HookEvent::ToolProposed,
                            &sid,
                            &turn_id,
                            json!({"id": u.id, "name": u.name, "input": u.input}),
                        );
                        let t0 = trace.now_us();
                        let invalid = resp.invalid_tool_inputs.get(&u.id).map(String::as_str);
                        let outcome = self
                            .tools
                            .process(&ctx(Some(loop_index)), &node.id, u, invalid)
                            .await?;
                        trace.record(
                            &format!("tool {}", u.name),
                            "tool",
                            t0,
                            trace.now_us(),
                            json!({"tool_use_id": u.id, "outcome": format!("{outcome:?}")}),
                        );
                        match outcome {
                            CallOutcome::AwaitingConfirm { correlation_id } => {
                                awaiting = Some(correlation_id);
                                break;
                            }
                            CallOutcome::Background { correlation_id } => {
                                background.push(correlation_id);
                                answered += 1;
                            }
                            CallOutcome::Done { .. } => answered += 1,
                        }
                    }
                } else {
                    let why = format!(
                        "the response ended with stop reason `{}`",
                        stop.unwrap_or("none")
                    );
                    for u in &uses {
                        self.tools.not_run(&ctx(Some(loop_index)), u, &why)?;
                    }
                }
            }

            // --- the Advancer decides
            let outcome = LoopOutcome {
                loop_index,
                provider_stop_reason: resp.stop_reason.clone(),
                tool_calls: if stop == Some("tool_use") {
                    answered
                } else {
                    0
                },
                output_chars: resp.text.chars().count(),
            };
            let a0 = trace.now_us();
            let decision = if awaiting.is_some() {
                Decision::EndTurn("awaiting_confirm".into())
            } else if matches!(stop, Some("refusal") | Some("max_tokens")) {
                Decision::EndTurn(stop.unwrap_or_default().to_string())
            } else {
                advancer.decide(&outcome)
            };
            trace.record(
                "advancer",
                "advancer",
                a0,
                trace.now_us(),
                json!({"advancer": advancer.name(), "decision": decision.label()}),
            );
            self.site(
                &mut trace,
                HookEvent::AdvancerDecided,
                &sid,
                &turn_id,
                json!({"advancer": advancer.name(), "decision": decision}),
            );
            sink.send(
                notify::LOOP_ENDED,
                LoopEnded {
                    turn_id: turn_id.clone(),
                    loop_index,
                    provider_stop_reason: resp.stop_reason.clone(),
                    tool_calls: uses.len() as u32,
                    advancer: advancer.name().into(),
                    decision: decision.label(),
                },
            );
            self.site(
                &mut trace,
                HookEvent::LoopEnded,
                &sid,
                &turn_id,
                json!({"loop": loop_index}),
            );
            self.ledger(
                "loop.ended",
                &sid,
                Some(&turn_id),
                json!({"loop": loop_index, "outcome": outcome, "advancer": advancer.name(), "decision": decision, "usage": resp.usage}),
            );
            trace.exit(json!({"decision": decision.label(), "usage": resp.usage}));
            match decision {
                Decision::Continue => loop_index += 1,
                Decision::EndTurn(reason) => {
                    stop_reason = reason;
                    run_model = false;
                }
            }
        }

        // 4. Results that settled while this turn ran.
        let settled_late = self.kernel.take_results(guard)?;
        let late = self.tools.absorb(&ctx(None), &settled_late)?;

        let output = match self.site(
            &mut trace,
            HookEvent::MessageSending,
            &sid,
            &turn_id,
            json!({"output": output}),
        ) {
            Outcome::Proceed(v) => v
                .get("output")
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_string(),
            _ => output,
        };
        self.site(&mut trace, HookEvent::ReplyClaim, &sid, &turn_id, json!({}));

        session.turns += 1;
        session.last_turn_id = Some(turn_id.clone());
        session.last_active_ms = theseus_protocol::now_unix_ms();
        session.tool_calls += tool_calls as u64;
        session.cost_usd += cost.unwrap_or(0.0);
        session.last_target = Some(TargetRef {
            profile: target.profile.clone(),
            provider: target.provider.clone(),
            model: target.model.clone(),
        });
        add_usage(&mut session.usage, &usage);
        let w0 = trace.now_us();
        self.store.put_session(&sid, &session)?;
        trace.record("session.write", "store", w0, trace.now_us(), Value::Null);

        if stop_reason.is_empty() {
            stop_reason = "end_turn".into();
        }
        let mut result = TurnSubmitResult {
            session_id: sid.clone(),
            turn_id: turn_id.clone(),
            loops: loops_run,
            output,
            stop_reason,
            provider_stop_reason: provider_stop,
            model: model_used,
            provider: target.provider.clone(),
            profile: target.profile.clone(),
            usage,
            elapsed_ms: started.elapsed().as_millis() as u64,
            first_token_ms,
            request_id,
            trace: None,
            execution_id: Some(guard.execution_id.clone()),
            cost_usd: if loops_run == 0 { Some(0.0) } else { cost },
            tool_calls,
            awaiting_confirm: awaiting.clone(),
            stop_details,
            continuation,
        };
        self.site(
            &mut trace,
            HookEvent::TurnEnded,
            &sid,
            &turn_id,
            json!({"loops": result.loops, "stop_reason": result.stop_reason}),
        );
        self.ledger(
            "turn.ended",
            &sid,
            Some(&turn_id),
            json!({"loops": result.loops, "stop_reason": result.stop_reason, "usage": result.usage, "cost_usd": result.cost_usd, "tool_calls": tool_calls, "session_usage": session.usage, "elapsed_ms": result.elapsed_ms, "first_token_ms": result.first_token_ms, "provider": result.provider, "model": result.model, "awaiting_confirm": awaiting, "continuation": continuation, "late_results": late}),
        );
        result.trace = Some(trace.finish(json!({
            "outcome": "complete",
            "loops": result.loops,
            "stop_reason": result.stop_reason,
            "usage": result.usage,
        })));
        self.ledger(
            "turn.trace",
            &sid,
            Some(&turn_id),
            serde_json::to_value(&result.trace).unwrap_or(Value::Null),
        );
        sink.send(notify::TURN_ENDED, &result);

        // Park: on the confirm, on outstanding jobs, or on the next input.
        let outstanding: Vec<String> = self
            .kernel
            .execution(&guard.execution_id)?
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
        let end = if let Some(c) = awaiting {
            TurnEnd::Wait {
                wake: Wake::Confirm { confirm_id: c },
            }
        } else if !outstanding.is_empty() {
            TurnEnd::Wait {
                wake: Wake::Actions {
                    correlation_ids: outstanding,
                },
            }
        } else {
            TurnEnd::Wait { wake: Wake::Input }
        };
        Ok((result, end, late > 0))
    }

    /// A new compilation: its record, the `derived_from` edge, and the
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
        if let Some(prev) = &c.derived_from {
            let edge =
                json!({"type": "derived_from", "from": c.id, "to": prev, "at_ms": c.created_at_ms});
            records.push(
                NewRecord::json(
                    theseus_store::kinds::EDGE,
                    Some(&format!("derived_from|{}|{}", c.id, prev)),
                    &edge,
                )?
                .scoped(&c.session_id),
            );
        }
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
    #[source]
    pub source: anyhow::Error,
}

pub fn add_usage(into: &mut Usage, u: &Usage) {
    into.input_tokens += u.input_tokens;
    into.output_tokens += u.output_tokens;
    into.cache_read_input_tokens += u.cache_read_input_tokens;
    into.cache_creation_input_tokens += u.cache_creation_input_tokens;
}
