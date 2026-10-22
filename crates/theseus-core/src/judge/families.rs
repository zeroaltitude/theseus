//! `families.v1` (theseus-xlri5): Jev reads the message in focus and the tool
//! families its request hides, and says of each whether answering the message
//! needs one of its tools. A model without the provider's tool search gets the
//! families Jev says yes about loaded with the request, past the place's budget
//! under the ceiling (`tools_defer::want`); the turn takes the answer, bounded,
//! before it settles its tools (`turn/families_step.rs`).
//!
//! - **One request a prompt**, started at the inbound point beside the turn's
//!   other work, in a task of its own: the state, its blob, the reservation, the
//!   call and the record are the task's, as the cross-stream check's are. Its
//!   cost is in the `judge.call` row the sink writes (context `purpose:
//!   "families"`).
//! - **A yes** is a family's Noul true at the question's confirm bar or above,
//!   in a judgment that acts (canary or live), with the Noul as its confidence.
//!   A timeout, a failure, the day's budget spent, or Jev known unreachable
//!   gives none, and the named rule works alone.

use std::sync::{Arc, Weak};
use std::time::Duration;

use serde_json::json;
use theseus_judge::band::{Band, Top};
use theseus_judge::builders::{FamiliesInput, FamilyItem};
use theseus_judge::{
    Answer, Ask, DecisionPoint, Input, Judge, Judgment, JudgmentSink, Mode, Outcome, Pack, Urgency,
};
use tokio::sync::oneshot;

use super::{spend, JudgeService, ScrubWith};
use crate::config::PackMode;
use crate::trace::Trace;

/// The pack (theseus-xlri5).
pub const FAMILIES_PACK: &str = "families.v1";

/// A family Jev judged relevant, and the Noul it said it with.
pub type Relevant = Vec<(String, f64)>;

/// What the turn asks about.
pub struct Families {
    pub session_id: String,
    pub turn_id: String,
    pub message: String,
    pub families: Vec<FamilyItem>,
}

/// The families Jev said answering the message needs, with their Nouls.
pub fn relevant(j: &Judgment) -> Relevant {
    if !j.actionable() || j.mode == Mode::Shadow {
        return Vec::new();
    }
    j.answers
        .iter()
        .filter(|a| a.band.band != Band::Escalate && a.band.top == Top::Noul(true))
        .filter_map(|a| match a.answer {
            Answer::Noul { noul } => Some((a.about.clone()?, noul)),
            _ => None,
        })
        .collect()
}

impl JudgeService {
    /// Start `families.v1` about `f`'s families and hand back where its yes set
    /// lands; none when there is nothing to ask, the pack is off, or Jev is
    /// known unreachable. The turn's trace gets the judgment's mark.
    pub fn families_start(
        &self,
        trace: &mut Trace,
        f: Families,
    ) -> Option<oneshot::Receiver<Relevant>> {
        if f.families.is_empty()
            || self.mode_for(FAMILIES_PACK, &f.session_id).mode == PackMode::Off
        {
            return None;
        }
        if self.jev_unreachable() {
            return None;
        }
        let pack = theseus_judge::pack::by_name(FAMILIES_PACK)?;
        let rt = tokio::runtime::Handle::try_current().ok()?;
        let id = theseus_judge::judge::new_id();
        trace.mark(
            "judge",
            "mark",
            json!({"pack": FAMILIES_PACK, "point": "inbound", "mode": "live", "judgment": id}),
        );
        let (tx, rx) = oneshot::channel();
        rt.spawn(judge_families(self.me.clone(), pack, f, id, tx));
        Some(rx)
    }

    /// The blocking half: the state, its blob, the ask and the reservation.
    fn prepare_families(
        &self,
        pack: Arc<Pack>,
        f: &Families,
        id: String,
        today: &str,
    ) -> Option<(super::Prepared, super::Beside)> {
        let input = FamiliesInput {
            message: f.message.clone(),
            families: f.families.clone(),
        };
        let scrub = ScrubWith(self.scrubber.clone());
        let state = theseus_judge::prepare(&pack, &Input::Families(input), &scrub).ok()?;
        let blob = self.stage_blob(state.state.json.as_bytes());
        let built = self
            .built()
            .map_err(|e| tracing::warn!(error = %format!("{e:#}"), "judge: the Jev client was not built"))
            .ok()?;
        let mut context = json!({
            "session": f.session_id, "turn": f.turn_id,
            "purpose": "families", "baseline": "none", "blob": blob, "on_path_ms": 0,
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

/// One request, in its own task: the yes set goes to the waiting turn before
/// the record is written.
async fn judge_families(
    me: Weak<JudgeService>,
    pack: Arc<Pack>,
    f: Families,
    id: String,
    tx: oneshot::Sender<Relevant>,
) {
    let today = spend::local_day(theseus_protocol::now_unix_ms());
    let Some(svc) = me.upgrade() else { return };
    let day = today.clone();
    let Ok(Some((super::Prepared { built, ask, need }, beside))) =
        tokio::task::spawn_blocking(move || svc.prepare_families(pack, &f, id, &day)).await
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
        let yes = relevant(j);
        if let Some(o) = j.context.as_object_mut() {
            let names: Vec<&str> = yes.iter().map(|(f, _)| f.as_str()).collect();
            o.insert(
                "families".into(),
                json!({"asked": j.questions, "relevant": names}),
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
