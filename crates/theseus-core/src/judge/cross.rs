//! `recall_cross.v1` (theseus-98bz.10): Jev reads the notes the thread lens
//! dropped as another stream's, near the bar, and says of each whether it is
//! about the very subject the message asks about, or only the same kind of
//! fact. The turn waits for it, bounded (`turn/cross_step.rs`).
//!
//! - **One request**, at most `[threads] cross_recall_judge_k` notes, in a
//!   task of its own: the state, its blob, the reservation, the call and the
//!   record are the task's, as the rerank's are. Its cost is in the
//!   `judge.call` row the sink writes (context `purpose: "recall"`).
//! - **A yes** is a note's Noul true at the question's confirm bar or above,
//!   in a judgment that acts (canary or live): the turn admits it with the
//!   penalty. A timeout, a failure, the day's budget spent, or Jev known
//!   unreachable gives none, and the drops stand.

use std::collections::BTreeSet;
use std::sync::{Arc, Weak};
use std::time::Duration;

use serde_json::json;
use theseus_judge::band::{Band, Top};
use theseus_judge::breaker::Status;
use theseus_judge::builders::{CrossInput, CrossNote};
use theseus_judge::{
    Ask, DecisionPoint, Input, Judge, Judgment, JudgmentSink, Mode, Outcome, Pack, Urgency,
};
use tokio::sync::oneshot;

use super::{spend, JudgeService, ScrubWith};
use crate::config::PackMode;
use crate::trace::Trace;

/// The pack (theseus-98bz.10).
pub const CROSS_PACK: &str = "recall_cross.v1";

/// The cross judge's own breaker (theseus-pt7lt): its failures move only it,
/// so a flaky cross judge stops neither `classify` nor routing.
pub const BREAKER: &str = "recall_cross";

/// What the turn asks about.
pub struct Cross {
    pub session_id: String,
    pub turn_id: String,
    pub recall_id: String,
    pub message: String,
    pub notes: Vec<CrossNote>,
}

/// The notes Jev said are about the message's subject, by node id.
pub fn same_subject(j: &Judgment) -> BTreeSet<String> {
    if !j.actionable() || j.mode == Mode::Shadow {
        return BTreeSet::new();
    }
    j.answers
        .iter()
        .filter(|a| a.band.band != Band::Escalate && a.band.top == Top::Noul(true))
        .filter_map(|a| a.about.clone())
        .collect()
}

impl JudgeService {
    /// Ask `recall_cross.v1` about `c`'s notes and wait at most `wait`: the
    /// node ids Jev said yes about, or none (off, no answer in time, a
    /// failure). A `judge` span covers the wait.
    pub async fn cross_live(
        &self,
        trace: &mut Trace,
        c: Cross,
        wait: Duration,
    ) -> Option<BTreeSet<String>> {
        if c.notes.is_empty() || self.mode_for(CROSS_PACK, &c.session_id).mode == PackMode::Off {
            return None;
        }
        if self.jev_unreachable() || self.cross_breaker_open() {
            return None;
        }
        let pack = theseus_judge::pack::by_name(CROSS_PACK)?;
        let rt = tokio::runtime::Handle::try_current().ok()?;
        let id = theseus_judge::judge::new_id();
        let t0 = trace.now_us();
        trace.mark(
            "judge",
            "mark",
            json!({"pack": CROSS_PACK, "point": "recall", "mode": "live", "judgment": id}),
        );
        let (tx, rx) = oneshot::channel();
        rt.spawn(judge_cross(self.me.clone(), pack, c, id.clone(), tx));
        let got = tokio::time::timeout(wait, rx)
            .await
            .ok()
            .and_then(Result::ok);
        trace.record(
            "judge",
            "wait",
            t0,
            trace.now_us(),
            json!({"pack": CROSS_PACK, "point": "recall", "mode": "live", "judgment": id,
                   "answered": got.is_some(), "wait_ms": wait.as_millis() as u64}),
        );
        got
    }

    /// Whether the cross judge's own breaker is open: read without a write,
    /// as `rerank_no_wait` reads rerank's, so no call is made and nothing is
    /// waited for.
    fn cross_breaker_open(&self) -> bool {
        self.built.get().is_some_and(|b| {
            b.judge
                .inner()
                .own_breakers()
                .into_iter()
                .any(|(n, s)| n == BREAKER && matches!(s, Status::Open { .. }))
        })
    }

    /// The blocking half: the state, its blob, the ask and the reservation.
    fn prepare_cross(
        &self,
        pack: Arc<Pack>,
        c: &Cross,
        id: String,
        today: &str,
    ) -> Option<(super::Prepared, super::Beside)> {
        let input = CrossInput {
            message: c.message.clone(),
            notes: c.notes.clone(),
        };
        let scrub = ScrubWith(self.scrubber.clone());
        let state = theseus_judge::prepare(&pack, &Input::RecallCross(input), &scrub).ok()?;
        let blob = self.stage_blob(state.state.json.as_bytes());
        let built = self
            .built()
            .map_err(|e| tracing::warn!(error = %format!("{e:#}"), "judge: the Jev client was not built"))
            .ok()?;
        let mut context = json!({
            "session": c.session_id, "turn": c.turn_id, "recall": c.recall_id,
            "purpose": "recall", "baseline": "fused", "blob": blob, "on_path_ms": 0,
        });
        let mode = self.ask_mode(&pack.name(), &mut context);
        let mut ask = Ask::new(pack, &state, mode, context);
        ask.id = Some(id);
        let need = built
            .judge
            .inner()
            .reserve_micros(std::slice::from_ref(&ask))
            .unwrap_or(0);
        let beside = self.reserve_beside(today, need)?;
        Some((super::Prepared { built, ask, need }, beside))
    }
}

/// One request, in its own task: the yes set goes to the waiting turn
/// before the record is written.
async fn judge_cross(
    me: Weak<JudgeService>,
    pack: Arc<Pack>,
    c: Cross,
    id: String,
    tx: oneshot::Sender<BTreeSet<String>>,
) {
    let today = spend::local_day(theseus_protocol::now_unix_ms());
    let Some(svc) = me.upgrade() else { return };
    let day = today.clone();
    let Ok(Some((super::Prepared { built, ask, need }, beside))) =
        tokio::task::spawn_blocking(move || svc.prepare_cross(pack, &c, id, &day)).await
    else {
        return;
    };
    let wrote = beside.spawn(&me);
    let urgency = Urgency::live(Duration::from_secs(5));
    let mut judgments = built
        .judge
        .inner()
        .judge(DecisionPoint {
            asks: vec![ask],
            urgency,
        })
        .await;
    let mut tx = Some(tx);
    for j in &mut judgments {
        let yes = same_subject(j);
        if let Some(o) = j.context.as_object_mut() {
            o.insert(
                "cross".into(),
                json!({"asked": j.questions, "same_subject": yes}),
            );
        }
        if let Some(tx) = tx.take() {
            let _ = tx.send(yes);
        }
        built.judge.sink().record(j);
    }
    if let Some(w) = wrote {
        let _ = w.await;
    }
    let Some(svc) = me.upgrade() else { return };
    for j in &judgments {
        let called = !matches!(j.outcome, Outcome::Skipped { .. });
        let failed = matches!(j.outcome, Outcome::Failed { .. });
        let unknown = matches!(
            j.outcome,
            Outcome::Failed {
                usage_unknown: true,
                ..
            }
        );
        let spent = j.cost_micros.unwrap_or(if unknown { need } else { 0 });
        svc.budget.settle(&today, need, spent, called, failed);
    }
}
