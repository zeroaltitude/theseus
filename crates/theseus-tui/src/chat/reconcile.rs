//! Re-read turn state and transcript after a gap; never resend an uncertain submit.
use super::live::{Live, Phase, TurnLive};
use super::state::{call, Chat, Effect, Purpose};
use super::style::Sty;
use serde_json::{json, Value};
use theseus_protocol::{method, Event, EventsLost, RpcError, SessionHistoryResult};

pub(super) fn live(running: bool, now: u64) -> Live {
    Live {
        // The running id may have started while disconnected. The next session
        // start replaces this marker; any session end clears it.
        turn: running.then(|| TurnLive {
            id: String::new(),
            since_ms: now,
            phase: Phase::Thinking,
        }),
        tools: Vec::new(),
    }
}

impl Chat {
    pub(super) fn recover_session(&mut self, sid: &str) -> Vec<Effect> {
        self.recovering.insert(sid.into());
        vec![call(
            method::SESSION_HISTORY,
            json!({"session_id": sid, "n": super::reopen::REPLAY_NODES}),
            Purpose::Recover(sid.into(), self.recovery_epoch),
        )]
    }

    pub(super) fn recovery_changed(&mut self, event: &Event) {
        if matches!(
            event,
            Event::TurnStarted(_)
                | Event::TurnEnded(_)
                | Event::TurnFailed(_)
                | Event::ConfirmRequested(_)
                | Event::ConfirmResolved(_)
        ) {
            self.recovery_epoch = self.recovery_epoch.wrapping_add(1);
        }
    }

    pub(super) fn recover_lost(&mut self, lost: &EventsLost) -> Vec<Effect> {
        self.recovery_epoch = self.recovery_epoch.wrapping_add(1);
        let mut sessions = Vec::new();
        sessions.extend(self.session.clone());
        sessions.extend(self.main_session());
        sessions.sort();
        sessions.dedup();
        sessions
            .into_iter()
            .filter(|sid| {
                lost.streams.is_empty() || lost.streams.contains(&format!("session:{sid}"))
            })
            .flat_map(|sid| self.recover_session(&sid))
            .collect()
    }

    pub(super) fn recovered(
        &mut self,
        sid: &str,
        epoch: u64,
        result: Result<Value, RpcError>,
    ) -> Vec<Effect> {
        if !self.recovering.contains(sid) {
            return Vec::new();
        }
        if !self.ours(sid) && self.main_session().as_deref() != Some(sid) {
            self.recovering.remove(sid);
            return Vec::new();
        }
        if epoch != self.recovery_epoch {
            return self.recover_session(sid);
        }
        let h = result
            .ok()
            .and_then(|v| serde_json::from_value::<SessionHistoryResult>(v).ok());
        let Some(h) = h.filter(|h| h.session.session_id == sid && h.turn_running.is_some()) else {
            self.say_plain(
                Sty::Yellow,
                "could not reconcile the conversation; queued messages remain unsent",
            );
            return Vec::new();
        };
        let running = h.turn_running == Some(true);
        if self.ours(sid) {
            self.live = live(running, self.now_ms);
            self.released = false;
            self.question = h.pending_confirms.first().cloned();
            self.ask_sync();
            self.history(sid, &h);
        } else {
            self.recovered_away(sid, &h, running);
        }
        self.recovering.remove(sid);
        Vec::new()
    }
}
