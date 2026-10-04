//! The `Judge` trait (spec §3.7), its one implementation over the Jev client,
//! and the `Recording` decorator that hands every judgment to a
//! [`JudgmentSink`], the spec's "every call recorded".
//!
//! A decision point asks some packs about their states. The judge prices
//! them (an unpriced model is never called), batches the ones that share a
//! state, checks the breaker, sends the batches concurrently, and returns one
//! [`Judgment`] per ask, in the asks' order, whatever happened: answered,
//! skipped (shed, the breaker open, unpriced, no key), or failed with a
//! classified error. Each answer carries its band. A judgment whose answer
//! came from another model than the pack pins is marked `model_drift` and is
//! never acted on: calibration does not transfer between models.
//!
//! What the core adds at the wire-in (23a) rides in `context`, opaque here:
//! session, execution, turn, loop, the arm, the baseline's decision, the
//! workload class.

use std::collections::BTreeMap;
use std::sync::{Arc, Mutex};
use std::time::Instant;

use futures_util::future::BoxFuture;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::band::{band, Banded};
use crate::batch::{self, Batch, Part};
use crate::breaker::{self, Breaker, BreakerConfig, Status, Transition};
use crate::builders::Prepared;
use crate::client::{Answer, CallError, JevClient, JevError, Timing, Urgency, Usage};
use crate::pack::{Asked, Pack, Point};
use crate::price::{JevPrice, Micros};
use crate::state::BuiltState;

/// The model a pack may name only where nothing acts on it.
pub const UNPINNED: &str = "jev-latest";

/// The pack's mode when the judgment was taken (the core's ladder decides).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Mode {
    Shadow,
    Canary,
    Live,
}

/// One pack asked about one state.
#[derive(Debug, Clone)]
pub struct Ask {
    pub pack: Arc<Pack>,
    pub state: Arc<BuiltState>,
    pub asked: Vec<Asked>,
    pub mode: Mode,
    /// The core's own fields, echoed into the judgment untouched.
    pub context: Value,
    /// The judgment's id, when the core minted it at its dispatch (23b: the
    /// turn's trace marks it before the call); `None`, minted here.
    pub id: Option<String>,
}

/// A new judgment's id: `jdg_<uuid v7>`.
pub fn new_id() -> String {
    format!("jdg_{}", uuid::Uuid::now_v7().simple())
}

impl Ask {
    pub fn new(pack: Arc<Pack>, prepared: &Prepared, mode: Mode, context: Value) -> Self {
        let asked = pack.ask(&prepared.dynamic);
        Self {
            pack,
            state: prepared.state.clone(),
            asked,
            mode,
            context,
            id: None,
        }
    }
}

/// Everything asked at one decision point, and how urgently.
#[derive(Debug, Clone)]
pub struct DecisionPoint {
    pub asks: Vec<Ask>,
    pub urgency: Urgency,
}

/// Why a judgment never reached Jev.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Skip {
    /// Every in-flight permit was taken (shadow only).
    Shed,
    /// The breaker is open: shadow skips, live abstains, at once.
    CircuitOpen,
    /// The pack's model has no catalog price.
    Unpriced,
    /// The key has not settled.
    NoKey,
    /// The state left the pack nothing to ask (an `only_when` source empty).
    NoQuestions,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "outcome", rename_all = "snake_case")]
pub enum Outcome {
    Answered,
    Skipped {
        reason: Skip,
    },
    Failed {
        error: JevError,
        class: String,
        transient: bool,
        usage_unknown: bool,
    },
}

impl Outcome {
    fn failed(error: JevError) -> Self {
        Outcome::Failed {
            class: error.class().to_string(),
            transient: error.is_transient(),
            usage_unknown: error.usage_unknown(),
            error,
        }
    }
}

/// What the record keeps of a state; its JSON goes to a blob.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct StateRecord {
    pub sha256: String,
    pub bytes: usize,
    pub tokens: u64,
    pub cap_tokens: u64,
    pub builder: String,
    pub builder_version: u32,
    pub truncated: Vec<String>,
    pub dropped: Vec<String>,
}

