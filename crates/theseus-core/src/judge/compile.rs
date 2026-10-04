//! `continue.v1` at `compile` (M5 25b; design §2.4; spec §4.4a, step 3):
//! CONTINUE, in shadow. A compile that appended although one of its
//! candidate signals fired (`crate::signals`) is judged: append, or
//! recompile, and how? No signal, or a deterministic trigger, and nothing
//! is asked: most turns in a live thread cost nothing. Nothing acts on the
//! answer in M5 (the compaction and assembled strategies are M6's), and the
//! compiler's request is the same bytes with the judge on or off.
//!
//! The dispatch is on the turn's path, and costs a spawn: the judgment's id
//! is minted here, for the mark on the turn's trace, and everything else
//! (the budget's read, the last human message, the state's build and blob,
//! the reservation, the call) is the spawned task's.

use std::sync::{Arc, Weak};

use serde_json::{json, Value};
use theseus_judge::builders::{ContinueInput, SignalInput};
use theseus_judge::{Ask, DecisionPoint, Input, Judge, Mode, Outcome, Pack, Urgency};
use theseus_kernel::Kernel;

use super::{sampled, spend, JudgeService, Prepared, ScrubWith};
use crate::compiler::Compiled;
use crate::config::PackMode;
use crate::node::{Body, Origin};

/// CONTINUE (§2.4), at `compile`.
pub const CONTINUE_PACK: &str = "continue.v1";

/// Where the compile happened: its turn, and the kernel the budget is read
/// from (held weakly: a judgment never keeps the kernel alive).
pub struct AtCompile<'a> {
    pub session_id: &'a str,
    pub execution_id: &'a str,
    pub turn_id: &'a str,
    pub loop_index: u32,
    pub kernel: &'a Arc<Kernel>,
}

/// What the spawned task builds the state from.
struct Asked {
    session_id: String,
    execution_id: String,
    turn_id: String,
    loop_index: u32,
    kernel: Weak<Kernel>,
    input: ContinueInput,
    id: String,
}

impl JudgeService {
    /// A compile: one that appended and fired a signal is judged by
    /// `continue.v1` in shadow, in a task of its own. Returns the mark's
    /// attributes for the turn's trace when it dispatched one (`pack`,
    /// `point`, `mode`, and `judgment`, the id its row will carry), at once,
    /// whatever Jev does.
    pub fn at_compile(&self, compiled: &Compiled, at: AtCompile<'_>) -> Option<Value> {
        let signals = &compiled.signals;
        if compiled.new_compilation || signals.fired.is_empty() {
            return None;
        }
        if self.cfg.mode_of(CONTINUE_PACK, PackMode::Shadow) == PackMode::Off {
            return None;
        }
        let pack = theseus_judge::pack::by_name(CONTINUE_PACK)?;
        let key = format!("{}#{}", at.turn_id, at.loop_index);
        if !sampled(&key, self.cfg.sample_of(CONTINUE_PACK, pack.sample)) {
            return None;
        }
        let rt = tokio::runtime::Handle::try_current().ok()?;
        let c = &compiled.compilation;
        let input = ContinueInput {
            signals: signals
                .fired
                .iter()
                .map(|s| SignalInput {
                    name: s.name.clone(),
                    value: s.detail.clone(),
                })
                .collect(),
            window_tokens: signals.window,
            prefix_tokens: signals.prefix_tokens,
            tail_tokens: signals.tail_tokens,
            tail_nodes: compiled.tail_nodes as u64,
            last_cache_read_tokens: signals.last_cache_read,
            compilation_age_minutes: signals.compilation_age_minutes,
            strategy: c.strategy.clone(),
            trigger: c.trigger.clone(),
            // Read off the turn's path, with the last human message.
            budget_left_usd: 0.0,
            last_human_message: None,
        };
        let id = theseus_judge::new_id();
        let asked = Asked {
            session_id: at.session_id.into(),
            execution_id: at.execution_id.into(),
            turn_id: at.turn_id.into(),
            loop_index: at.loop_index,
            kernel: Arc::downgrade(at.kernel),
            input,
            id: id.clone(),
        };
        rt.spawn(judge_continue(self.me.clone(), pack.clone(), asked));
        Some(json!({
            "pack": CONTINUE_PACK, "point": pack.point, "mode": "shadow", "judgment": id,
        }))
    }

