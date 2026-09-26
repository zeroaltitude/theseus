//! The turn runner (spec §3.3, §3.3a). A turn is the sequence of loops run
//! under one acquisition of the session's turn lock, ended by the Advancer.
//! A loop: toolchain manager compiles the context and offers tools, one
//! provider call, the response comes back. M0: the context is the prompt and
//! nothing else, the tool list is empty, and the Advancer stops after one loop.
//! M2: the turn is a kernel turn (admission, per-execution lock, budget) and
//! the provider call is an action with a correlation id, settled by a
//! completion in the same frame as the execution's continuation (§3.16).

use std::sync::Arc;
use std::time::{Duration, Instant};

use anyhow::Result;
use serde_json::{json, Value};
use theseus_protocol::{
    notify, LoopEnded, LoopStarted, Message as Wire, ModelDelta, Notification, TurnStarted,
    TurnSubmitResult, Usage,
};
use tokio::sync::mpsc;

use crate::advancer::{Advancer, Decision, LoopOutcome};
use crate::hooks::{HookEvent, Hooks, Outcome};
use crate::ledger::LedgerRow;
use crate::provider::{ContentBlock, Message, Provider, ProviderError, ProviderRequest, ToolDef};
use crate::session::SessionRecord;
use crate::store::Store;
use crate::trace::Trace;
use crate::Config;
use theseus_kernel::{
    run_gate, Accepted, AllowAll, Authority, Completion, Execution, Kernel, KernelError,
    Outcome as ActionOutcome, Proposal, RetryClass, SessionKind as KSessionKind, TurnEnd,
    TurnGuard, Wake,
};

/// What the toolchain manager hands to the provider for one loop.
pub struct Compiled {
    pub system: Option<String>,
    pub messages: Vec<Message>,
    pub tools: Vec<ToolDef>,
}

/// M0 toolchain manager: compile the prompt, offer no tools.
pub struct ToolchainManager;

impl ToolchainManager {
    pub fn compile(&self, _cfg: &Config, target: &Target, input: &str) -> Compiled {
        Compiled {
            system: target.system.clone(),
            messages: vec![Message {
                role: "user".into(),
                content: input.to_string(),
            }],
            tools: Vec::new(),
        }
    }
}

/// What a turn runs against, resolved from a profile plus any raw overrides.
#[derive(Debug, Clone)]
pub struct Target {
    pub profile: String,
    pub provider: String,
    pub model: String,
    pub max_tokens: u32,
    pub system: Option<String>,
}

pub struct TurnRunner {
    pub cfg: Arc<Config>,
    /// Providers by name; `cfg.model.provider` is the default.
    pub providers: std::collections::BTreeMap<String, Arc<dyn Provider>>,
    pub hooks: Hooks,
    pub store: Store,
    /// The durable kernel: admission, the per-execution turn lock, budgets,
    /// and the action/completion record of every provider call.
    pub kernel: Arc<Kernel>,
    /// Woken whenever a turn ends or an execution changes, so waiters for
    /// admission retry without polling blindly.
    pub admission: Arc<tokio::sync::Notify>,
    pub advancer: Arc<dyn Advancer>,
    pub toolchain: ToolchainManager,
}

/// How long a turn may wait for admission before the client gets an error.
const ADMISSION_WAIT_MAX: Duration = Duration::from_secs(600);