impl From<&BuiltState> for StateRecord {
    fn from(s: &BuiltState) -> Self {
        Self {
            sha256: s.sha256.clone(),
            bytes: s.bytes,
            tokens: s.tokens,
            cap_tokens: s.cap_tokens,
            builder: s.builder.clone(),
            builder_version: s.builder_version,
            truncated: s.truncated.clone(),
            dropped: s.dropped.clone(),
        }
    }
}

/// One answer, with its band.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AnswerRecord {
    pub question: String,
    /// The definition it came from (differs for a per-item Noul).
    pub def: String,
    pub about: Option<String>,
    pub answer: Answer,
    pub band: Banded,
}

/// The call a judgment rode in, shared with the other judgments of a batch.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CallRecord {
    pub id: String,
    pub packs: usize,
    pub questions: usize,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Judgment {
    /// `jdg_<uuid v7>`, the ledger row's key.
    pub id: String,
    /// `loop.v1`.
    pub pack: String,
    pub version: u32,
    pub pack_sha256: String,
    pub point: Point,
    pub mode: Mode,
    /// The model the pack pins, and the one that answered.
    pub model: String,
    pub answered_by: Option<String>,
    pub model_drift: bool,
    pub state: StateRecord,
    pub questions: usize,
    pub answers: Vec<AnswerRecord>,
    pub call: Option<CallRecord>,
    pub timing: Timing,
    /// This judgment's share of the call's usage and cost (by question
    /// count), or none when it never ran or its usage is unknown.
    pub usage: Option<Usage>,
    pub cost_micros: Option<Micros>,
    /// This judgment's share of the call's reservation (none when unpriced).
    pub reserve_micros: Option<Micros>,
    pub outcome: Outcome,
    /// A breaker change this call caused (`judge.circuit`).
    pub circuit: Option<Transition>,
    pub rate_limit: BTreeMap<String, String>,
    pub context: Value,
}

impl Judgment {
    fn pending(ask: &Ask) -> Self {
        Self {
            id: ask.id.clone().unwrap_or_else(new_id),
            pack: ask.pack.name(),
            version: ask.pack.version,
            pack_sha256: ask.pack.sha256.clone(),
            point: ask.pack.point,
            mode: ask.mode,
            model: ask.pack.jev_model.clone(),
            answered_by: None,
            model_drift: false,
            state: StateRecord::from(ask.state.as_ref()),
            questions: ask.asked.len(),
            answers: Vec::new(),
            call: None,
            timing: Timing::default(),
            usage: None,
            cost_micros: None,
            reserve_micros: None,
            outcome: Outcome::Skipped {
                reason: Skip::NoQuestions,
            },
            circuit: None,
            rate_limit: BTreeMap::new(),
            context: ask.context.clone(),
        }
    }

    /// Answered by the model the pack pins: the only kind the core may act on.
    pub fn actionable(&self) -> bool {
        self.outcome == Outcome::Answered && !self.model_drift
    }

    pub fn answer(&self, question: &str) -> Option<&AnswerRecord> {
        self.answers.iter().find(|a| a.question == question)
    }
}

/// Every judgment goes here (the core's sink writes ledger rows and state
/// blobs in batched frames, from its own task; it must not block).
pub trait JudgmentSink: Send + Sync {
    fn record(&self, judgment: &Judgment);
}

/// Keeps judgments in memory (tests, and the probe).
#[derive(Debug, Default)]
pub struct MemorySink(Mutex<Vec<Judgment>>);

impl MemorySink {
    pub fn judgments(&self) -> Vec<Judgment> {
        self.0.lock().unwrap_or_else(|e| e.into_inner()).clone()
    }
}

impl JudgmentSink for MemorySink {
    fn record(&self, judgment: &Judgment) {
        self.0
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .push(judgment.clone());
    }
}

impl<S: JudgmentSink + ?Sized> JudgmentSink for Arc<S> {
    fn record(&self, judgment: &Judgment) {
        (**self).record(judgment);
    }
}

