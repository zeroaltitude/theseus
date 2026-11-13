//! `undo.v1` on the tool gate's own path (theseus-p5kln, row B6
//! `theseus-2yowd`): is this call reversible, and if it is not, what shape
//! should the same work take instead?
//!
//! The pack is `crates/theseus-judge/packs/undo.v1.toml` and its header is
//! the design of the question. `docs/design/undo-v1.md` is the design of the
//! consumption. This module is the dispatch between them: it builds the
//! state, asks once, and hands the gate a [`UndoVerdict`] it can act on.
//!
//! **BLOCKING, UNLIKE EVERY OTHER PACK AT THE GATE.** `security.v3` is asked
//! after the gate has decided and the gate never waits for it
//! (`judge/gate.rs`). This one decides, so the call waits — at most [`WAIT`],
//! which is the pack's own `on_path_p95` rollback bar (300 ms) and not a
//! number chosen here. A verdict that misses the wait acts on nothing; its
//! row still lands, so the learning ledger keeps it.
//!
//! **FAIL OPEN, DELIBERATELY**, as the pack's header says in those words.
//! Every one of these leaves the call to the ordinary order: the judge off,
//! the pack in shadow, the credential unresolved, the breaker open, the call
//! shed, the wait missed, and an answer **under the question's confirm bar**.
//! [`UndoVerdict::of`] is where the last of those is read, and it is the
//! pack's own rule quoted: "when the answer is under the question's confirm
//! bar, the call proceeds under the ordinary order".
//!
//! **WHAT IS NOT RESOLVED FOR v1, and why each is the safe direction.**
//! `remote_keeps` stays `unknown` (`p5kln-remote-keeps`: it is an API call,
//! and the gate's path has none), which the pack reads as keeping nothing, so
//! a remote write reads as a loss and is held rather than run.
//! `read_this_session` stays false (the transcript scan it needs is a page of
//! the session's nodes on the gate's own path, which this row would not buy
//! at 300 ms), so a file this session read reads as one nothing holds — again
//! toward holding. `posture` is empty because 0d runs **before** the order:
//! there is no posture yet, and the field's own comment calls it context only.

use std::sync::{Arc, Weak};
use std::time::{Duration, Instant};

use serde_json::{json, Value};
use theseus_judge::band::{Band, Top};
use theseus_judge::builders::UndoInput;
use theseus_judge::{
    Answer, Ask, DecisionPoint, Input, Judge, Judgment, JudgmentSink as _, Mode, Outcome, Pack,
    Urgency,
};
use tokio::sync::oneshot;

use super::{spend, JudgeService, Prepared, ScrubWith};
use crate::config::PackMode;

/// The pack this layer asks, as the config's switch names it.
pub use crate::config::UNDO_PACK as PACK_NAME;
/// Short, for this module's own reads.
const PACK: &str = PACK_NAME;

#[path = "undo/state.rs"]
pub(crate) mod state;

/// The question whose answer decides, and the one that words the hold.
pub const VERDICT: &str = "reversible";
pub const SHAPE: &str = "shape";

/// The three options of the deciding question, as the pack names them.
pub const REVERSIBLE: &str = "reversible";
pub const RESHAPEABLE: &str = "irreversible_reshapeable";
pub const FINAL: &str = "irreversible_final";

/// The most the gate waits for the verdict: the pack's own `on_path_p95`
/// rollback bar. A verdict later than this acts on nothing.
pub const WAIT: Duration = Duration::from_millis(300);

/// What the pack said about one call, once the fail-open rule has been
/// applied: [`Says::Reversible`] is also what a non-answer comes to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Says {
    /// Nothing is lost, or what is lost can be put back: the call goes into
    /// the ordinary order and the gate is the gate it was in 1.0.4.
    Reversible,
    /// Something would be lost and the same work has a shape that keeps a
    /// way back: the call is held and the model re-plans it.
    Reshapeable,
    /// Something would be lost and no shape of the same call keeps a way
    /// back: the gate's posture is raised to `approve`.
    Final,
}

