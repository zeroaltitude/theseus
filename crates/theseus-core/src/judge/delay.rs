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
    /// The course that ran: the option Jev chose when its band reached the
    /// `course` question's confirm bar, else that question's own declared
    /// `no_match`, read from the pack that was asked
    /// (`theseus_judge::AnswerRecord::ran`, theseus-ig13o). It was the
    /// caller's own hand-copied `NO_MATCH` constant until then, because
    /// nothing on the live path read a declared default. `None` only where the
    /// question declares none, which no pack of ours does
    /// (`every_choice_question_declares_a_no_match`).
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
        if !matches!(course.answer, Answer::Choice { .. }) {
            return None;
        }
        // `ran` reads the confirm band, as `recall_cross.v1` reads its Noul:
        // under the bar the course is the question's own declared default. A
        // conservative band here can only make the system less willing to
        // stop, which is the direction the owner's rule points.
        Some(Self {
            worth_it: *worth_it,
            progressing: *progressing,
            course: course.ran().map(str::to_string),
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
        let beside = self.reserve_beside(today, need, spend::Class::of(ask.mode))?;
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

#[cfg(test)]
mod tests {
    use super::*;

    /// A judgment of `delay.v1` with one `course` answer at `confidence`,
    /// carrying `no_match` as a live answer carries its question's declared
    /// default.
    fn judged(confidence: f64, no_match: Option<&str>) -> Judgment {
        let pack = theseus_judge::pack::by_name(PACK).expect("delay.v1 is embedded");
        let thresholds = pack.question("course").unwrap().thresholds;
        let one = |id: &str, answer: Answer| theseus_judge::judge::AnswerRecord {
            question: id.into(),
            def: id.into(),
            about: None,
            band: theseus_judge::band::band(&answer, thresholds),
            no_match: no_match.map(str::to_string),
            answer,
        };
        let answers = vec![
            one("worth_it", Answer::Noul { noul: 0.9 }),
            one("progressing", Answer::Noul { noul: 0.1 }),
            one(
                "course",
                Answer::Choice {
                    choice: "stop_and_report".into(),
                    probabilities: vec![("stop_and_report".to_string(), confidence)],
                    confidence,
                },
            ),
        ];
        serde_json::from_value(json!({
            "id": "jdg_delay", "pack": PACK, "version": 1, "pack_sha256": "00",
            "point": "delay", "mode": "live", "model": "jev-1.13.0",
            "answered_by": "jev-1.13.0", "model_drift": false,
            "state": {"sha256": "00", "bytes": 10, "tokens": 3, "cap_tokens": 2500,
                "builder": "delay", "builder_version": 1, "truncated": [], "dropped": []},
            "questions": answers.len(), "answers": answers, "call": null,
            "timing": {"queued_ms": 0, "http_ms": 9, "total_ms": 9},
            "usage": null, "cost_micros": 1, "reserve_micros": 1,
            "outcome": {"outcome": "answered"}, "circuit": null, "rate_limit": {},
            "context": {},
        }))
        .unwrap()
    }

    /// The verdict's `course` is what the answer ran: Jev's own option at or
    /// above the bar, and the `course` question's own declared `no_match`
    /// under it (theseus-ig13o). A planted default that is not `keep_going`
    /// proves the value comes from the pack and not from a constant in
    /// `turn::delay_step`, which is where it used to come from.
    #[test]
    fn the_verdict_takes_the_courses_declared_default_under_the_bar() {
        let declared = theseus_judge::pack::by_name(PACK)
            .unwrap()
            .question("course")
            .unwrap()
            .no_match
            .clone()
            .unwrap();
        assert_eq!(declared, "keep_going");
        let sure = DelayVerdict::of(&judged(0.95, Some(&declared))).expect("a verdict");
        assert_eq!(sure.course.as_deref(), Some("stop_and_report"));
        let under = DelayVerdict::of(&judged(0.30, Some(&declared))).expect("a verdict");
        assert_eq!(under.course.as_deref(), Some("keep_going"));
        // The planted one, to show the pack is what is read.
        let planted = DelayVerdict::of(&judged(0.30, Some("narrow_scope"))).expect("a verdict");
        assert_eq!(planted.course.as_deref(), Some("narrow_scope"));
        // A question that declares none leaves nothing to run.
        let bare = DelayVerdict::of(&judged(0.30, None)).expect("a verdict");
        assert_eq!(bare.course, None);
    }
}