pub trait Judge: Send + Sync {
    /// One judgment per ask, in the asks' order. Never fails as a whole.
    fn judge(&self, point: DecisionPoint) -> BoxFuture<'_, Vec<Judgment>>;
}

/// Records every judgment its inner judge returns.
pub struct Recording<J, S> {
    inner: J,
    sink: S,
}

impl<J, S> Recording<J, S> {
    pub fn new(inner: J, sink: S) -> Self {
        Self { inner, sink }
    }

    pub fn inner(&self) -> &J {
        &self.inner
    }

    pub fn sink(&self) -> &S {
        &self.sink
    }
}

impl<J: Judge, S: JudgmentSink> Judge for Recording<J, S> {
    fn judge(&self, point: DecisionPoint) -> BoxFuture<'_, Vec<Judgment>> {
        Box::pin(async move {
            let judgments = self.inner.judge(point).await;
            for j in &judgments {
                self.sink.record(j);
            }
            judgments
        })
    }
}

/// The judge over TypeSafe Jev.
pub struct JevJudge {
    client: JevClient,
    prices: BTreeMap<String, JevPrice>,
    breaker: Mutex<Breaker>,
}

impl JevJudge {
    pub fn new(
        client: JevClient,
        prices: BTreeMap<String, JevPrice>,
        breaker: BreakerConfig,
    ) -> Self {
        Self {
            client,
            prices,
            breaker: Mutex::new(Breaker::new(breaker)),
        }
    }

    pub fn client(&self) -> &JevClient {
        &self.client
    }

    pub fn breaker_status(&self) -> Status {
        self.breaker().status(Instant::now())
    }

    fn breaker(&self) -> std::sync::MutexGuard<'_, Breaker> {
        self.breaker.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// What the asks would reserve, batched as they would go out; none when
    /// a model is unpriced. The core's live spend reserves this up front.
    pub fn reserve_micros(&self, asks: &[Ask]) -> Option<Micros> {
        let parts: Vec<Part> = asks
            .iter()
            .enumerate()
            .filter(|(_, a)| !a.asked.is_empty())
            .map(|(i, a)| part(i, a))
            .collect();
        batch::plan(parts)
            .iter()
            .map(|b| {
                Some(
                    self.prices
                        .get(&b.request.model)?
                        .reserve_request(&b.request),
                )
            })
            .sum()
    }

