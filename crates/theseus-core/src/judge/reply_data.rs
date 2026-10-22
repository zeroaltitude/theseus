//! `reply_data.v1` (theseus-a1wvq.15): route.v7's `data` question asked of one
//! reply Theseus wrote, so an open run (what a China-origin model may carry)
//! survives the reply. The turn queues the reply and goes on; this task
//! prepares the state, reserves the judge's day budget beside the call, asks,
//! and writes the answer between turns: the run is extended only by an `open`
//! at the act band from the route pack's own mode, onto a stored run that is
//! still the one the reply was queued against (`data::reply::lands`). Every
//! other outcome, a refused reservation and a failed call included, writes
//! only the `data.reply` row, and the reply stays the end of the run.
//! One judgment a reply: the pack takes one input, and a reply is asked about
//! once.

use std::sync::{Arc, Weak};
use std::time::Duration;

use serde_json::json;
use theseus_judge::builders::InboundInput;
use theseus_judge::{
    Answer, Ask, DecisionPoint, Input, Judge, Judgment, JudgmentSink, Mode, Outcome, Pack, Urgency,
};

use super::inbound::ROUTE_PACK;
use super::{spend, JudgeService, Prepared, ScrubWith};
use crate::config::PackMode;
use crate::data::reply::{lands, ReplyAsk};
use crate::fact::reply_data::DataReply;
use crate::routing::data::{DataAnswer, QUESTION};

/// The pack: the data question about one reply.
pub const PACK: &str = "reply_data.v1";

/// The answer a judgment gives, when it may act: in a mode that acts, as the
/// route pack's `data` answer does.
fn answer_of(j: &Judgment) -> Option<DataAnswer> {
    if !j.actionable() || j.mode == Mode::Shadow {
        return None;
    }
    let a = j.answer(QUESTION)?;
    let Answer::Choice {
        choice, confidence, ..
    } = &a.answer
    else {
        return None;
    };
    Some(DataAnswer {
        choice: choice.clone(),
        confidence: *confidence,
        band: a.band.band,
    })
}

impl JudgeService {
    /// Queue `ask` and return: nothing waits for the answer. The question is
    /// asked only while the route pack acts for the session, as the data
    /// question about a person's message is.
    pub fn ask_reply_data(&self, ask: ReplyAsk) {
        if self.mode_for(ROUTE_PACK, &ask.session).mode < PackMode::Canary || self.breaker_open() {
            return;
        }
        let Some(pack) = self.pack(PACK) else { return };
        let Ok(rt) = tokio::runtime::Handle::try_current() else {
            return;
        };
        rt.spawn(reply_one(self.me.clone(), pack, ask));
    }

    /// The blocking half: the state (the reply scrubbed in `message`), its
    /// blob staged, and the reservation, its frame left beside the call.
    fn prepare_reply(
        &self,
        pack: Arc<Pack>,
        ask: &ReplyAsk,
        today: &str,
    ) -> Option<(Prepared, super::Beside)> {
        let input = InboundInput {
            message: ask.text.clone(),
            author: "assistant".into(),
            place_kind: "reply".into(),
            previous_human_message: None,
            last_reply: None,
            minutes_since_last_message: None,
            live_tasks: vec![],
            live_threads: vec![],
            in_thread: false,
            personas: vec![],
            speaker: None,
            thread_now: None,
            categories: vec![],
            current_role: None,
            roles: vec![],
            data_names: ask.names.clone(),
            onto: None,
        };
        let scrub = ScrubWith(self.scrubber.clone());
        let state = theseus_judge::prepare(&pack, &Input::Inbound(input), &scrub).ok()?;
        let blob = self.stage_blob(state.state.json.as_bytes());
        let built = self
            .built()
            .map_err(|e| tracing::warn!(error = %format!("{e:#}"), "judge: the Jev client was not built"))
            .ok()?;
        let mut context = json!({
            "session": ask.session, "purpose": "reply_data", "node": ask.node,
            "baseline": "none", "blob": blob, "on_path_ms": 0,
        });
        let mode = self.ask_mode(ROUTE_PACK, &mut context);
        let jask = Ask::new(pack, &state, mode, context);
        let need = built
            .judge
            .inner()
            .reserve_micros(std::slice::from_ref(&jask))
            .unwrap_or(0);
        let beside = self.reserve_beside(today, need)?;
        Some((
            Prepared {
                built,
                ask: jask,
                need,
            },
            beside,
        ))
    }

    /// The answer's write, one frame: the `data.reply` row and the session's
    /// record, with the run extended when `lands` says so, under the
    /// session's lock, so a run that moved since the reply was queued is
    /// left alone.
    pub(crate) fn land_reply(
        &self,
        ask: &ReplyAsk,
        answer: Option<&DataAnswer>,
        cost: Option<u64>,
    ) {
        let written = self.store.update_session(&ask.session, |rec| {
            let run = lands(rec.data.as_deref(), ask, answer);
            if let Some(run) = &run {
                rec.data.get_or_insert_with(Default::default).run = Some(run.clone());
            }
            let fact = DataReply {
                node: &ask.node,
                choice: answer.map(|a| a.choice.as_str()),
                band: answer.map(|a| a.band),
                cost_micros: cost,
                extended: run.is_some(),
            };
            Ok(vec![crate::fact::row(&fact, Some(&ask.session), None)?])
        });
        if let Err(e) = written {
            tracing::warn!(error = %format!("{e:#}"), "judge: a reply's data answer was not written");
        }
    }
}

/// One reply's judgment, in its own task. The service is held only around the
/// blocking halves, never across the call or the wait for a quiet moment.
async fn reply_one(me: Weak<JudgeService>, pack: Arc<Pack>, ask: ReplyAsk) {
    let today = spend::local_day(theseus_protocol::now_unix_ms());
    let Some(svc) = me.upgrade() else { return };
    let total = Duration::from_secs(svc.cfg.total_secs);
    let (day, queued) = (today.clone(), ask.clone());
    let prepared = tokio::task::spawn_blocking(move || svc.prepare_reply(pack, &queued, &day))
        .await
        .ok()
        .flatten();
    let Some((
        Prepared {
            built,
            ask: jask,
            need,
        },
        beside,
    )) = prepared
    else {
        return;
    };
    let wrote = beside.spawn(&me);
    let judgments = built
        .judge
        .inner()
        .judge(DecisionPoint {
            asks: vec![jask],
            urgency: Urgency::live(total),
        })
        .await;
    let (mut answer, mut cost) = (None, None);
    for j in judgments {
        answer = answer.or_else(|| answer_of(&j));
        cost = cost.or(j.cost_micros);
        built.judge.sink().record(&j);
        let (called, failed, unknown) = match &j.outcome {
            Outcome::Answered => (true, false, false),
            Outcome::Failed { usage_unknown, .. } => (true, true, *usage_unknown),
            Outcome::Skipped { .. } => (false, false, false),
        };
        let spent = j.cost_micros.unwrap_or(if unknown { need } else { 0 });
        if let Some(svc) = me.upgrade() {
            svc.budget.settle(&today, need, spent, called, failed);
        }
    }
    if let Some(w) = wrote {
        let _ = w.await;
    }
    let (turns, timing) = match me.upgrade() {
        Some(s) => (s.between.get().cloned(), s.sink_timing()),
        None => return,
    };
    let _writing = match turns {
        Some(t) => Some(t.between(tokio::time::Instant::now(), &timing).await),
        None => None,
    };
    let Some(svc) = me.upgrade() else { return };
    let _ = tokio::task::spawn_blocking(move || svc.land_reply(&ask, answer.as_ref(), cost)).await;
}
