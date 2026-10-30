//! A background job's ask of Jev (route.v10 row 4, theseus-ej6bj; design §2's
//! closing paragraph): the acting route pack's `model` question, about a job
//! that is no turn: a compaction's summary, a synthesis, the self builder or
//! reviewer, a card, a note, a name. The state is `builders::job`'s (one
//! paragraph, the place `job`, the route facts), so every site asks the same
//! question the same way.
//!
//! - **Bounded.** The caller waits at most `[routing] max_wait_ms` (300 ms in
//!   the design) and a job that gets no answer in that time runs on `live`:
//!   it never waits on Jev longer than that, because a job that hangs on Jev
//!   is a worse failure than one that picks the default. The ask goes on in a
//!   task of its own, is recorded and settled as any judgment is, and a late
//!   answer moves nothing.
//! - **One pack.** The acting route pack, when it asks `model` (route.v10's;
//!   every pack before it does not, and then there is no ask and the site
//!   runs its legacy profile, `config::legacy`).
//! - **Recorded** as a judgment of the route pack with the context
//!   `purpose: "job"`, so `judge.call` rows and the learning loop see it.

use std::sync::{Arc, Weak};
use std::time::Duration;

use serde_json::json;
use theseus_judge::band::Band;
use theseus_judge::builders::{job_input, JobInput, JobKind, RestrictedModels, RouteFacts};
use theseus_judge::{
    Answer, Ask, DecisionPoint, Input, Judge, Judgment, Mode, Outcome, Pack, Urgency,
};
use tokio::sync::oneshot;

use super::inbound::ROUTE_PACK;
use super::{spend, JudgeService, ScrubWith};
use crate::catalog::price_period_at;
use crate::config::PackMode;

/// What a job's ask came to.
#[derive(Debug, Clone, PartialEq)]
pub(crate) enum JobAnswer {
    /// Jev named an option at or above the pack's confirm bar.
    Picked { option: String, confidence: f64 },
    /// It answered, under the confirm bar (the pack's `no_match` is Sonnet), or
    /// in a judgment that does not act.
    Unsure,
    /// No answer in time, or none at all (a failure, the day's limit).
    Late,
    /// Not asked: routing or the judge is off, Jev cannot be reached, or the
    /// acting pack asks no `model`.
    Unasked,
}

impl JudgeService {
    /// A test's route pack that asks `model`, standing in for the acting one
    /// everywhere (`stand_in`, named route.v8 as the daemon compares it), so the
    /// background sites and the turn path see one switch, `route_open`.
    #[cfg(test)]
    pub(crate) fn job_pack_with(&self, pack: Arc<Pack>) {
        let mut p = (*pack).clone();
        p.version = 8;
        self.stand_in(ROUTE_PACK, p);
    }

    /// Ask which model `kind` should run on, and wait at most `wait` for the
    /// answer. `restricted` is what the session's labels allow; private
    /// material is floored at `not_allowed` by the job builder whatever it
    /// says. `mode` is the route pack's mode (`[routing]` over the ladder):
    /// nothing is asked below a canary.
    pub(crate) async fn job_ask(
        &self,
        kind: JobKind,
        restricted: RestrictedModels,
        mode: PackMode,
        wait: Duration,
    ) -> JobAnswer {
        let Some(pack) = self.route_pack() else {
            return JobAnswer::Unasked;
        };
        if mode < PackMode::Canary
            || !self.cfg.enabled
            || self.jev_unreachable()
            || !self.reachable()
        {
            return JobAnswer::Unasked;
        }
        let Ok(rt) = tokio::runtime::Handle::try_current() else {
            return JobAnswer::Unasked;
        };
        let facts = RouteFacts {
            price_period: price_period_at(theseus_protocol::now_unix_ms()),
            restricted_models: restricted,
            last_reply_model: None,
            task_models: Vec::new(),
            // A job has no conversation to estimate (the field is left out of its state).
            est_tokens: None,
        };
        let (tx, rx) = oneshot::channel();
        rt.spawn(judge_job(
            self.me.clone(),
            pack,
            JobInput { kind, facts },
            theseus_judge::judge::new_id(),
            tx,
        ));
        match tokio::time::timeout(wait, rx).await {
            Ok(Ok(answer)) => answer,
            // A sender dropped with nothing sent, or no answer by the bound.
            Ok(Err(_)) | Err(_) => JobAnswer::Late,
        }
    }

    /// The blocking half: the state, its blob, the ask and the reservation.
    fn prepare_job(
        &self,
        pack: Arc<Pack>,
        job: &JobInput,
        id: String,
        today: &str,
    ) -> Option<(super::Prepared, super::Beside)> {
        let scrub = ScrubWith(self.scrubber.clone());
        let state = theseus_judge::prepare(&pack, &Input::Inbound(job_input(job)), &scrub).ok()?;
        let blob = self.stage_blob(state.state.json.as_bytes());
        let built = self
            .built()
            .map_err(|e| tracing::warn!(error = %format!("{e:#}"), "judge: the Jev client was not built"))
            .ok()?;
        let mut context = json!({
            "session": "", "turn": id, "purpose": "job", "baseline": "live",
            "place_kind": "job", "blob": blob, "on_path_ms": 0, "class": "job",
        });
        // The acting route pack's mode, by its name, as the turn's ask takes it
        // (the pack that asks `model` is the acting one once its row flips it).
        let mode = self.ask_mode(ROUTE_PACK, &mut context);
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

/// The route pack's `model` answer from its judgment.
fn picked(j: &Judgment) -> JobAnswer {
    if !j.actionable() || j.mode == Mode::Shadow {
        return JobAnswer::Unsure;
    }
    let Some(a) = j.answer("model") else {
        return JobAnswer::Unsure;
    };
    match &a.answer {
        Answer::Choice {
            choice, confidence, ..
        } if a.band.band != Band::Escalate => JobAnswer::Picked {
            option: choice.clone(),
            confidence: *confidence,
        },
        _ => JobAnswer::Unsure,
    }
}

/// One job's request, in its own task: the answer goes to the waiting job as
/// the call lands, and the reservation settles after, as every judgment's does.
async fn judge_job(
    me: Weak<JudgeService>,
    pack: Arc<Pack>,
    job: JobInput,
    id: String,
    tx: oneshot::Sender<JobAnswer>,
) {
    let today = spend::local_day(theseus_protocol::now_unix_ms());
    let Some(svc) = me.upgrade() else { return };
    let (total, day) = (svc.cfg.total_secs, today.clone());
    let Ok(Some((super::Prepared { built, ask, need }, beside))) =
        tokio::task::spawn_blocking(move || svc.prepare_job(pack, &job, id, &day)).await
    else {
        return;
    };
    let wrote = beside.spawn(&me);
    let judgments = built
        .judge
        .judge(DecisionPoint {
            asks: vec![ask],
            urgency: Urgency::live(Duration::from_secs(total)),
        })
        .await;
    // An answer the call did not give (a failure, a skip) is no answer: the
    // sender is dropped here, before the reservation settles below, so the
    // waiting job runs on `live` at once and not when this task ends.
    match judgments.first().filter(|j| j.outcome == Outcome::Answered) {
        Some(j) => {
            let _ = tx.send(picked(j));
        }
        None => drop(tx),
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
