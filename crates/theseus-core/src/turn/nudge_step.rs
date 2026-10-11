//! The stop nudge (theseus-fh0u; the owner's D8): a turn the baseline ends
//! with no tool calls asks `loop.v1` at its stop (`judge/stop.rs`), live,
//! and waits for the verdict at most `judge::stop::WAIT`. When it acts (the
//! last message announced work still to come, `announced_unfinished` in its
//! act band), the turn goes on once: one line in the harness's own voice, a
//! harness node the model reads as the user's (as a wake's), and its
//! `turn.nudged` row, in one frame, then one more model call in the same
//! turn. At most one nudge a turn: a nudged turn that stops again ends, and
//! its end lands `loop.v1`'s `nudge_loop` event for its rollback rule.
//!
//! Never asked: in a shared place (no nudging where others read), with the
//! switch off (`[judge] stop_nudge`, which also holds `loop.v1` in shadow, or
//! the session's own, `session.nudge`), while a job of the turn still runs
//! (its end comes to the turn; the turn is waiting, not stopped), at a
//! strict loop cap, with Jev known unreachable, and with the judge off: then
//! nothing here waits or changes.

use theseus_judge::learn::CanaryEvent;
use theseus_protocol::nudge::TurnNudge;

use super::*;
use crate::judge::stop::{StopVerdict, WAIT};

/// The nudge's author on its node.
pub const NUDGE_AUTHOR: &str = "harness:stop_nudge";

/// The one line the model reads, in the harness's voice.
pub const NUDGE_TEXT: &str = "[The harness: your last message said more work follows, and this \
     turn would end here. Continue, or say that you are done.]";

/// The session's own switch: META `stop_nudge.<session>`, `{"on": false}`
/// to keep its turns from the nudge (`session.nudge`).
pub fn session_key(session_id: &str) -> String {
    format!("stop_nudge.{session_id}")
}

/// What the turn holds of its stop and its nudge.
#[derive(Default)]
pub(super) struct Nudge {
    /// A stop was judged live at its end, so the turn's end does not judge
    /// it again unless the turn went on.
    judged: bool,
    /// The nudge sent: what the result says, the turn's tool calls before
    /// it, and the stopped answer's text.
    sent: Option<(TurnNudge, u32, String)>,
    /// The nudged turn's end was told to the ladder.
    landed: bool,
}

impl Nudge {
    /// The turn's last stop was judged at the stop, live: its end marks and
    /// judges no other.
    pub(super) fn judged_last(&self) -> bool {
        self.judged && self.sent.is_none()
    }

    /// The result's `nudged`: `acted` when the model called a tool after it.
    pub(super) fn result(&self, tool_calls: u32) -> Option<TurnNudge> {
        self.sent.as_ref().map(|(n, before, _)| TurnNudge {
            acted: tool_calls > *before,
            ..n.clone()
        })
    }
}