    async fn run(&self, b: Batch, urgency: Urgency, out: &mut [Option<Judgment>]) {
        let call_id = format!("jcl_{}", uuid::Uuid::now_v7().simple());
        let counts: Vec<usize> = b.parts.iter().map(|p| p.asked.len()).collect();
        let total_q: usize = counts.iter().sum();
        let price = self.prices.get(&b.request.model).cloned();
        let reserve = price
            .as_ref()
            .map(|p| batch::shares(p.reserve_request(&b.request), &counts));
        for (k, p) in b.parts.iter().enumerate() {
            if let Some(j) = out[p.tag].as_mut() {
                j.call = Some(CallRecord {
                    id: call_id.clone(),
                    packs: b.parts.len(),
                    questions: total_q,
                });
                j.reserve_micros = reserve.as_ref().map(|r| r[k]);
            }
        }
        if !self.breaker().admit(Instant::now()) {
            for p in &b.parts {
                if let Some(j) = out[p.tag].as_mut() {
                    j.outcome = Outcome::Skipped {
                        reason: Skip::CircuitOpen,
                    };
                }
            }
            return;
        }
        let called = self.client.call(&b.request, urgency).await;
        let counted = match &called.result {
            Ok(_) => breaker::Outcome::Answered,
            Err(CallError::Jev(e)) if e.counts_for_breaker() => breaker::Outcome::Transient,
            Err(CallError::Jev(_)) => breaker::Outcome::Answered,
            Err(CallError::Shed | CallError::NoKey) => breaker::Outcome::NotSent,
        };
        let circuit = self.breaker().record(Instant::now(), counted);
        // The usage the call billed, when known, split by question count.
        let billed = match &called.result {
            Ok(r) => Some(r.usage),
            Err(CallError::Jev(JevError::Malformed { usage, .. })) => *usage,
            Err(_) => None,
        };
        let usages: Option<Vec<Usage>> = billed.map(|u| {
            let ins = batch::shares(u.input_tokens, &counts);
            let outs = batch::shares(u.output_tokens, &counts);
            ins.into_iter()
                .zip(outs)
                .map(|(input_tokens, output_tokens)| Usage {
                    input_tokens,
                    output_tokens,
                })
                .collect()
        });
        let costs: Option<Vec<Micros>> = billed
            .zip(price.as_ref())
            .map(|(u, p)| batch::shares(p.cost_micros(&u), &counts));
        let split = match &called.result {
            Ok(r) => Some((r.model.clone(), batch::split(&b, r))),
            Err(_) => None,
        };
        for (k, p) in b.parts.iter().enumerate() {
            let Some(j) = out[p.tag].as_mut() else {
                continue;
            };
            j.timing = called.timing;
            j.rate_limit = called.rate_limit.clone();
            j.usage = usages.as_ref().map(|u| u[k]);
            j.cost_micros = costs.as_ref().map(|c| c[k]);
            if k == 0 {
                j.circuit = circuit;
            }
            j.outcome = match &called.result {
                Ok(_) => Outcome::Answered,
                Err(CallError::Shed) => Outcome::Skipped { reason: Skip::Shed },
                Err(CallError::NoKey) => Outcome::Skipped {
                    reason: Skip::NoKey,
                },
                Err(CallError::Jev(e)) => Outcome::failed(e.clone()),
            };
            if let Some((model, split)) = &split {
                j.answered_by = Some(model.clone());
                j.model_drift = j.model != UNPINNED && *model != j.model;
                j.answers = split[k]
                    .iter()
                    .filter_map(|(asked, answer)| {
                        let answer = answer.clone()?;
                        Some(AnswerRecord {
                            question: asked.id.clone(),
                            def: asked.def.clone(),
                            about: asked.about.clone(),
                            band: band(&answer, asked.thresholds),
                            answer,
                        })
                    })
                    .collect();
            }
        }
    }
}

pub(crate) fn part(tag: usize, a: &Ask) -> Part {
    Part {
        tag,
        pack: a.pack.clone(),
        state: a.state.clone(),
        asked: a.asked.clone(),
    }
}

impl Judge for JevJudge {
    fn judge(&self, point: DecisionPoint) -> BoxFuture<'_, Vec<Judgment>> {
        Box::pin(async move {
            let mut out: Vec<Option<Judgment>> = point
                .asks
                .iter()
                .map(|a| Some(Judgment::pending(a)))
                .collect();
            let mut parts = Vec::new();
            for (i, a) in point.asks.iter().enumerate() {
                let j = out[i].as_mut().expect("pending");
                if a.asked.is_empty() {
                    continue;
                }
                if !self.prices.contains_key(&a.pack.jev_model) {
                    j.outcome = Outcome::Skipped {
                        reason: Skip::Unpriced,
                    };
                    continue;
                }
                parts.push(part(i, a));
            }
            let batches = batch::plan(parts);
            // Each batch fills only its own parts' slots, so the batches run
            // concurrently over disjoint slices of the output.
            let mut slots: Vec<Vec<Option<Judgment>>> = Vec::with_capacity(batches.len());
            for b in &batches {
                slots.push(
                    out.iter_mut()
                        .enumerate()
                        .map(|(i, j)| {
                            if b.parts.iter().any(|p| p.tag == i) {
                                j.take()
                            } else {
                                None
                            }
                        })
                        .collect(),
                );
            }
            let runs = batches
                .into_iter()
                .zip(slots.iter_mut())
                .map(|(b, slot)| self.run(b, point.urgency, slot));
            futures_util::future::join_all(runs).await;
            for slot in slots {
                for (i, j) in slot.into_iter().enumerate() {
                    if j.is_some() {
                        out[i] = j;
                    }
                }
            }
            out.into_iter()
                .map(|j| j.expect("every ask has its judgment"))
                .collect()
        })
    }
}