/// One call's verdict as the gate reads it.
#[derive(Debug, Clone, PartialEq)]
pub struct UndoVerdict {
    pub judgment: String,
    pub says: Says,
    /// The shape the pack chose for a call that loses something, at or above
    /// the `shape` question's own confirm bar; `none` and a weaker answer
    /// are both `None`. The words are the pack's, not this module's.
    pub shape: Option<String>,
    /// The deciding answer's confidence.
    pub p: f64,
}

impl UndoVerdict {
    /// The verdict a judgment says. `None`: nothing to act on — the judgment
    /// is in shadow, the pack may not be acted on, the deciding question was
    /// not answered, or its answer is under the question's confirm bar (the
    /// pack's fail-open rule). A `reversible` answer gives `Some`, because
    /// the row the gate keeps should say the pack was asked and said so.
    pub fn of(pack: &Pack, j: &Judgment) -> Option<Self> {
        if !j.actionable() || j.mode == Mode::Shadow {
            return None;
        }
        let a = j.answer(VERDICT)?;
        let Answer::Choice { choice, .. } = &a.answer else {
            return None;
        };
        if a.band.top != Top::Choice(choice.clone()) {
            return None;
        }
        // The pack's own fail-open rule: under the question's confirm bar
        // the call proceeds under the ordinary order. `band` is Escalate
        // exactly there, so this is that rule and not a second threshold.
        if a.band.band == Band::Escalate {
            return None;
        }
        let says = match choice.as_str() {
            RESHAPEABLE => Says::Reshapeable,
            FINAL => Says::Final,
            // `no_match` is `reversible`, for the same reason.
            _ => Says::Reversible,
        };
        Some(Self {
            judgment: j.id.clone(),
            says,
            shape: shape_of(pack, j),
            p: a.band.value,
        })
    }

    /// Whether the call is held here, before `order` runs (consumption point
    /// one, `docs/design/undo-v1.md`).
    pub fn holds(&self) -> bool {
        self.says == Says::Reshapeable
    }

    /// Whether the decision `order` returned is raised to `approve`
    /// (consumption point two).
    pub fn raises(&self) -> bool {
        self.says == Says::Final
    }

    /// The pack's own sentence for the shape it chose, for the hold's words.
    /// None when it chose none, or when the pack has no such option.
    pub fn shape_means(&self, pack: &Pack) -> Option<String> {
        let id = self.shape.as_deref()?;
        let q = pack.question(SHAPE)?;
        q.options
            .iter()
            .find(|o| o.id == id)
            .and_then(|o| o.means.clone())
    }
}

/// The shape the pack chose, at or above the `shape` question's confirm bar;
/// `none` is no shape.
fn shape_of(pack: &Pack, j: &Judgment) -> Option<String> {
    let a = j.answer(SHAPE)?;
    let Answer::Choice { choice, .. } = &a.answer else {
        return None;
    };
    if a.band.band == Band::Escalate || choice == "none" {
        return None;
    }
    pack.question(SHAPE)?
        .options
        .iter()
        .any(|o| &o.id == choice)
        .then(|| choice.clone())
}

impl JudgeService {
    /// Whether the gate asks this pack at all: false with the judge off or
    /// the pack switched off, so the gate's path reads nothing more and
    /// never builds a state. `[judge] reversibility = false` leaves it on
    /// here and in shadow, which is the point of that switch: the judgment
    /// is still made and recorded, and `UndoVerdict::of` returns `None`.
    pub fn undo_on(&self, session: &str) -> bool {
        self.mode_for(&self.placed(PACK, session), session).on()
    }

