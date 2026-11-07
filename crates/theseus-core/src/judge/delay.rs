//! `delay.v1` live (theseus-5x8ry): the one reconsideration point asks Jev
//! what to do about a delay that survived the free pre-check. The turn waits
//! for the verdict at most `[judge] total_secs`, so the state is built and its
//! blob staged off the worker and the shadow budget's frame is written beside
//! the call (theseus-otny), as `heard.v1` does at a voice turn.
//!
//! The verdict is deliberately small. Jev answers two Nouls and one Choice and
//! **no free text** — `theseus_judge::Answer` is `Choice`, `Score` or `Noul`,
//! so there is no sentence to quote. The notice's words are composed by
//! `turn::delay_step` from these answers and the state's own fields.
//!
//! A `None` here means *the adviser could not answer*, and it cannot say which
//! of five causes it was: the pack off, the judge off, the day's limit
//! reached, the call timed out, or Jev failed. The caller must therefore be
//! correct for all five at once, and it is — see `turn::delay_step`'s
//! no-adviser-no-stop rule.

use std::sync::{Arc, Weak};
use std::time::Instant;

use serde_json::json;
pub use theseus_judge::builders::delay::{DelayInput, DelaySignal};
use theseus_judge::{
    Answer, Ask, DecisionPoint, Input, Judge, Judgment, JudgmentSink, Mode, Outcome, Pack, Urgency,
};
use tokio::sync::oneshot;

use super::{spend, JudgeService, Prepared, ScrubWith};
use crate::config::PackMode;

/// The pack: one delayed piece of work.
pub const PACK: &str = "delay.v1";

/// What the reconsideration point reads of `delay.v1`'s verdict.
#[derive(Debug, Clone, PartialEq)]
pub struct DelayVerdict {
    /// Jev's probability that the work is still worth finishing.
    pub worth_it: f64,
    /// Jev's probability that it is still moving.
    pub progressing: f64,
    /// The chosen course's id, or `None` when the `course` answer was under
    /// its confirm bar. The caller applies its own `NO_MATCH` for `None`:
    /// nothing on the live path reads a pack's declared `no_match`
    /// (theseus-h66v4.6.1), so a declared default would be silently ignored.
    pub course: Option<String>,
    /// The pack's band says the course may be acted on, from the pinned
    /// model, in a mode that acts.
    pub acts: bool,
}

impl DelayVerdict {
    fn of(j: &Judgment) -> Option<Self> {
        let worth = j.answer("worth_it")?;
        let prog = j.answer("progressing")?;
        let course = j.answer("course")?;
        let (Answer::Noul { noul: worth_it }, Answer::Noul { noul: progressing }) =
            (&worth.answer, &prog.answer)
        else {
            return None;
        };
        let Answer::Choice { choice, .. } = &course.answer else {
            return None;
        };
        // The confirm band, as `recall_cross.v1` reads its Noul: under the bar
        // the course is unknown and the caller's own default stands. A
        // conservative band here can only make the system less willing to
        // stop, which is the direction the owner's rule points.
        let sure = course.band.band != theseus_judge::band::Band::Escalate;
        Some(Self {
            worth_it: *worth_it,
            progressing: *progressing,
            course: sure.then(|| choice.clone()),
            acts: j.actionable() && j.mode != Mode::Shadow,
        })
    }
}

impl JudgeService {
    /// Ask `delay.v1` about one delay in `session`. The verdict comes as soon
    /// as Jev answers; `None`: nothing was asked, or Jev gave none (the pack
    /// or the judge off, the day's limit, a timed-out or failed call).
    /// Dropping the future leaves the call to finish and be recorded.
    pub async fn judge_delay(&self, session: &str, input: DelayInput) -> Option<DelayVerdict> {
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
    fn prepare_delay(
        &self,
        pack: Arc<Pack>,
        session: &str,
        input: DelayInput,
        today: &str,
    ) -> Option<(Prepared, super::Beside)> {
        let scrub = ScrubWith(self.scrubber.clone());
        let kind = input.kind.clone();
        let state = theseus_judge::prepare(&pack, &Input::Delay(input), &scrub).ok()?;
        let blob = self.stage_blob(state.state.json.as_bytes());
        let built = self
            .built()
            .map_err(|e| tracing::warn!(error = %format!("{e:#}"), "judge: the Jev client was not built"))
            .ok()?;
        let mut context = json!({
            "session": session, "purpose": "delay", "baseline": "rules", "blob": blob,
            "kind": kind, "on_path_ms": 0,
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

/// One delay's judgment, in its own task. The service is held only around the
/// blocking halves, never across the call.
async fn judge_one(
    me: Weak<JudgeService>,
    pack: Arc<Pack>,
    session: String,
    input: DelayInput,
    tx: oneshot::Sender<DelayVerdict>,
) {
    let today = spend::local_day(theseus_protocol::now_unix_ms());
    let Some(svc) = me.upgrade() else { return };
    let total = std::time::Duration::from_secs(svc.cfg.total_secs);
    let day = today.clone();
    let prepared =
        tokio::task::spawn_blocking(move || svc.prepare_delay(pack, &session, input, &day))
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
        let verdict = DelayVerdict::of(j);
        let taken = match (verdict, tx.take()) {
            (Some(v), Some(tx)) => tx.send(v).is_ok(),
            _ => false,
        };
        if let Some(o) = j.context.as_object_mut() {
            o.insert("delay".into(), json!({"taken": taken, "ms": ms}));
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
