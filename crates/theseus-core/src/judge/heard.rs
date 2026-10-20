//! `heard.v1` live (theseus-s3dwj): a voice call's engine asks Jev what a
//! short or unclear transcript was, speech to answer, Theseus's own echo, or
//! noise, when the engine's rules would make a turn of it and it began over
//! Theseus's speech, just after it, or at the join. The engine waits for the
//! verdict at most `[voice] judge_ms`, so the state is built and its blob
//! staged off the worker and the shadow budget's frame is written beside the
//! call (theseus-otny), as at the turn's stop. A verdict the engine did not
//! wait for is still recorded; its judgment is the sink's `judge.call` row,
//! scoped `judge:heard`, with the call's own time.

use std::sync::{Arc, Weak};
use std::time::Instant;

use serde_json::json;
use theseus_judge::band::Top;
pub use theseus_judge::builders::{HeardInput, HeardOverlap};
use theseus_judge::{
    Answer, Ask, DecisionPoint, Input, Judge, Judgment, JudgmentSink, Mode, Outcome, Pack, Urgency,
};
use tokio::sync::oneshot;

use super::{spend, JudgeService, Prepared, ScrubWith};
use crate::config::PackMode;

/// The pack: a short or unclear transcript in a voice call.
pub const PACK: &str = "heard.v1";

/// What the engine reads of `heard.v1`'s verdict.
#[derive(Debug, Clone, PartialEq)]
pub struct HeardVerdict {
    pub judgment: String,
    /// `addressed`, `echo` or `other`.
    pub choice: String,
    /// That option's probability.
    pub p: f64,
    /// The pack's band says act, on an answer from the pinned model, in a
    /// mode that acts: only then may the engine take it over its own rules.
    pub acts: bool,
}

impl HeardVerdict {
    fn of(j: &Judgment) -> Option<Self> {
        let a = j.answer("heard")?;
        let Answer::Choice {
            choice,
            probabilities,
            ..
        } = &a.answer
        else {
            return None;
        };
        let p = probabilities
            .iter()
            .find(|(k, _)| k == choice)
            .map_or(a.band.value, |(_, p)| *p);
        Some(Self {
            judgment: j.id.clone(),
            choice: choice.clone(),
            p,
            acts: j.actionable()
                && j.mode != Mode::Shadow
                && a.band.is_act()
                && a.band.top == Top::Choice(choice.clone()),
        })
    }
}

impl JudgeService {
    /// Ask `heard.v1` about one transcript of `session`'s call. The verdict
    /// comes as soon as Jev answers; `None`: nothing was asked, or Jev gave
    /// none (the pack or the judge off, the day's limit, a failed call).
    /// Dropping the future leaves the call to finish and be recorded.
    pub async fn judge_heard(&self, session: &str, input: HeardInput) -> Option<HeardVerdict> {
        if self.mode_for(PACK, session).mode == PackMode::Off {
            return None;
        }
        let pack = self.pack(PACK)?;
        let rt = tokio::runtime::Handle::try_current().ok()?;
        let (tx, rx) = oneshot::channel();
        rt.spawn(judge_one(
            self.me.clone(),
            pack,
            session.to_string(),
            input,
            tx,
        ));
        rx.await.ok()
    }

    /// The blocking half: the state, its blob staged for the sink, and the
    /// reservation, its frame left beside the call. `None`: nothing to send.
    fn prepare_heard(
        &self,
        pack: Arc<Pack>,
        session: &str,
        input: HeardInput,
        today: &str,
    ) -> Option<(Prepared, super::Beside)> {
        let scrub = ScrubWith(self.scrubber.clone());
        let place = input.place.clone();
        let state = theseus_judge::prepare(&pack, &Input::Heard(input), &scrub).ok()?;
        let blob = self.stage_blob(state.state.json.as_bytes());
        let built = self
            .built()
            .map_err(|e| tracing::warn!(error = %format!("{e:#}"), "judge: the Jev client was not built"))
            .ok()?;
        let mut context = json!({
            "session": session, "purpose": "voice", "baseline": "rules", "blob": blob,
            "place": place, "on_path_ms": 0,
        });
        let mode = self.ask_mode(&pack.name(), &mut context);
        let ask = Ask::new(pack, &state, mode, context);
        let need = built
            .judge
            .inner()
            .reserve_micros(std::slice::from_ref(&ask))
            .unwrap_or(0);
        let beside = self.reserve_beside(today, need)?;
        Some((Prepared { built, ask, need }, beside))
    }
}

/// One transcript's judgment, in its own task. The service is held only
/// around the blocking halves, never across the call. The waiting engine
/// gets the verdict before the row is recorded.
async fn judge_one(
    me: Weak<JudgeService>,
    pack: Arc<Pack>,
    session: String,
    input: HeardInput,
    tx: oneshot::Sender<HeardVerdict>,
) {
    let today = spend::local_day(theseus_protocol::now_unix_ms());
    let Some(svc) = me.upgrade() else { return };
    let total = std::time::Duration::from_secs(svc.cfg.total_secs);
    let day = today.clone();
    let prepared =
        tokio::task::spawn_blocking(move || svc.prepare_heard(pack, &session, input, &day))
            .await
            .ok()
            .flatten();
    let Some((Prepared { built, ask, need }, beside)) = prepared else {
        return;
    };
    let wrote = beside.spawn(&me);
    let t0 = Instant::now();
    let mut judgments = built
        .judge
        .inner()
        .judge(DecisionPoint {
            asks: vec![ask],
            urgency: Urgency::live(total),
        })
        .await;
    let ms = t0.elapsed().as_millis() as u64;
    let mut tx = Some(tx);
    for j in &mut judgments {
        let verdict = HeardVerdict::of(j);
        let taken = match (verdict, tx.take()) {
            (Some(v), Some(tx)) => tx.send(v).is_ok(),
            _ => false,
        };
        if let Some(o) = j.context.as_object_mut() {
            o.insert("heard".into(), json!({"taken": taken, "ms": ms}));
        }
        built.judge.sink().record(j);
    }
    if let Some(w) = wrote {
        let _ = w.await;
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
