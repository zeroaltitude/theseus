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
use theseus_judge::breaker::Status;
use theseus_judge::builders::{RerankInput, RerankNote, RERANK_NOTES};
use theseus_judge::{
    Ask, DecisionPoint, Input, Judge, Judgment, JudgmentSink, Mode, Outcome, Pack, Urgency,
};
use theseus_memory::recall::{Asker, Candidate, Link, Params, Place};
use theseus_memory::rerank::{eligible, reorder, repack};
use theseus_memory::{MemoryScience, Retention};
use theseus_protocol::memory::RecallRerank;
use tokio::sync::oneshot;

use super::{sampled, spend, JudgeService, ScrubWith};
use crate::config::PackMode;
use crate::trace::Trace;

/// Recall's `+rerank` arm (M6 32c).
pub const RERANK_PACK: &str = "rerank.v1";

/// The rerank's own deadline (§2.7: Jev takes about 350 ms).
pub const DEADLINE: Duration = Duration::from_millis(600);

/// Rerank's own breaker (32d): its failures and timeouts move it alone.
pub const BREAKER: &str = "rerank";

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
    /// The turn's arm's science (`Memory::science_for`), and the candidates'
    /// retention when it reads it (32a): the repack ranks as the recall did.
    pub science: Arc<dyn MemoryScience>,
    pub retention: BTreeMap<String, Retention>,
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
            retention: &self.retention,
        }
    }
}

/// What a live rerank hands the waiting turn: Jev's order, by key (the
/// eligible notes re-sorted, then the rest in the fused order), or why
/// recall's own stands.
type Handoff = Result<Vec<String>, String>;

/// A rerank on its way: the recall, and its eligible notes in the fused
/// order; a live one's handoff to its waiting turn.
struct Dispatched {
    recalled: Recalled,
    eligible: Vec<Candidate>,
    id: String,
    mode: Mode,
    live: Option<oneshot::Sender<Handoff>>,
}

impl Dispatched {
    /// Hand the turn `h`: whether it took it (a send after the turn gave
    /// up, or with no turn waiting, is not taken). `None`: no turn waits on
    /// this rerank (a shadow one).
    fn hand(&mut self, h: Handoff) -> Option<bool> {
        self.live.take().map(|tx| tx.send(h).is_ok())
    }
}

/// What a live rerank did for the turn that waited on it (32d).
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Waited {
    /// The rerank's judgment, when one was dispatched.
    pub judgment: Option<String>,
    /// Jev's order, by key, when it came in time and answered.
    pub order: Option<Vec<String>>,
    /// Why recall's own order stands, when it does.
    pub why: Option<String>,
    /// From the rerank's start to the turn going on.
    pub waited: Duration,
    /// The wait's bound.
    pub wait: Duration,
}

impl Waited {
    /// The recall manifest's account of it.
    pub fn manifest(&self) -> RecallRerank {
        RecallRerank {
            judgment: self.judgment.clone(),
            applied: self.order.is_some(),
            why: self.why.clone(),
            waited_ms: (self.waited.as_secs_f64() * 1_000_000.0).round() / 1000.0,
            wait_ms: self.wait.as_millis() as u64,
        }
    }
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

    /// `rerank.v1`'s mode for `session` now: the ladder's (26a, `mode_for`),
    /// under the config's ceiling and its own line. The arms rule (32d):
    /// memory's mode decides what
    /// reaches the model, rerank's whether Jev orders it. A recall in front
    /// of the model is reranked live when this is `live`, in shadow when
    /// `shadow`; one that reaches no model is reranked in shadow, as 32c's;
    /// `off` reranks none.
    pub fn rerank_mode(&self, session: &str) -> PackMode {
        self.mode_for(RERANK_PACK, session).mode
    }

    /// A turn's recall ran: `rerank.v1` judges its top 20 in shadow, in a
    /// task of its own, and the turn's trace is marked with the judgment's
    /// id. Returns at once, whatever Jev does; nothing when the pack is off,
    /// the recall is out of its sample, or no candidate passed the filters.
    pub fn at_recall(&self, trace: &mut Trace, recalled: Recalled) {
        if self.rerank_mode(&recalled.session_id) == PackMode::Off {
            return;
        }
        let Some((pack, eligible)) = self.rerank_eligible(&recalled) else {
            return;
        };
        let Ok(rt) = tokio::runtime::Handle::try_current() else {
            return;
        };
        let id = theseus_judge::judge::new_id();
        trace.mark(
            "judge",
            "mark",
            json!({"pack": RERANK_PACK, "point": "recall", "mode": "shadow", "judgment": id}),
        );
        rt.spawn(judge_rerank(
            self.me.clone(),
            pack,
            Dispatched {
                recalled,
                eligible,
                id,
                mode: Mode::Shadow,
                live: None,
            },
        ));
    }

