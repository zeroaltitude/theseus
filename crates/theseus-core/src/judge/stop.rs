//! `loop.v1` live at a turn's stop (theseus-fh0u; the owner's D8: tasks,
//! conversations and one-shot `ask`, live, at most one nudge a turn, with a
//! switch). The turn that the baseline ends with no tool calls asks here,
//! before its end, and waits for the verdict at most [`WAIT`]: the state is
//! built and its blob staged off the worker, the shadow budget's frame is
//! written beside the call (theseus-otny), and the verdict comes back over a
//! oneshot the moment Jev answers, before its row is recorded. A verdict the
//! turn did not take in time acts on nothing; its row says so
//! (`context.stop.taken`). The nudge itself is the turn's
//! (`turn/nudge_step.rs`).
//!
//! A turn's stop judged here is not judged again after the turn
//! (`JudgeService::after_turn` reads the mark's point, [`POINT`]); every
//! judgment taken after a turn is recorded in shadow, since nothing acts
//! on it then.

use std::sync::{Arc, Weak};
use std::time::{Duration, Instant};

use serde_json::json;
use theseus_judge::{
    decide, Ask, DecisionPoint, Input, Judge, Judgment, JudgmentSink, Mode, Outcome, Pack, Urgency,
    Verdict,
};
use tokio::sync::oneshot;

use super::{spend, Dispatch, JudgeService, LoopEnd, Prepared, ScrubWith};

/// The most a turn waits at its stop for the verdict: `loop.v1`'s own
/// on-path bar (its `on_path_p95` rule, 1,000 ms). A verdict later than it
/// nudges nothing.
pub const WAIT: Duration = Duration::from_millis(1_000);

/// The mark's point for a stop judged live, so the turn's end does not
/// judge it again.
pub const POINT: &str = "stop";

/// What the turn reads of `loop.v1`'s verdict at its stop.
#[derive(Debug, Clone, PartialEq)]
pub struct StopVerdict {
    pub judgment: String,
    /// The pack's deciding questions say act (`announced_unfinished`
    /// act-true), on an answer from the pinned model.
    pub acts: bool,
    /// The question that decided, and its p.
    pub by: Option<String>,
    pub p: Option<f64>,
}

impl StopVerdict {
    /// The verdict a judgment says, by the pack's decision (§2.4): only a
    /// judgment the core may act on (`actionable`) can act.
    pub fn of(pack: &Pack, j: &Judgment) -> Self {
        let d = decide(pack, &j.answers);
        Self {
            judgment: j.id.clone(),
            acts: j.actionable() && d.verdict == Verdict::Act,
            by: d.by,
            p: d.value,
        }
    }
}

impl JudgeService {
    /// `loop.v1`'s dispatch at this stop when it acts here (live, or a
    /// canary's arm), its id minted now; `None`: it judges in shadow after
    /// the turn, or not at all.
    pub fn plan_stop(&self, turn_id: &str, session_id: &str) -> Option<Dispatch> {
        let mut d = self.plan_loop_end("no_tool_calls", turn_id, session_id)?;
        if d.mode == Mode::Shadow {
            return None;
        }
        d.point = POINT.into();
        Some(d)
    }

    /// Ask `loop.v1` about this stop now, as `d` marked it: the verdict comes
    /// over the receiver as soon as Jev answers. `None`: nothing to ask.
    pub fn ask_at_stop(
        &self,
        d: &Dispatch,
        end: LoopEnd,
    ) -> Option<oneshot::Receiver<StopVerdict>> {
        let pack = self.pack(&d.pack)?;
        let rt = tokio::runtime::Handle::try_current().ok()?;
        let (tx, rx) = oneshot::channel();
        rt.spawn(judge_stop(self.me.clone(), pack, end, d.id.clone(), tx));
        Some(rx)
    }

    /// The blocking half: the state from the session's nodes, its blob
    /// staged for the sink, and the reservation, its frame left beside the
    /// call. `None`: nothing to send.
    fn prepare_stop(
        &self,
        pack: Arc<Pack>,
        end: &LoopEnd,
        id: String,
        today: &str,
    ) -> Option<(Prepared, super::Beside)> {
        let nodes: Vec<_> = self
            .store
            .session_nodes(&end.session_id)
            .map_err(|e| tracing::warn!(error = %format!("{e:#}"), "judge: the session's nodes were not read"))
            .ok()?
            .into_iter()
            .map(|(_, n)| n)
            .collect();
        let input = super::loop_end::input(&nodes, end, theseus_protocol::now_unix_ms());
        let scrub = ScrubWith(self.scrubber.clone());
        let state = theseus_judge::prepare(&pack, &Input::Loop(input), &scrub).ok()?;
        let blob = self.stage_blob(state.state.json.as_bytes());
        let built = self
            .built()
            .map_err(|e| tracing::warn!(error = %format!("{e:#}"), "judge: the Jev client was not built"))
            .ok()?;
        let mut context = json!({
            "session": end.session_id, "execution": end.execution_id, "turn": end.turn_id,
            "loops": end.loops, "baseline": "until_no_tool_calls", "decision": "no_tool_calls",
            "class": end.class(), "blob": blob, "point": POINT,
            "wait_ms": WAIT.as_millis() as u64,
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
        Some((Prepared { built, ask, need }, beside))
    }
}

/// One stop's judgment, in its own task. The service is held only around
/// the blocking halves, never across the call. The waiting turn gets the
/// verdict before the row is recorded; the row says whether it took it.
async fn judge_stop(
    me: Weak<JudgeService>,
    pack: Arc<Pack>,
    end: LoopEnd,
    id: String,
    tx: oneshot::Sender<StopVerdict>,
) {
    let today = spend::local_day(theseus_protocol::now_unix_ms());
    let Some(svc) = me.upgrade() else { return };
    let total = Duration::from_secs(svc.cfg.total_secs);
    let day = today.clone();
    let prepared = tokio::task::spawn_blocking(move || svc.prepare_stop(pack, &end, id, &day))
        .await
        .ok()
        .flatten();
    let Some((Prepared { built, ask, need }, beside)) = prepared else {
        return;
    };
    let wrote = beside.spawn(&me);
    let pack = ask.pack.clone();
    let t0 = Instant::now();
    // The call runs to its own deadline past the turn's wait, so a late
    // verdict is still recorded for the learning ledger.
    let mut judgments = built
        .judge
        .inner()
        .judge(DecisionPoint {
            asks: vec![ask],
            urgency: Urgency::live(total),
        })
        .await;
    let ms = t0.elapsed().as_millis() as u64;
    tracing::debug!(ms, "judge: loop.v1 judged at a stop");
    let mut tx = Some(tx);
    for j in &mut judgments {
        let v = StopVerdict::of(&pack, j);
        let acts = v.acts;
        let taken = tx.take().is_some_and(|tx| tx.send(v).is_ok());
        if let Some(o) = j.context.as_object_mut() {
            o.insert(
                "stop".into(),
                json!({"acts": acts, "taken": taken, "ms": ms}),
            );
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
