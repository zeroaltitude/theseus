//! The turn runner (spec §3.3, §3.3a). A turn is the sequence of loops run
//! under one acquisition of the session's turn lock, ended by the Advancer.
//! A loop: toolchain manager compiles the context and offers tools, one
//! provider call, the response comes back. M0: the context is the prompt and
//! nothing else, the tool list is empty, and the Advancer stops after one loop.

use std::sync::Arc;
use std::time::Instant;

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
use crate::session::{SessionRecord, TurnLocks};
use crate::store::Store;
use crate::trace::Trace;
use crate::Config;

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
    pub locks: TurnLocks,
    pub advancer: Arc<dyn Advancer>,
    pub toolchain: ToolchainManager,
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

    pub async fn run(
        &self,
        mut session: SessionRecord,
        input: String,
        target: Target,
        events: mpsc::UnboundedSender<Wire>,
    ) -> Result<TurnSubmitResult> {
        let provider = self
            .providers
            .get(&target.provider)
            .cloned()
            .ok_or_else(|| anyhow::anyhow!("unknown provider {:?}", target.provider))?;
        let model_id = target.model.clone();
        let arrived = Instant::now();
        let lock = self.locks.for_session(&session.session_id);
        let _held = lock.lock().await; // one turn per session
        let lock_wait_us = arrived.elapsed().as_micros() as u64;
        // The caller's copy may be stale: it was read before the lock. Re-read
        // under the lock so concurrent turns on one session never lose updates.
        if let Some(fresh) = self
            .store
            .get_session::<SessionRecord>(&session.session_id)?
        {
            session = fresh;
        }
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
            "lock.wait",
            "lock",
            0,
            lock_wait_us,
            json!({"note": "session turn lock"}),
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
            json!({"input_chars": input.chars().count(), "profile": target.profile, "provider": target.provider, "model": model_id}),
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
