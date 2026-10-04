//! The `+rerank` arm in shadow (M6 step 32c; design `m6-memory.md` §2.7):
//! after a turn's recall has run its pipeline, `rerank.v1` asks Jev, of each
//! of the top 20 candidates that passed every filter, whether it holds
//! information that would help answer the message, and the row records what
//! `+rerank` would admit beside what `baseline` admitted.
//!
//! - **Off the turn's path.** [`JudgeService::at_recall`] picks the eligible
//!   candidates (pure, a few microseconds), mints the judgment's id, marks
//!   the turn's trace, and spawns the rest: the state, its blob, the
//!   reservation, the call, the reorder, and the record. A slow, failing, or
//!   rate-limited Jev changes no turn's request bytes, duration, or frames.
//! - **Only what passed the filters reaches Jev** (the place rule first:
//!   `theseus_memory::rerank::eligible`), and the builder scrubs the state
//!   as it scrubs every state.
//! - **Its own deadline.** The call goes as a live one would, under
//!   [`DEADLINE`] (600 ms): any failure (a timeout, the breaker open, no key,
//!   a shed call, model drift) falls back to the fused order, and the row's
//!   `rerank.fallback` says which. A rerank the day's limit stops is never
//!   sent and writes no row, as every shadow judgment at the limit (the
//!   budget's `judge.paused` row and its skipped count say it).
//! - **Paid by the judge's day budget**, as every shadow judgment: its row's
//!   context says `purpose: "recall"`.
//!
//! 30b's live arm calls the same pure halves before its pack:
//! `eligible`, `reorder` over the answers' probabilities ([`probabilities`]),
//! and `repack`, with the fused order on any fallback.

use std::collections::{BTreeMap, BTreeSet};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Weak};
use std::time::{Duration, Instant};

use serde_json::{json, Value};
use theseus_judge::builders::{RerankInput, RerankNote, RERANK_NOTES};
use theseus_judge::{
    Ask, DecisionPoint, Input, Judge, Judgment, JudgmentSink, Mode, Outcome, Pack, Urgency,
};
use theseus_memory::recall::{Asker, Candidate, Link, Params, Place};
use theseus_memory::rerank::{eligible, reorder, repack};
use theseus_memory::MemoryScience;

use super::{sampled, spend, JudgeService, ScrubWith};
use crate::config::PackMode;
use crate::trace::Trace;

/// Recall's `+rerank` arm (M6 32c).
pub const RERANK_PACK: &str = "rerank.v1";

/// The rerank's own deadline (§2.7: Jev takes about 350 ms).
pub const DEADLINE: Duration = Duration::from_millis(600);

/// The most of the message the state holds, in characters.
const MESSAGE_CHARS: usize = 2000;

/// The rerank's deadline as the service holds it (tests lengthen it).
pub(super) struct RerankDeadline(AtomicU64);

impl Default for RerankDeadline {
    fn default() -> Self {
        Self(AtomicU64::new(DEADLINE.as_millis() as u64))
    }
}

/// What a recall's pipeline read, handed to the rerank after it.
pub struct Recalled {
    pub recall_id: String,
    pub session_id: String,
    pub turn_id: String,
    /// The recall's query: the new message, and the start of the reply
    /// before it.
    pub message: String,
    pub place: Place,
    pub in_context: BTreeSet<String>,
    /// The nodes the operator labeled wrong or stale (30b): as the
    /// recall's pipeline, the rerank drops them.
    pub labeled: BTreeSet<String>,
    pub candidates: Vec<Candidate>,
    /// The memory pass's links among them (31a): the repack prefers the
    /// newer node, as the recall did.
    pub links: Vec<Link>,
    pub params: Params,
    pub science: Arc<dyn MemoryScience>,
    /// What the fused pack admitted, by key (`<node>#<chunk>`).
    pub admitted: Vec<String>,
    pub now_ms: u64,
}