    /// Ask `undo.v1` about one planned call, and wait at most [`WAIT`] for
    /// the answer. `None`: nothing was asked, Jev gave no answer in time, or
    /// the answer fails open. The call is left to finish and be recorded
    /// either way.
    pub async fn judge_undo(&self, call: state::UndoCall) -> Option<UndoVerdict> {
        let session = call.session_id.clone();
        if !self.undo_on(&session) {
            return None;
        }
        let pack = self.pack(&self.placed(PACK, &session))?;
        let rt = tokio::runtime::Handle::try_current().ok()?;
        let (tx, rx) = oneshot::channel();
        rt.spawn(judge_one(self.me.clone(), pack, call, tx));
        tokio::time::timeout(WAIT, rx).await.ok()?.ok()
    }

    /// The blocking half: the tree probed for each target's recovery word,
    /// the state built under the pack's cap, its blob staged for the sink,
    /// and the reservation, its frame left beside the call. `None`: nothing
    /// to send.
    fn prepare_undo(
        &self,
        pack: Arc<Pack>,
        call: &state::UndoCall,
        today: &str,
    ) -> Option<(Prepared, super::Beside)> {
        let mut probe = theseus_judge::builders::GitCli::default();
        let input: UndoInput = state::input_of(call, &mut probe);
        let scrub = ScrubWith(self.scrubber.clone());
        let state = theseus_judge::prepare(&pack, &Input::Undo(input), &scrub).ok()?;
        let blob = self.stage_blob(state.state.json.as_bytes());
        let built = self
            .built()
            .map_err(|e| tracing::warn!(error = %format!("{e:#}"), "judge: the Jev client was not built"))
            .ok()?;
        let mut context = json!({
            "session": call.session_id, "turn": call.turn_id, "tool": call.tool,
            "baseline": "posture", "blob": blob, "point": "gate",
            "wait_ms": WAIT.as_millis() as u64,
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

/// One call's judgment, in its own task. The service is held only around the
/// blocking halves, never across the call. The waiting gate gets the verdict
/// before the row is recorded; the row says whether it took it.
async fn judge_one(
    me: Weak<JudgeService>,
    pack: Arc<Pack>,
    call: state::UndoCall,
    tx: oneshot::Sender<UndoVerdict>,
) {
    let today = spend::local_day(theseus_protocol::now_unix_ms());
    let Some(svc) = me.upgrade() else { return };
    let total = Duration::from_secs(svc.cfg.total_secs);
    let day = today.clone();
    let prepared = tokio::task::spawn_blocking(move || svc.prepare_undo(pack, &call, &day))
        .await
        .ok()
        .flatten();
    let Some((Prepared { built, ask, need }, beside)) = prepared else {
        return;
    };
    let wrote = beside.spawn(&me);
    let pack = ask.pack.clone();
    let t0 = Instant::now();
    // The call runs to its own deadline past the gate's wait, so a late
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
    let mut tx = Some(tx);
    for j in &mut judgments {
        let verdict = UndoVerdict::of(&pack, j);
        let says = verdict.as_ref().map(|v| says_str(v.says));
        let taken = match (verdict, tx.take()) {
            (Some(v), Some(tx)) => tx.send(v).is_ok(),
            _ => false,
        };
        if let Some(o) = j.context.as_object_mut() {
            o.insert(
                "undo".into(),
                json!({"says": says, "taken": taken, "ms": ms}),
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

/// The verdict's word, for a row and for a judgment's context.
pub fn says_str(s: Says) -> &'static str {
    match s {
        Says::Reversible => REVERSIBLE,
        Says::Reshapeable => RESHAPEABLE,
        Says::Final => FINAL,
    }
}

/// Whether the pack reads as off for the whole daemon, for health's line.
pub fn off(mode: PackMode) -> bool {
    mode == PackMode::Off
}

/// A `Value` that is not an object or is empty reads as no other arguments.
pub(crate) fn other_args(v: &Value) -> Option<Value> {
    match v {
        Value::Object(m) if m.is_empty() => None,
        Value::Null => None,
        other => Some(other.clone()),
    }
}

#[cfg(test)]
#[path = "undo/tests.rs"]
mod tests;