impl TurnRunner {
    /// After a loop the Advancer ended: true when the turn goes on once more,
    /// nudged. `i` is the loop's index.
    pub(super) async fn stop_nudge(&self, t: &mut Turn<'_>, i: u32) -> Result<bool> {
        if t.nudge.sent.is_some() {
            self.nudged_turn_ended(t);
            return Ok(false);
        }
        if t.stop_reason != "no_tool_calls" || t.nudge.judged || !self.may_nudge(t)? {
            return Ok(false);
        }
        let (sid, turn_id) = (t.tc.session_id, t.tc.turn_id);
        let Some(d) = self.judge.plan_stop(turn_id, sid) else {
            return Ok(false);
        };
        let end = crate::judge::LoopEnd {
            session_id: sid.into(),
            execution_id: t.tc.execution_id.into(),
            turn_id: turn_id.into(),
            task: t.tc.task.is_some(),
            output: t.output.clone(),
            loops: t.loops,
            cost_usd: t.cost,
            tool_calls: t.tool_calls,
        };
        let class = end.class();
        let Some(rx) = self.judge.ask_at_stop(&d, end) else {
            return Ok(false);
        };
        d.mark(&mut t.trace, json!({"loop": i, "class": class}));
        t.nudge.judged = true;
        let t0 = t.trace.now_us();
        let got = wait_for(rx).await;
        let acts = got.as_ref().is_some_and(|v| v.acts);
        t.trace.record(
            "judge",
            "wait",
            t0,
            t.trace.now_us(),
            json!({"pack": d.pack, "point": d.point, "judgment": d.id, "answered": got.is_some(),
                   "acts": acts, "wait_ms": WAIT.as_millis() as u64}),
        );
        let Some(v) = got.filter(|v| v.acts) else {
            return Ok(false);
        };
        let nudge = TurnNudge {
            judgment: v.judgment,
            by: v.by.unwrap_or_default(),
            p: v.p.unwrap_or_default(),
            loop_index: i,
            acted: false,
        };
        // The row and the node, in one frame: the row waits for the node's.
        t.record(&fact::turn::TurnNudged { nudge: &nudge });
        let node = Node::relayed(
            sid,
            Some(turn_id),
            crate::node::Origin::Harness,
            NUDGE_AUTHOR,
            NUDGE_TEXT,
        );
        t.tc.store.append(&[node.record()?])?;
        t.tc.node_written(&node);
        let before = t.last.as_ref().map(|r| r.text.clone()).unwrap_or_default();
        t.nudge.sent = Some((nudge, t.tool_calls, before));
        t.stop_reason.clear();
        Ok(true)
    }

    /// `loop.v1`'s mark at the turn's end (23b), unless the turn's last stop
    /// was judged live at the stop.
    pub(super) fn mark_end(&self, t: &mut Turn<'_>, result: &TurnSubmitResult) {
        let (task, judged) = (t.tc.task.is_some(), t.nudge.judged_last());
        self.judge.mark_turn_end(&mut t.trace, result, task, judged);
    }

    /// Whether this stop may be nudged at all, before anything is asked.
    fn may_nudge(&self, t: &Turn<'_>) -> Result<bool> {
        if t.tc.class == crate::places::PlaceClass::Shared {
            return Ok(false);
        }
        if t.target.max_loops_mode != crate::config::MaxLoopsMode::Notify
            && t.loops >= t.target.max_loops
        {
            return Ok(false);
        }
        if !self.judge.config().stop_nudge || self.judge.jev_unreachable() {
            return Ok(false);
        }
        let own: Option<Value> = t.tc.store.get_meta(&session_key(t.tc.session_id))?;
        if own.is_some_and(|v| v["on"] == false) {
            return Ok(false);
        }
        Ok(self.jobs_outstanding(t.tc.execution_id, &t.background)? == 0)
    }

    /// A nudged turn ended: `loop.v1`'s `nudge_loop` rule counts it (a
    /// nudged turn that made no new call and said the same again rolls the
    /// pack back). Off the worker: the event is a frame of its own.
    fn nudged_turn_ended(&self, t: &mut Turn<'_>) {
        let Some((_, before, said)) = t.nudge.sent.as_ref().filter(|_| !t.nudge.landed) else {
            return;
        };
        t.nudge.landed = true;
        let event = CanaryEvent::NudgedTurnEnded {
            task: t.tc.turn_id.into(),
            new_tool_calls: t.tool_calls.saturating_sub(*before),
            final_before: said.clone(),
            final_after: t.last.as_ref().map(|r| r.text.clone()).unwrap_or_default(),
        };
        let judge = self.judge.clone();
        tokio::task::spawn_blocking(move || judge.land(crate::judge::LOOP_PACK, event));
    }
}

/// The verdict, or none past the wait: one handed over before the wait
/// ends is taken; after it, the judgment finds the turn gone.
async fn wait_for(mut rx: tokio::sync::oneshot::Receiver<StopVerdict>) -> Option<StopVerdict> {
    match tokio::time::timeout(WAIT, &mut rx).await {
        Ok(got) => got.ok(),
        Err(_) => {
            rx.close();
            rx.try_recv().ok()
        }
    }
}