impl Recalled {
    fn asker(&self) -> Asker<'_> {
        Asker {
            session_id: &self.session_id,
            place: &self.place,
            in_context: &self.in_context,
            labeled: &self.labeled,
            links: &self.links,
            now_ms: self.now_ms,
        }
    }
}

/// A rerank on its way: the recall, and its eligible notes in the fused
/// order.
struct Dispatched {
    recalled: Recalled,
    eligible: Vec<Candidate>,
    id: String,
}

impl JudgeService {
    /// The rerank's deadline.
    pub fn rerank_deadline(&self) -> Duration {
        Duration::from_millis(self.rerank_deadline.0.load(Ordering::Relaxed))
    }

    /// A longer deadline (tests: a rerank awaited in the turn would show).
    #[cfg(test)]
    pub(crate) fn set_rerank_deadline(&self, d: Duration) {
        self.rerank_deadline
            .0
            .store(d.as_millis() as u64, Ordering::Relaxed);
    }

    /// A turn's recall ran: `rerank.v1` judges its top 20 in shadow, in a
    /// task of its own, and the turn's trace is marked with the judgment's
    /// id. Returns at once, whatever Jev does; nothing when the pack is off,
    /// the recall is out of its sample, or no candidate passed the filters.
    pub fn at_recall(&self, trace: &mut Trace, recalled: Recalled) {
        let mode = self.cfg.mode_of(RERANK_PACK, PackMode::Shadow);
        if mode == PackMode::Off {
            return;
        }
        let Some(pack) = theseus_judge::pack::by_name(RERANK_PACK) else {
            return;
        };
        if !sampled(
            &recalled.recall_id,
            self.cfg.sample_of(RERANK_PACK, pack.sample),
        ) {
            return;
        }
        let eligible = eligible(
            recalled.science.as_ref(),
            &recalled.asker(),
            &recalled.candidates,
            &recalled.params,
        );
        if eligible.is_empty() {
            return;
        }
        let Ok(rt) = tokio::runtime::Handle::try_current() else {
            return;
        };
        let id = theseus_judge::judge::new_id();
        trace.mark(
            "judge",
            "mark",
            json!({"pack": RERANK_PACK, "point": "recall", "mode": mode.as_str(), "judgment": id}),
        );
        rt.spawn(judge_rerank(
            self.me.clone(),
            pack,
            Dispatched {
                recalled,
                eligible,
                id,
            },
        ));
    }

    /// The blocking half before the call: the state, its blob, and the
    /// reservation. `None`: nothing to send (the day's limit among them).
    fn prepare_rerank(
        &self,
        pack: Arc<Pack>,
        d: &Dispatched,
        today: &str,
    ) -> Option<super::Prepared> {
        let r = &d.recalled;
        let input = RerankInput {
            message: r.message.chars().take(MESSAGE_CHARS).collect(),
            notes: d
                .eligible
                .iter()
                .take(RERANK_NOTES)
                .map(|c| RerankNote {
                    key: c.key(),
                    text: c.text.clone(),
                })
                .collect(),
        };
        let scrub = ScrubWith(self.scrubber.clone());
        let state = theseus_judge::prepare(&pack, &Input::Rerank(input), &scrub).ok()?;
        let blob = self
            .store
            .blobs()
            .put(state.state.json.as_bytes())
            .map_err(|e| tracing::warn!(error = %e, "judge: the rerank's state was not written; not judged"))
            .ok()?;
        let built = self
            .built()
            .map_err(|e| tracing::warn!(error = %format!("{e:#}"), "judge: the Jev client was not built"))
            .ok()?;
        let context = json!({
            "session": r.session_id, "turn": r.turn_id, "recall": r.recall_id,
            "purpose": "recall", "arm": "+rerank", "baseline": "fused", "blob": blob,
            "deadline_ms": self.rerank_deadline().as_millis() as u64, "on_path_ms": 0,
        });
        let mut ask = Ask::new(pack, &state, Mode::Shadow, context);
        ask.id = Some(d.id.clone());
        let need = built
            .judge
            .inner()
            .reserve_micros(std::slice::from_ref(&ask))
            .unwrap_or(0);
        self.reserve(today, need)
            .then_some(super::Prepared { built, ask, need })
    }
}