    /// The pack and the recall's eligible notes, when it is in the pack's
    /// sample and any passed the filters (the place rule first, and the
    /// owner's labels: only they may reach Jev).
    fn rerank_eligible(&self, r: &Recalled) -> Option<(Arc<Pack>, Vec<Candidate>)> {
        let pack = theseus_judge::pack::by_name(RERANK_PACK)?;
        if !sampled(&r.recall_id, self.cfg.sample_of(RERANK_PACK, pack.sample)) {
            return None;
        }
        let eligible = eligible(r.science.as_ref(), &r.asker(), &r.candidates, &r.params);
        (!eligible.is_empty()).then_some((pack, eligible))
    }

    /// Why a live rerank would not be waited on now, read without a write:
    /// rerank's breaker open, or the day's limit reached.
    fn rerank_no_wait(&self) -> Option<&'static str> {
        if let Some(b) = self.built.get() {
            let open = b
                .judge
                .inner()
                .own_breakers()
                .into_iter()
                .any(|(n, s)| n == BREAKER && matches!(s, Status::Open { .. }));
            if open {
                return Some("breaker_open");
            }
        }
        let today = spend::local_day(theseus_protocol::now_unix_ms());
        self.budget.paused(&today).then_some("budget")
    }

    /// A recall in front of the model, with `rerank.v1` live (32d): its
    /// top 20 go to Jev in a task of its own, as `at_recall`'s do, and the
    /// turn waits for the answer at most `wait` from now, the state's build
    /// included. In time and answered, Jev's order comes back; otherwise
    /// (a miss, a timeout, a failure, rerank's breaker open, the day's
    /// budget spent) recall's own stands, and an answer that comes after the
    /// turn went on is still recorded, marked `late`. The call keeps its own
    /// deadline. A `judge` span covers the wait.
    pub async fn at_recall_live(
        &self,
        trace: &mut Trace,
        recalled: Recalled,
        wait: Duration,
    ) -> Waited {
        let start = tokio::time::Instant::now();
        let t0 = trace.now_us();
        let mut w = Waited {
            wait,
            ..Waited::default()
        };
        let dispatched = match self.rerank_eligible(&recalled) {
            Some(pe) => tokio::runtime::Handle::try_current()
                .ok()
                .map(|rt| (pe, rt)),
            None => None,
        };
        let Some(((pack, eligible), rt)) = dispatched else {
            w.why = Some("nothing_eligible".into());
            return w;
        };
        let id = theseus_judge::judge::new_id();
        w.judgment = Some(id.clone());
        trace.mark(
            "judge",
            "mark",
            json!({"pack": RERANK_PACK, "point": "recall", "mode": "live", "judgment": id}),
        );
        let no_wait = self.rerank_no_wait();
        let (tx, mut rx) = oneshot::channel();
        rt.spawn(judge_rerank(
            self.me.clone(),
            pack,
            Dispatched {
                recalled,
                eligible,
                id: id.clone(),
                mode: Mode::Live,
                live: no_wait.is_none().then_some(tx),
            },
        ));
        let got = match no_wait {
            Some(why) => Err(why.to_string()),
            None => match tokio::time::timeout_at(start + wait, &mut rx).await {
                Ok(Ok(h)) => h,
                // The rerank ended without a word: it sent nothing.
                Ok(Err(_)) => Err("not_sent".into()),
                // The wait is over. An answer handed over before this is
                // taken; after it, the rerank finds the turn gone.
                Err(_) => {
                    rx.close();
                    rx.try_recv().unwrap_or_else(|_| Err("timeout".into()))
                }
            },
        };
        w.waited = start.elapsed();
        match got {
            Ok(order) => w.order = Some(order),
            Err(why) => w.why = Some(why),
        }
        trace.record(
            "judge",
            "wait",
            t0,
            trace.now_us(),
            json!({"pack": RERANK_PACK, "point": "recall", "mode": "live", "judgment": id,
                   "applied": w.order.is_some(), "why": w.why, "wait_ms": wait.as_millis() as u64}),
        );
        w
    }

    /// The blocking half before the call: the state, its blob, and the
    /// reservation. `Err`: nothing to send, and why (the day's limit is
    /// `budget`).
    fn prepare_rerank(
        &self,
        pack: Arc<Pack>,
        d: &Dispatched,
        today: &str,
    ) -> Result<super::Prepared, &'static str> {
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
        let state =
            theseus_judge::prepare(&pack, &Input::Rerank(input), &scrub).map_err(|_| "state")?;
        let blob = self
            .store
            .blobs()
            .put(state.state.json.as_bytes())
            .map_err(|e| tracing::warn!(error = %e, "judge: the rerank's state was not written; not judged"))
            .map_err(|_| "state")?;
        let built = self
            .built()
            .map_err(|e| tracing::warn!(error = %format!("{e:#}"), "judge: the Jev client was not built"))
            .map_err(|_| "client")?;
        let live = d.mode == Mode::Live;
        let mut context = json!({
            "session": r.session_id, "turn": r.turn_id, "recall": r.recall_id,
            "purpose": "recall", "arm": "+rerank", "baseline": "fused", "blob": blob,
            "deadline_ms": self.rerank_deadline().as_millis() as u64,
            "on_path_ms": 0, "live": live,
        });
        // The ladder's arm in the context (26a). A shadow rerank stays
        // shadow; a live one is the ladder's: live, or canary in a canary's
        // arm.
        let laddered = self.ask_mode(&pack.name(), &mut context);
        let mode = match d.mode {
            Mode::Shadow => Mode::Shadow,
            _ => laddered,
        };
        let mut ask = Ask::new(pack, &state, mode, context);
        ask.id = Some(d.id.clone());
        let need = built
            .judge
            .inner()
            .reserve_micros(std::slice::from_ref(&ask))
            .unwrap_or(0);
        match self.reserve(today, need) {
            true => Ok(super::Prepared { built, ask, need }),
            false => Err("budget"),
        }
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

