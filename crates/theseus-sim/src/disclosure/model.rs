//! The sim's model: a provider that does what the session's plan says, one
//! call a loop, and repeats in every answer each atom its request carried. A
//! model that tells everything it was shown is the worst case for disclosure:
//! whatever a compile admits reaches its words, its stream, its post, and the
//! briefs it writes.
//!
//! Before it answers, the model may change who can view a channel, as a
//! member joining or a role changing while the turn runs would. The binding
//! hears of it at once or not at all, which is the window 19c's held post and
//! quiet loops close.

use std::sync::{Arc, Mutex, OnceLock, Weak};

use anyhow::{anyhow, Result};
use rand::Rng;
use serde_json::{json, Value};
use theseus_core::provider::{
    CallTiming, Delta, DeltaSink, ModelResponse, Provider, ProviderFuture, ProviderRequest,
};
use theseus_core::Core;
use theseus_protocol::Usage;

use super::atoms::{marker, scan};
use super::world::{Act, Call, Shared};

/// How many characters a streamed delta carries: an atom's marker may be cut
/// across two, as a real stream cuts words.
const CHUNK: usize = 48;

pub struct Leaky {
    pub shared: Arc<Mutex<Shared>>,
    /// The core, to tell it who views a channel when the binding hears of a
    /// change mid-turn; set once the core is built.
    pub core: OnceLock<Weak<Core>>,
}

impl Provider for Leaky {
    fn name(&self) -> &str {
        "sim"
    }

    fn stream_message<'a>(
        &'a self,
        req: &'a ProviderRequest,
        on_delta: DeltaSink<'a>,
    ) -> ProviderFuture<'a> {
        Box::pin(async move {
            let blocks = self.answer(req)?;
            let pushes =
                std::mem::take(&mut self.shared.lock().map_err(|_| anyhow!("poisoned"))?.pushes);
            if let Some(core) = self.core.get().and_then(Weak::upgrade) {
                super::push_all(&core, &pushes);
            }
            for b in &blocks {
                match b["type"].as_str() {
                    Some("text") => {
                        let t: Vec<char> = b["text"].as_str().unwrap_or("").chars().collect();
                        for piece in t.chunks(CHUNK) {
                            let s: String = piece.iter().collect();
                            on_delta(Delta::Text(&s));
                        }
                    }
                    Some("tool_use") => on_delta(Delta::ToolUseStart {
                        id: b["id"].as_str().unwrap_or(""),
                        name: b["name"].as_str().unwrap_or(""),
                    }),
                    _ => {}
                }
            }
            let stop = if blocks.iter().any(|b| b["type"] == "tool_use") {
                "tool_use"
            } else {
                "end_turn"
            };
            let text = theseus_core::provider::text_of(&blocks);
            Ok(ModelResponse {
                usage: Usage {
                    input_tokens: req.estimate_tokens(),
                    output_tokens: (text.len() as u64 / 4).max(1),
                    ..Default::default()
                },
                text,
                content: blocks,
                stop_reason: Some(stop.into()),
                model: req.model.clone(),
                message_id: Some("msg_sim".into()),
                request_id: Some("req_sim".into()),
                timing: CallTiming {
                    first_byte_ms: Some(1),
                    first_token_ms: Some(1),
                    total_ms: 1,
                },
                ..Default::default()
            })
        })
    }
}

impl Leaky {
    /// Record the request, maybe change a channel mid-turn, and answer by the
    /// session's plan.
    fn answer(&self, req: &ProviderRequest) -> Result<Vec<Value>> {
        let pos = self
            .core
            .get()
            .and_then(Weak::upgrade)
            .map_or(0, |c| c.store.last_position());
        let mut s = self
            .shared
            .lock()
            .map_err(|_| anyhow!("the sim's state is poisoned"))?;
        let session = s
            .current
            .clone()
            .ok_or_else(|| anyhow!("the model was called while no turn of the sim's runs"))?;
        let system = serde_json::to_string(&req.system)?;
        let text = serde_json::to_string(&req.messages)?;
        let seen = scan(&format!("{system}\n{text}"));
        s.calls.push(Call {
            session: session.clone(),
            digest: req.digest(),
            system,
            text,
            messages: req.messages.clone(),
            pos,
        });
        let p = s.p_mid_turn;
        if s.rng.random_bool(p) {
            mid_turn(&mut s);
        }
        let act = s
            .plans
            .get_mut(&session)
            .and_then(|q| q.pop_front())
            .unwrap_or(Act::Say);
        let echo: Vec<String> = seen.iter().map(|id| marker(*id)).collect();
        let said = if echo.is_empty() {
            "Nothing to report.".to_string()
        } else {
            format!("I read {}.", echo.join(" "))
        };
        Ok(blocks(&mut s, act, said))
    }
}

/// A member joins or leaves a channel while the turn runs, the turn's own
/// channel half the time: the binding hears of it (and reads every channel)
/// half the time.
fn mid_turn(s: &mut Shared) {
    if s.channels.is_empty() {
        return;
    }
    let own = s
        .turn_channel
        .and_then(|c| s.channels.iter().position(|x| x.id == c));
    let i = match own {
        Some(i) if s.rng.random_bool(0.5) => i,
        _ => s.rng.random_range(0..s.channels.len()),
    };
    s.change_viewers(i);
    s.mid_turn_changes += 1;
    if s.rng.random_bool(0.5) {
        s.mid_turn_pushed += 1;
        let pushes = s.read_all();
        s.pushes.extend(pushes);
    }
}

/// The answer's blocks: what the model says, and the act's call.
fn blocks(s: &mut Shared, act: Act, said: String) -> Vec<Value> {
    let (name, input) = match act {
        Act::Say => return vec![json!({"type": "text", "text": said})],
        Act::ReadPrivate(k) | Act::ReadOpen(k) => {
            let open = matches!(act, Act::ReadOpen(_));
            let path = s
                .files
                .iter()
                .filter(|f| f.public == open)
                .nth(k)
                .map(|f| f.path.clone())
                .unwrap_or_default();
            ("fs_read", json!({ "path": path }))
        }
        Act::Fetch => {
            s.fetches += 1;
            let n = s.fetches;
            (
                "http_fetch",
                json!({ "url": format!("https://pages.example.invalid/page-{n}") }),
            )
        }
        Act::RunEgress => ("proc_run", json!({"argv": ["sim-job"], "egress": true})),
        Act::RunLocal => ("proc_run", json!({"argv": ["sim-job"], "egress": false})),
        Act::Task { wake } => (
            "task_create",
            // Enough for a model call's worst case, so the task never asks
            // about its budget.
            json!({"brief": format!("Look further. {said}"), "wake_parent": wake, "budget_usd": 1000.0}),
        ),
    };
    s.tool_calls += 1;
    let id = s.call_id();
    vec![
        json!({"type": "text", "text": format!("On it. {said}")}),
        json!({"type": "tool_use", "id": id, "name": name, "input": input}),
    ]
}