fn kernel_kind(k: theseus_protocol::SessionKind) -> KSessionKind {
    match k {
        theseus_protocol::SessionKind::Conversation => KSessionKind::Conversation,
        theseus_protocol::SessionKind::Task => KSessionKind::Task,
    }
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
        Ok(Target {
            profile: name.to_string(),
            provider,
            model: model.unwrap_or(&prof.model).to_string(),
            max_tokens: prof.max_output_tokens,
            system: prof.system.clone(),
        })
    }

    /// Make sure the session has a kernel execution (sessions written before
    /// M2 have none) and return it.
    fn execution_for(&self, session: &mut SessionRecord, by: &str) -> Result<Execution> {
        if let Some(id) = &session.execution_id {
            if let Some(e) = self.kernel.execution(id)? {
                return Ok(e);
            }
        }
        let e = self.kernel.open_execution(
            &session.session_id,
            kernel_kind(session.kind),
            Authority {
                principal: by.to_string(),
                ..Default::default()
            },
            None,
            None,
        )?;
        session.execution_id = Some(e.id.clone());
        self.store.put_session(&session.session_id, session)?;
        Ok(e)
    }

    /// Take the kernel turn: wake the execution with this input, then wait for
    /// admission (ceiling, or another turn on the same execution). Terminal
    /// executions refuse with a classified error.
    async fn admit(&self, exec_id: &str, arrived: Instant) -> Result<TurnGuard> {
        loop {
            match self.kernel.admit(exec_id) {
                Ok(g) => return Ok(g),
                Err(e) => match e.downcast_ref::<KernelError>() {
                    Some(KernelError::AdmissionFull { .. })
                    | Some(KernelError::TurnHeld { .. })
                    | Some(KernelError::NotRunnable {
                        state: "running", ..
                    }) => {}
                    Some(KernelError::NotRunnable {
                        state: "waiting" | "blocked",
                        ..
                    }) => {
                        self.kernel.wake_input(exec_id)?;
                        continue;
                    }
                    Some(KernelError::NotRunnable { state, .. }) => {
                        return Err(TurnError {
                            class: format!("execution_{state}"),
                            transient: false,
                            usage_unknown: false,
                            turn_id: String::new(),
                            session_id: String::new(),
                            elapsed_ms: arrived.elapsed().as_millis() as u64,
                            trace: None,
                            source: e,
                        }
                        .into());
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

    pub async fn run(
        &self,
        mut session: SessionRecord,
        input: String,
        target: Target,
        events: mpsc::UnboundedSender<Wire>,
        by: &str,
    ) -> Result<TurnSubmitResult> {
        let arrived = Instant::now();
        let exec = self.execution_for(&mut session, by)?;
        // Input on the session makes its execution runnable (Waiting → Queued).
        if let Err(e) = self.kernel.wake_input(&exec.id) {
            if let Some(KernelError::NotRunnable { state, .. }) = e.downcast_ref::<KernelError>() {
                return Err(TurnError {
                    class: format!("execution_{state}"),
                    transient: false,
                    usage_unknown: false,
                    turn_id: String::new(),
                    session_id: session.session_id.clone(),
                    elapsed_ms: 0,
                    trace: None,
                    source: e,
                }
                .into());
            }
            return Err(e);
        }
        let guard = self.admit(&exec.id, arrived).await?;
        let admission_wait_us = arrived.elapsed().as_micros() as u64;
        let r = self
            .run_inner(
                &guard,
                session,
                input,
                target,
                events,
                arrived,
                admission_wait_us,
            )
            .await;
        // However the turn went, the kernel turn ends: results produced during
        // it are consumed, the execution parks on input, waiters are woken. A
        // terminal execution (cancelled, budget exhausted) keeps its state.
        let _ = self.kernel.take_results(&guard);
        if let Err(e) = self
            .kernel
            .end_turn(guard, TurnEnd::Wait { wake: Wake::Input })
        {
            tracing::warn!(error = %e, "end_turn failed");
        }
        self.admission.notify_waiters();
        r
    }

    #[allow(clippy::too_many_arguments)]
    async fn run_inner(
        &self,
        guard: &TurnGuard,
        mut session: SessionRecord,
        input: String,
        target: Target,
        events: mpsc::UnboundedSender<Wire>,
        arrived: Instant,
        lock_wait_us: u64,
    ) -> Result<TurnSubmitResult> {
        let provider = self
            .providers
            .get(&target.provider)
            .cloned()
            .ok_or_else(|| anyhow::anyhow!("unknown provider {:?}", target.provider))?;
        let model_id = target.model.clone();
        // The caller's copy may be stale: it was read before the lock. Re-read
        // under the lock so concurrent turns on one session never lose updates.
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
        let started = Instant::now();
        let sid = session.session_id.clone();
        let turn_id = crate::new_id("turn");
        let mut trace = Trace::start_at(
            arrived,
            "turn",
            "turn",
            json!({
                "turn_id": turn_id,
                "session_id": sid,
                "profile": target.profile,
                "provider": target.provider,
                "model": model_id,
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
        trace.record(
            "session.reread",
            "store",
            lock_wait_us,
            trace.now_us(),
            Value::Null,
        );

        let _ = events.send(Wire::Notification(Notification::new(
            notify::TURN_STARTED,
            TurnStarted {
                session_id: sid.clone(),
                turn_id: turn_id.clone(),
            },
        )));
        self.ledger(
            "turn.started",
            &sid,
            Some(&turn_id),
            json!({"input_chars": input.chars().count(), "profile": target.profile, "provider": target.provider, "model": model_id, "execution_id": guard.execution_id, "kernel_turn": guard.turn}),
        );

        if let Outcome::Blocked { reason } = self.site(
            &mut trace,
            HookEvent::TurnStarting,
            &sid,
            &turn_id,
            json!({}),
        ) {
            anyhow::bail!("turn blocked: {reason}");
        }
        let input = match self.site(
            &mut trace,
            HookEvent::InputReceived,
            &sid,
            &turn_id,
            json!({"input": input}),
        ) {
            Outcome::Proceed(v) => v
                .get("input")
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_string(),
            Outcome::Blocked { reason } => anyhow::bail!("input blocked: {reason}"),
            Outcome::Claimed { .. } => input,
        };

        let mut loop_index: u32 = 0;
        let mut output = String::new();
        let mut usage = Usage::default();
        let mut provider_stop: Option<String>;
        let mut model_used: String;
        let mut first_token_ms: Option<u64>;
        let mut request_id: Option<String>;
        let stop_reason: String;

        loop {
            trace.enter(
                &format!("loop {loop_index}"),
                "loop",
                json!({"loop": loop_index}),
            );
            // --- toolchain manager: compile context, offer tools
            let c0 = trace.now_us();
            let compiled = self.toolchain.compile(&self.cfg, &target, &input);
            let c1 = trace.now_us();
            trace.record(
                "compile",
                "compile",
                c0,
                c1,
                json!({"messages": compiled.messages.len(), "tools": compiled.tools.len(), "system": compiled.system.is_some()}),
            );
            self.site(
                &mut trace,
                HookEvent::ContextBuilt,
                &sid,
                &turn_id,
                json!({"loop": loop_index, "messages": compiled.messages.len(), "tools": compiled.tools.len()}),
            );
            let _ = events.send(Wire::Notification(Notification::new(
                notify::LOOP_STARTED,
                LoopStarted {
                    turn_id: turn_id.clone(),
                    loop_index,
                    model: model_id.clone(),
                    tools_offered: compiled.tools.len() as u32,
                },
            )));
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
                json!({"loop": loop_index, "profile": target.profile, "provider": target.provider, "model": model_id}),
            ) {
                anyhow::bail!("model call blocked: {reason}");
            }

            // --- the provider call is an action (§3.16): planned, authorized,
            // dispatched, each committed before the next step; the budget
            // reservation is taken in the plan frame.
            let mut proposal = Proposal {
                tool: "provider.messages".into(),
                args: json!({"provider": target.provider, "model": model_id, "max_tokens": target.max_tokens, "loop": loop_index, "turn_id": turn_id}),
                resource: Some(target.provider.clone()),
                policy_context: json!({"profile": target.profile}),
            };
            let (gate, gate_trace) = run_gate(&AllowAll, &mut proposal, &execution.authority);
            if let theseus_kernel::GateResult::Deny { reason } = gate {
                anyhow::bail!("provider call denied by policy: {reason}");
            }
            let input_estimate: u64 = compiled
                .messages
                .iter()
                .map(|m| m.content.chars().count() as u64 / 3)
                .sum::<u64>()
                + compiled
                    .system
                    .as_ref()
                    .map_or(0, |s| s.chars().count() as u64 / 3);
            let reserve = target.max_tokens as u64 + input_estimate;
            let o0 = trace.now_us();
            let action = match self.kernel.plan_action(
                guard,
                &proposal,
                RetryClass::NonRepeatable,
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
            let o1 = trace.now_us();
            trace.record(
                "action.outbox",
                "store",
                o0,
                o1,
                json!({"correlation_id": action.correlation_id, "tool": action.tool, "reserved_units": reserve, "gate": gate_trace}),
            );
            let call_started_ms = theseus_protocol::now_unix_ms();

            // --- one provider call, streamed
            let ev = events.clone();
            let tid = turn_id.clone();
            let mut on_delta = move |t: &str| {
                let _ = ev.send(Wire::Notification(Notification::new(
                    notify::MODEL_DELTA,
                    ModelDelta {
                        turn_id: tid.clone(),
                        loop_index,
                        text: t.to_string(),
                    },
                )));
            };
            let call_started = Instant::now();
            trace.enter(
                "provider.call",
                "provider",
                json!({"provider": target.provider, "model": model_id, "max_tokens": target.max_tokens}),
            );
            let call_t0 = trace.now_us();
            let resp = match provider
                .stream_message(
                    ProviderRequest {
                        model: &model_id,
                        max_tokens: target.max_tokens,
                        system: compiled.system.as_deref(),
                        messages: &compiled.messages,
                        tools: compiled.tools,
                    },
                    &mut on_delta,
                )
                .await
            {
                Ok(r) => r,
                Err(e) => {
                    // Classify, ledger, and fail the turn. No retry here: the
                    // reservation is held (usage may be unknown) and a human or a
                    // later policy decides. The provider is not what the loop
                    // waits on forever; every path out is a bounded timeout.
                    let pe = e.downcast_ref::<ProviderError>();
                    let (class, transient, unknown) = pe
                        .map(|p| (p.class(), p.is_transient(), p.usage_unknown()))
                        .unwrap_or(("unknown", false, true));
                    trace.exit(json!({"error": class, "message": e.to_string()}));
                    // Settle the action: unknown usage holds the reservation (§3.16).
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
                    });
                    let s1 = trace.now_us();
                    trace.record(
                        "action.settle",
                        "store",
                        s0,
                        s1,
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
                            "model": model_id,
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
                "usage": resp.usage,
                "stop_reason": resp.stop_reason,
                "blocks": resp.content.len(),
                "output_chars": resp.text.chars().count(),
                "rate_limit_tokens_remaining": resp.rate_limit.tokens_remaining,
            }));
            add_usage(&mut usage, &resp.usage);
            let s0 = trace.now_us();
            let settled = self.kernel.accept_completion(&Completion {
                correlation_id: action.correlation_id.clone(),
                outcome: ActionOutcome::Succeeded,
                result_ref: resp.request_id.clone(),
                external_op_id: resp.request_id.clone(),
                started_at_ms: call_started_ms,
                finished_at_ms: theseus_protocol::now_unix_ms(),
                producer: format!("provider:{}", target.provider),
                signature: None,
                usage_units: Some(resp.usage.input_tokens + resp.usage.output_tokens),
            })?;
            let s1 = trace.now_us();
            trace.record(
                "action.settle",
                "store",
                s0,
                s1,
                json!({"correlation_id": action.correlation_id, "outcome": "succeeded", "units": resp.usage.input_tokens + resp.usage.output_tokens, "settled": matches!(settled, Accepted::Settled { .. })}),
            );
            provider_stop = resp.stop_reason.clone();
            model_used = resp.model.clone();
            first_token_ms = resp.timing.first_token_ms;
            request_id = resp.request_id.clone();
            output.push_str(&resp.text);
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
                    "timing": resp.timing,
                    "rate_limit": resp.rate_limit,
                    "stop_reason": resp.stop_reason,
                    "blocks": resp.content.len(),
                }),
            );

            self.site(
                &mut trace,
                HookEvent::PostModelCall,
                &sid,
                &turn_id,
                json!({"loop": loop_index, "stop_reason": resp.stop_reason, "output_tokens": resp.usage.output_tokens}),
            );
            let tool_calls = resp.tool_calls();
            for tc in &tool_calls {
                // No tools are offered in M0, so this never fires; the site exists.
                if let ContentBlock::ToolUse { id, name, input } = tc {
                    self.site(
                        &mut trace,
                        HookEvent::ToolProposed,
                        &sid,
                        &turn_id,
                        json!({"id": id, "name": name, "input": input}),
                    );
                }
            }

            // --- the Advancer decides
            let outcome = LoopOutcome {
                loop_index,
                provider_stop_reason: resp.stop_reason.clone(),
                tool_calls: tool_calls.len() as u32,
                output_chars: resp.text.chars().count(),
            };
            let a0 = trace.now_us();
            let decision = self.advancer.decide(&outcome);
            let a1 = trace.now_us();
            trace.record(
                "advancer",
                "advancer",
                a0,
                a1,
                json!({"advancer": self.advancer.name(), "decision": decision.label()}),
            );
            self.site(
                &mut trace,
                HookEvent::AdvancerDecided,
                &sid,
                &turn_id,
                json!({"advancer": self.advancer.name(), "decision": decision}),
            );
            let _ = events.send(Wire::Notification(Notification::new(
                notify::LOOP_ENDED,
                LoopEnded {
                    turn_id: turn_id.clone(),
                    loop_index,
                    provider_stop_reason: resp.stop_reason.clone(),
                    tool_calls: outcome.tool_calls,
                    advancer: self.advancer.name().into(),
                    decision: decision.label(),
                },
            )));
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
                json!({"loop": loop_index, "outcome": outcome, "advancer": self.advancer.name(), "decision": decision, "usage": resp.usage}),
            );

            trace.exit(json!({"decision": decision.label(), "usage": resp.usage}));
            match decision {
                Decision::Continue => {
                    loop_index += 1;
                    continue;
                }
                Decision::EndTurn(reason) => {
                    stop_reason = reason;
                    break;
                }
            }
        }

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
        add_usage(&mut session.usage, &usage);
        let w0 = trace.now_us();
        self.store.put_session(&sid, &session)?;
        let w1 = trace.now_us();
        trace.record("session.write", "store", w0, w1, Value::Null);

        let mut result = TurnSubmitResult {
            session_id: sid.clone(),
            turn_id: turn_id.clone(),
            loops: loop_index + 1,
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
            json!({"loops": result.loops, "stop_reason": result.stop_reason, "usage": result.usage, "session_usage": session.usage, "elapsed_ms": result.elapsed_ms, "first_token_ms": result.first_token_ms, "provider": result.provider, "model": result.model}),
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
        let _ = events.send(Wire::Notification(Notification::new(
            notify::TURN_ENDED,
            &result,
        )));
        Ok(result)
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
