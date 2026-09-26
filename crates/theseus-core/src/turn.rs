//! The turn runner (spec §3.3, §3.3a). A turn is the sequence of loops run
//! under one acquisition of the session's turn lock, ended by the Advancer.
//! A loop: toolchain manager compiles the context and offers tools, one
//! provider call, the response comes back. M0: the context is the prompt and
//! nothing else, the tool list is empty, and the Advancer stops after one loop.

use std::sync::Arc;
use std::time::Instant;

use anyhow::{Context, Result};
use serde_json::{json, Value};
use theseus_protocol::{
    notify, LoopEnded, LoopStarted, ModelDelta, Notification, TurnStarted, TurnSubmitResult, Usage,
};
use tokio::sync::mpsc;

use crate::advancer::{Advancer, Decision, LoopOutcome};
use crate::hooks::{HookEvent, Hooks, Outcome};
use crate::ledger::LedgerRow;
use crate::provider::{Anthropic, Message, ToolDef};
use crate::session::{SessionRecord, TurnLocks};
use crate::store::Store;
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
    pub fn compile(&self, cfg: &Config, input: &str) -> Compiled {
        Compiled {
            system: cfg.model.system.clone(),
            messages: vec![Message {
                role: "user".into(),
                content: input.to_string(),
            }],
            tools: Vec::new(),
        }
    }
}

pub struct TurnRunner {
    pub cfg: Arc<Config>,
    pub provider: Anthropic,
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

    /// Visit a hook site and ledger the visit.
    fn site(&self, event: HookEvent, session: &str, turn: &str, payload: Value) -> Outcome {
        let (outcome, visit) = self
            .hooks
            .dispatch(event, Some(turn), Some(session), payload);
        self.ledger(
            "hook.site",
            session,
            Some(turn),
            serde_json::to_value(&visit).unwrap_or(Value::Null),
        );
        outcome
    }

    pub async fn run(
        &self,
        mut session: SessionRecord,
        input: String,
        events: mpsc::UnboundedSender<Notification>,
    ) -> Result<TurnSubmitResult> {
        let lock = self.locks.for_session(&session.session_id);
        let _held = lock.lock().await; // one turn per session
        let started = Instant::now();
        let sid = session.session_id.clone();
        let turn_id = crate::new_id("turn");

        let _ = events.send(Notification::new(
            notify::TURN_STARTED,
            TurnStarted {
                session_id: sid.clone(),
                turn_id: turn_id.clone(),
            },
        ));
        self.ledger(
            "turn.started",
            &sid,
            Some(&turn_id),
            json!({"input_chars": input.chars().count()}),
        );

        if let Outcome::Blocked { reason } =
            self.site(HookEvent::TurnStarting, &sid, &turn_id, json!({}))
        {
            anyhow::bail!("turn blocked: {reason}");
        }
        let input = match self.site(
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
        let stop_reason: String;

        loop {
            // --- toolchain manager: compile context, offer tools
            let compiled = self.toolchain.compile(&self.cfg, &input);
            self.site(
                HookEvent::ContextBuilt,
                &sid,
                &turn_id,
                json!({"loop": loop_index, "messages": compiled.messages.len(), "tools": compiled.tools.len()}),
            );
            let _ = events.send(Notification::new(
                notify::LOOP_STARTED,
                LoopStarted {
                    turn_id: turn_id.clone(),
                    loop_index,
                    model: self.cfg.model.model.clone(),
                    tools_offered: compiled.tools.len() as u32,
                },
            ));
            self.ledger(
                "loop.started",
                &sid,
                Some(&turn_id),
                json!({"loop": loop_index}),
            );

            if let Outcome::Blocked { reason } = self.site(
                HookEvent::PreModelCall,
                &sid,
                &turn_id,
                json!({"loop": loop_index, "model": self.cfg.model.model}),
            ) {
                anyhow::bail!("model call blocked: {reason}");
            }

            // --- one provider call, streamed
            let ev = events.clone();
            let tid = turn_id.clone();
            let resp = self
                .provider
                .stream_message(
                    &self.cfg.model.model,
                    self.cfg.model.max_tokens,
                    compiled.system.as_deref(),
                    &compiled.messages,
                    compiled.tools,
                    |t| {
                        let _ = ev.send(Notification::new(
                            notify::MODEL_DELTA,
                            ModelDelta {
                                turn_id: tid.clone(),
                                loop_index,
                                text: t.to_string(),
                            },
                        ));
                    },
                )
                .await
                .context("provider call")?;

            usage.input_tokens += resp.usage.input_tokens;
            usage.output_tokens += resp.usage.output_tokens;
            usage.cache_read_input_tokens += resp.usage.cache_read_input_tokens;
            usage.cache_creation_input_tokens += resp.usage.cache_creation_input_tokens;
            provider_stop = resp.stop_reason.clone();
            model_used = resp.model.clone();
            output.push_str(&resp.text);

            self.site(
                HookEvent::PostModelCall,
                &sid,
                &turn_id,
                json!({"loop": loop_index, "stop_reason": resp.stop_reason, "output_tokens": resp.usage.output_tokens}),
            );
            for tc in &resp.tool_calls {
                // No tools are offered in M0, so this never fires; the site exists.
                self.site(HookEvent::ToolProposed, &sid, &turn_id, tc.clone());
            }

            // --- the Advancer decides
            let outcome = LoopOutcome {
                loop_index,
                provider_stop_reason: resp.stop_reason.clone(),
                tool_calls: resp.tool_calls.len() as u32,
                output_chars: resp.text.chars().count(),
            };
            let decision = self.advancer.decide(&outcome);
            self.site(
                HookEvent::AdvancerDecided,
                &sid,
                &turn_id,
                json!({"advancer": self.advancer.name(), "decision": decision}),
            );
            let _ = events.send(Notification::new(
                notify::LOOP_ENDED,
                LoopEnded {
                    turn_id: turn_id.clone(),
                    loop_index,
                    provider_stop_reason: resp.stop_reason.clone(),
                    tool_calls: outcome.tool_calls,
                    advancer: self.advancer.name().into(),
                    decision: decision.label(),
                },
            ));
            self.site(
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
        self.site(HookEvent::ReplyClaim, &sid, &turn_id, json!({}));

        session.turns += 1;
        session.last_turn_id = Some(turn_id.clone());
        self.store.put_session(&sid, &session)?;

        let result = TurnSubmitResult {
            session_id: sid.clone(),
            turn_id: turn_id.clone(),
            loops: loop_index + 1,
            output,
            stop_reason,
            provider_stop_reason: provider_stop,
            model: model_used,
            usage,
            elapsed_ms: started.elapsed().as_millis() as u64,
        };
        self.site(
            HookEvent::TurnEnded,
            &sid,
            &turn_id,
            json!({"loops": result.loops, "stop_reason": result.stop_reason}),
        );
        self.ledger(
            "turn.ended",
            &sid,
            Some(&turn_id),
            json!({"loops": result.loops, "stop_reason": result.stop_reason, "usage": result.usage, "elapsed_ms": result.elapsed_ms}),
        );
        let _ = events.send(Notification::new(notify::TURN_ENDED, &result));
        Ok(result)
    }
}