    /// The blocking half before the call: the budget left and the last
    /// human message read, the state built and its blob written, and the
    /// reservation. `None`: nothing to send.
    fn prepare_continue(&self, pack: Arc<Pack>, mut a: Asked, today: &str) -> Option<Prepared> {
        a.input.budget_left_usd = a
            .kernel
            .upgrade()
            .and_then(|k| k.execution(&a.execution_id).ok().flatten())
            .map_or(0.0, |e| {
                theseus_judge::price::micros_to_usd(e.budget.available())
            });
        a.input.last_human_message = self.last_human_message(&a.session_id);
        let scrub = ScrubWith(self.scrubber.clone());
        let state = theseus_judge::prepare(&pack, &Input::Continue(a.input), &scrub).ok()?;
        let blob = self
            .store
            .blobs()
            .put(state.state.json.as_bytes())
            .map_err(|e| tracing::warn!(error = %e, "judge: the state's blob was not written; not judged"))
            .ok()?;
        let built = self
            .built()
            .map_err(|e| tracing::warn!(error = %format!("{e:#}"), "judge: the Jev client was not built"))
            .ok()?;
        let context = json!({
            "session": a.session_id, "execution": a.execution_id, "turn": a.turn_id,
            "loop": a.loop_index, "baseline": "append", "decision": "append",
            "blob": blob, "on_path_ms": 0,
        });
        let mut ask = Ask::new(pack, &state, Mode::Shadow, context);
        ask.id = Some(a.id);
        let need = built
            .judge
            .inner()
            .reserve_micros(std::slice::from_ref(&ask))
            .unwrap_or(0);
        self.reserve(today, need)
            .then_some(Prepared { built, ask, need })
    }

    /// The newest message the operator wrote in the session.
    fn last_human_message(&self, session_id: &str) -> Option<String> {
        let nodes = self
            .store
            .session_nodes(session_id)
            .map_err(|e| tracing::warn!(error = %format!("{e:#}"), "judge: the session's nodes were not read"))
            .ok()?;
        nodes.into_iter().rev().find_map(|(_, n)| match n.body {
            Body::UserMessage { text, .. } if n.origin == Origin::Operator => Some(text),
            _ => None,
        })
    }
}

/// One `continue.v1` judgment, in its own task. The service is held only
/// around the blocking halves, never across the call.
async fn judge_continue(me: Weak<JudgeService>, pack: Arc<Pack>, asked: Asked) {
    let today = spend::local_day(theseus_protocol::now_unix_ms());
    let Some(svc) = me.upgrade() else { return };
    let day = today.clone();
    let prepared = tokio::task::spawn_blocking(move || svc.prepare_continue(pack, asked, &day))
        .await
        .ok()
        .flatten();
    let Some(Prepared { built, ask, need }) = prepared else {
        return;
    };
    let judgments = built
        .judge
        .judge(DecisionPoint {
            asks: vec![ask],
            urgency: Urgency::Shadow,
        })
        .await;
    let Some(svc) = me.upgrade() else { return };
    for j in &judgments {
        let (called, failed, unknown) = match &j.outcome {
            Outcome::Answered => (true, false, false),
            Outcome::Failed { usage_unknown, .. } => (true, true, *usage_unknown),
            Outcome::Skipped { .. } => (false, false, false),
        };
        // A call whose usage is unknown is booked at its reservation.
        let spent = j.cost_micros.unwrap_or(if unknown { need } else { 0 });
        svc.budget.settle(&today, need, spent, called, failed);
    }
}