/// Jev's probability for each note it answered, by the note's key.
pub fn probabilities(j: &Judgment) -> BTreeMap<String, f64> {
    j.answers
        .iter()
        .filter_map(|a| match a.answer {
            theseus_judge::Answer::Noul { noul } => Some((a.about.clone()?, noul)),
            _ => None,
        })
        .collect()
}

/// Why a rerank fell back to the fused order, if it did: the outcome's
/// class or skip reason, or `model_drift`.
pub fn fallback(j: &Judgment) -> Option<String> {
    match &j.outcome {
        Outcome::Answered if j.model_drift => Some("model_drift".into()),
        Outcome::Answered => None,
        Outcome::Skipped { reason } => Some(
            serde_json::to_value(reason)
                .ok()
                .and_then(|v| v.as_str().map(str::to_string))
                .unwrap_or_else(|| "skipped".into()),
        ),
        Outcome::Failed { class, .. } => Some(class.clone()),
    }
}

/// What the row adds of the rerank: the recall, both orders' admitted
/// keys, whether they differ, Jev's latency against the deadline, and the
/// fallback.
fn rerank_record(d: &Dispatched, j: &Judgment, deadline: Duration) -> Value {
    let r = &d.recalled;
    let fused: Vec<String> = d.eligible.iter().map(Candidate::key).collect();
    let fell = fallback(j);
    let order = match fell {
        Some(_) => fused.clone(),
        None => reorder(&fused, &probabilities(j)),
    };
    let top: Vec<&String> = order.iter().take(RERANK_NOTES).collect();
    let pack = repack(
        r.science.as_ref(),
        &r.asker(),
        r.candidates.clone(),
        &r.params,
        order.clone(),
    );
    let reranked: Vec<String> = pack.admitted.iter().map(|a| a.candidate.key()).collect();
    let as_set = |v: &[String]| v.iter().cloned().collect::<BTreeSet<_>>();
    let deadline_ms = deadline.as_millis() as u64;
    json!({
        "recall": r.recall_id,
        "eligible": fused.len(),
        "asked": j.questions,
        "fused_admitted": r.admitted,
        "reranked_admitted": reranked,
        "changed": as_set(&r.admitted) != as_set(&reranked),
        "order_changed": order != fused,
        "top": top,
        "fallback": fell,
        "latency_ms": j.timing.total_ms,
        "deadline_ms": deadline_ms,
        "within_deadline": j.timing.total_ms <= deadline_ms,
        "cost_micros": j.cost_micros,
    })
}

/// One rerank, in its own task. The service is held only around the
/// blocking halves, never across the call.
async fn judge_rerank(me: Weak<JudgeService>, pack: Arc<Pack>, d: Dispatched) {
    let today = spend::local_day(theseus_protocol::now_unix_ms());
    let Some(svc) = me.upgrade() else { return };
    let deadline = svc.rerank_deadline();
    let day = today.clone();
    let Ok((Some(super::Prepared { built, ask, need }), d)) =
        tokio::task::spawn_blocking(move || {
            let p = svc.prepare_rerank(pack, &d, &day);
            (p, d)
        })
        .await
    else {
        return;
    };
    let t0 = Instant::now();
    let mut judgments = built
        .judge
        .inner()
        .judge(DecisionPoint {
            asks: vec![ask],
            urgency: Urgency::live(deadline),
        })
        .await;
    tracing::debug!(
        ms = t0.elapsed().as_millis() as u64,
        "judge: rerank.v1 judged"
    );
    // The reorder and the repack are pure, and small (at most the index's
    // hits): they run here, before the row is recorded.
    for j in &mut judgments {
        let rerank = rerank_record(&d, j, deadline);
        if let Some(o) = j.context.as_object_mut() {
            o.insert("rerank".into(), rerank);
        }
        built.judge.sink().record(j);
    }
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
