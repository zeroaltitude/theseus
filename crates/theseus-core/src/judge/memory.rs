//! `memory.v1` and `attribution.v1` (M6 step 31a; design m6 §2.6), in
//! shadow, at the memory pass: after a turn ends, off its path. The pass
//! hands each node it labels to `memory.v1` and each recall whose turn has
//! its reply to `attribution.v1` ([`JudgeService::at_memory_pass`]), which
//! returns at once: the state's build, its blob, the reservation from the
//! shadow budget, the call, and the record are the spawned task's, as
//! `loop.v1`'s are. With `[judge]` off, or a pack's line `off`, nothing is
//! asked, and the deterministic labels and attribution stand alone. Their
//! rows are the sink's `judge.call` rows, scoped `judge:memory` and
//! `judge:attribution`; no turn trace carries them, since no turn waits.

use std::sync::{Arc, Weak};

use serde_json::json;
use theseus_judge::builders::{AttributionInput, MemoryInput};
use theseus_judge::{Ask, DecisionPoint, Input, Judge, Outcome, Pack, Urgency};

use super::{sampled, spend, JudgeService, Prepared, ScrubWith};

/// What a node is, as memory keeps it.
pub const MEMORY_PACK: &str = "memory.v1";
/// Whether the reply relied on each recalled note.
pub const ATTRIBUTION_PACK: &str = "attribution.v1";

/// One question the memory pass asks in shadow.
#[derive(Debug, Clone)]
pub enum MemoryAsk {
    /// A node it labeled.
    Node {
        session_id: String,
        turn_id: Option<String>,
        node_id: String,
        input: MemoryInput,
    },
    /// A recall whose turn has its reply.
    Recall {
        session_id: String,
        turn_id: Option<String>,
        recall_id: String,
        input: AttributionInput,
    },
}

impl MemoryAsk {
    fn pack(&self) -> &'static str {
        match self {
            MemoryAsk::Node { .. } => MEMORY_PACK,
            MemoryAsk::Recall { .. } => ATTRIBUTION_PACK,
        }
    }

    /// What the sample is drawn by: the node, or the recall.
    fn key(&self) -> &str {
        match self {
            MemoryAsk::Node { node_id, .. } => node_id,
            MemoryAsk::Recall { recall_id, .. } => recall_id,
        }
    }
}

impl JudgeService {
    /// The memory pass's question, in shadow, in a task of its own; sampled
    /// by the node or the recall. Returns at once, whatever Jev does.
    pub fn at_memory_pass(&self, ask: MemoryAsk) {
        let name = ask.pack();
        if !self.pack_on(name) {
            return;
        }
        let Some(pack) = theseus_judge::pack::by_name(name) else {
            return;
        };
        if !sampled(ask.key(), self.cfg.sample_of(name, pack.sample)) {
            return;
        }
        let Ok(rt) = tokio::runtime::Handle::try_current() else {
            return;
        };
        rt.spawn(judge_memory(self.me.clone(), pack, ask));
    }

    /// The blocking half: the state, its blob, and the reservation.
    fn prepare_memory(&self, pack: Arc<Pack>, ask: &MemoryAsk) -> Option<Prepared> {
        let (input, context) = match ask {
            MemoryAsk::Node {
                session_id,
                turn_id,
                node_id,
                input,
            } => (
                Input::Memory(input.clone()),
                json!({"session": session_id, "turn": turn_id, "node": node_id,
                       "baseline": "rules", "class": "memory", "on_path_ms": 0}),
            ),
            MemoryAsk::Recall {
                session_id,
                turn_id,
                recall_id,
                input,
            } => (
                Input::Attribution(input.clone()),
                json!({"session": session_id, "turn": turn_id, "recall": recall_id,
                       "notes": input.notes.iter().map(|n| &n.id).collect::<Vec<_>>(),
                       "baseline": "rules", "class": "memory", "on_path_ms": 0}),
            ),
        };
        let scrub = ScrubWith(self.scrubber.clone());
        let state = theseus_judge::prepare(&pack, &input, &scrub).ok()?;
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
        let mut context = context;
        context["blob"] = json!(blob);
        let mode = self.ask_mode(&pack.name(), &mut context);
        let ask = Ask::new(pack, &state, mode, context);
        let need = built
            .judge
            .inner()
            .reserve_micros(std::slice::from_ref(&ask))
            .unwrap_or(0);
        Some(Prepared { built, ask, need })
    }
}

/// One memory-pass judgment, in its own task; the service is held only
/// around the blocking half, never across the call.
async fn judge_memory(me: Weak<JudgeService>, pack: Arc<Pack>, ask: MemoryAsk) {
    let today = spend::local_day(theseus_protocol::now_unix_ms());
    let Some(svc) = me.upgrade() else { return };
    let prepared = tokio::task::spawn_blocking(move || svc.prepare_memory(pack, &ask))
        .await
        .ok()
        .flatten();
    let Some(Prepared { built, ask, need }) = prepared else {
        return;
    };
    if !super::reserve_between(&me, &today, need).await {
        return;
    }
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
        let spent = j.cost_micros.unwrap_or(if unknown { need } else { 0 });
        svc.budget.settle(&today, need, spent, called, failed);
    }
}