/// The order a judgment gives the eligible notes: Jev's over the fused
/// one's top 20, or the fused one on a fallback.
fn order_of(d: &Dispatched, j: &Judgment) -> (Vec<String>, Vec<String>, Option<String>) {
    let fused: Vec<String> = d.eligible.iter().map(Candidate::key).collect();
    let fell = fallback(j);
    let order = match fell {
        Some(_) => fused.clone(),
        None => reorder(&fused, &probabilities(j)),
    };
    (fused, order, fell)
}

/// What the row adds of the rerank: the recall, both orders' admitted
/// keys, whether they differ, Jev's latency against the deadline, the
/// fallback, and for a live one whether the turn used its order (`applied`)
/// or had gone on before it came (`late`).
fn rerank_record(
    d: &Dispatched,
    j: &Judgment,
    deadline: Duration,
    (fused, order, fell): (Vec<String>, Vec<String>, Option<String>),
    taken: Option<bool>,
) -> Value {
    let r = &d.recalled;
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
    let mut v = json!({
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
    });
    if d.mode == Mode::Live {
        v["live"] = json!(true);
        v["applied"] = json!(taken == Some(true) && fell.is_none());
        v["late"] = json!(taken == Some(false));
    }
    v
}

/// One rerank, in its own task. The service is held only around the
/// blocking halves, never across the call. A live one hands its waiting
/// turn the order (or why there is none) before its row is recorded.
async fn judge_rerank(me: Weak<JudgeService>, pack: Arc<Pack>, d: Dispatched) {
    let today = spend::local_day(theseus_protocol::now_unix_ms());
    let Some(svc) = me.upgrade() else { return };
    let deadline = svc.rerank_deadline();
    let day = today.clone();
    let Ok((prepared, mut d)) = tokio::task::spawn_blocking(move || {
        let p = svc.prepare_rerank(pack, &d, &day);
        (p, d)
    })
    .await
    else {
        return;
    };
    let super::Prepared { built, ask, need } = match prepared {
        Ok(p) => p,
        Err(why) => {
            d.hand(Err(why.into()));
            return;
        }
    };
    let urgency = Urgency::live(deadline);
    let t0 = Instant::now();
    #[cfg(test)]
    let mut judgments = match svc_test_judge(&me) {
        Some(j) => {
            j.judge(DecisionPoint {
                asks: vec![ask],
                urgency,
            })
            .await
        }
        None => {
            built
                .judge
                .inner()
                .judge(DecisionPoint {
                    asks: vec![ask],
                    urgency,
                })
                .await
        }
    };
    #[cfg(not(test))]
    let mut judgments = built
        .judge
        .inner()
        .judge(DecisionPoint {
            asks: vec![ask],
            urgency,
        })
        .await;
    tracing::debug!(
        ms = t0.elapsed().as_millis() as u64,
        "judge: rerank.v1 judged"
    );
    // The reorder and the repack are pure, and small (at most the index's
    // hits): they run here, before the row is recorded.
    for j in &mut judgments {
        let orders = order_of(&d, j);
        let taken = d.hand(match &orders.2 {
            None => Ok(orders.1.clone()),
            Some(fell) => Err(fell.clone()),
        });
        let rerank = rerank_record(&d, j, deadline, orders, taken);
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

/// A test's judge in Jev's place, when one is set (the paused clock's
/// tests: a channel for Jev).
#[cfg(test)]
fn svc_test_judge(me: &Weak<JudgeService>) -> Option<Arc<dyn Judge>> {
    me.upgrade()?.rerank_judge.get().cloned()
}
