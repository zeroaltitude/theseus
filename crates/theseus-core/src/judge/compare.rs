//! `compare.v1` (own-benchmark, theseus-pw1q.12): does another model's
//! reply do what a turn's own reply did, for the same request? One Choice,
//! asked of a replay's two replies, awaited by the replay that asks it
//! (`turn.replay`, the nightly sampler). Off every turn, at a point of its
//! own (`replay`); its judgment is the sink's `judge.call` row, scoped
//! `judge:compare`, and its spend the judge's day budget's as every
//! judgment's. A new pack: `WIRED` in shadow, and it has no action, so its
//! verdict reaches the report alone; a version that moved routing would
//! climb the ladder on its frozen holdout, whose baseline is the owner's
//! labels and route corrections.

use serde_json::{json, Value};
use theseus_judge::builders::CompareInput;
use theseus_judge::{Answer, Ask, DecisionPoint, Input, Judge, Judgment, Outcome, Urgency};

use super::{spend, JudgeService, Prepared, ScrubWith};
use crate::config::PackMode;

/// The replay comparison (own-benchmark).
pub const COMPARE_PACK: &str = "compare.v1";

/// A replay is matched when `same` and `better` together reach this: the
/// report's bar (the owner's, in the row's brief).
pub const MATCH_BAR: f64 = 0.8;

/// What Jev said of a replay.
#[derive(Debug, Clone, PartialEq)]
pub struct Compared {
    pub judgment: String,
    /// The pack's mode when it answered.
    pub mode: String,
    /// The option Jev chose: `same`, `better`, `weaker`, `different`, `other`.
    pub verdict: String,
    /// The probability of that option.
    pub p: f64,
    /// `same` and `better` together: what the bar reads.
    pub same_or_better: f64,
    /// What the judgment cost.
    pub cost_usd: f64,
}

/// The chosen option, its probability, and `same` plus `better`.
pub fn read(j: &Judgment) -> Option<(String, f64, f64)> {
    let a = j.answers.iter().find(|a| a.question == "verdict")?;
    let Answer::Choice {
        choice,
        probabilities,
        ..
    } = &a.answer
    else {
        return None;
    };
    let of = |id: &str| {
        probabilities
            .iter()
            .find(|(k, _)| k == id)
            .map_or(0.0, |(_, p)| *p)
    };
    Some((choice.clone(), of(choice), of("same") + of("better")))
}

impl JudgeService {
    /// `compare.v1` on one replay, awaited: Jev's verdict, or why there is
    /// none (the judge or the pack off, the day's budget, no client, a failed
    /// call).
    pub async fn compare_replies(
        &self,
        session: &str,
        turn: &str,
        input: CompareInput,
    ) -> Result<Compared, String> {
        let given = self.mode_for(COMPARE_PACK, session);
        if given.mode == PackMode::Off {
            return Err("the judge or compare.v1 is off".into());
        }
        let Some(pack) = theseus_judge::pack::by_name(COMPARE_PACK) else {
            return Err("this build has no compare.v1".into());
        };
        let today = spend::local_day(theseus_protocol::now_unix_ms());
        let context = json!({"session": session, "turn": turn,
            "purpose": "replay", "baseline": "none", "on_path_ms": 0});
        let me = self.me.clone();
        let day = today.clone();
        let prepared = tokio::task::spawn_blocking(move || {
            me.upgrade()
                .ok_or_else(|| "the judge stopped".to_string())
                .and_then(|svc| svc.prepare_compare(pack, input, context, &day))
        })
        .await
        .map_err(|e| format!("the comparison's state was not built: {e}"))??;
        let Prepared { built, ask, need } = prepared;
        let mode = ask.mode;
        let judgments = built
            .judge
            .judge(DecisionPoint {
                asks: vec![ask],
                urgency: Urgency::Shadow,
            })
            .await;
        for j in &judgments {
            let (called, failed, unknown) = match &j.outcome {
                Outcome::Answered => (true, false, false),
                Outcome::Failed { usage_unknown, .. } => (true, true, *usage_unknown),
                Outcome::Skipped { .. } => (false, false, false),
            };
            let spent = j.cost_micros.unwrap_or(if unknown { need } else { 0 });
            self.budget.settle(&today, need, spent, called, failed);
        }
        let j = judgments
            .into_iter()
            .next()
            .ok_or_else(|| "Jev returned no judgment".to_string())?;
        if let Some(why) = super::rerank::fallback(&j) {
            return Err(format!("Jev did not answer: {why}"));
        }
        let (verdict, p, same_or_better) =
            read(&j).ok_or_else(|| "Jev's answer held no verdict".to_string())?;
        Ok(Compared {
            judgment: j.id.clone(),
            mode: match mode {
                theseus_judge::Mode::Live => "live",
                _ => "shadow",
            }
            .into(),
            verdict,
            p,
            same_or_better,
            cost_usd: j.cost_micros.map_or(0.0, theseus_kernel::micros_to_usd),
        })
    }

    /// The blocking half: the state, its blob, and the reservation.
    fn prepare_compare(
        &self,
        pack: std::sync::Arc<theseus_judge::Pack>,
        input: CompareInput,
        mut context: Value,
        today: &str,
    ) -> Result<Prepared, String> {
        let scrub = ScrubWith(self.scrubber.clone());
        let state = theseus_judge::prepare(&pack, &Input::Compare(input), &scrub)
            .map_err(|e| e.to_string())?;
        let blob = self
            .store
            .blobs()
            .put(state.state.json.as_bytes())
            .map_err(|e| format!("the comparison's state was not written: {e}"))?;
        let built = self
            .built()
            .map_err(|e| format!("the Jev client was not built: {e:#}"))?;
        context["blob"] = json!(blob);
        let mode = self.ask_mode(&pack.name(), &mut context);
        let ask = Ask::new(pack, &state, mode, context);
        let need = built
            .judge
            .inner()
            .reserve_micros(std::slice::from_ref(&ask))
            .unwrap_or(0);
        if !self.reserve(today, need) {
            return Err("the judge's day budget is spent".into());
        }
        Ok(Prepared { built, ask, need })
    }
}
