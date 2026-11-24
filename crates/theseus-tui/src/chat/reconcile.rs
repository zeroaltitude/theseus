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

    fn recovery_notices(&mut self, sid: &str, h: &SessionHistoryResult, shown: u64) {
        if (h.older.is_some() || h.nodes.len() >= super::reopen::REPLAY_NODES)
            && h.nodes.first().is_some_and(|n| n.position > shown)
        {
            self.say_plain(Sty::Dim, "earlier unseen messages · /history shows them");
            self.earlier = h.older.or_else(|| h.nodes.first().map(|n| n.position));
        }
        if let Some((text, after)) = self.uncertain_submits.remove(sid) {
            if !h
                .nodes
                .iter()
                .any(|n| n.position > after && n.kind == "user_message" && n.text == text)
            {
                self.say_plain(
                    Sty::Dim,
                    "your last message may not have reached the daemon; it was not resent",
                );
            }
        }
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
        if h.as_ref()
            .is_some_and(|h| h.session.session_id == sid && h.turn_running.is_none())
        {
            self.flash("the daemon is older than this chat; restart it to send queued messages");
            return vec![Effect::RecoverLater(sid.into())];
        }
        let Some(h) = h.filter(|h| h.session.session_id == sid && h.turn_running.is_some()) else {
            self.flash(
                "could not reconcile the conversation; retrying before sending queued messages",
            );
            return vec![Effect::RecoverLater(sid.into())];
        };
        let running = h.turn_running == Some(true);
        if self.ours(sid) {
            self.live = live(running, self.now_ms);
            self.released = false;
            self.question = h.pending_confirms.first().cloned();
            self.ask_sync();
            self.note_last_turn(sid, h.nodes.iter().rev().find_map(|n| n.turn_id.clone()));
            // Turn results carry no node position. Resolve the last rendered
            // turn's nodes here. The event-dedup set also contains unshown
            // background replies, so it cannot stand in for this cursor.
            let ended_at = h
                .nodes
                .iter()
                .filter(|n| {
                    n.turn_id
                        .as_ref()
                        .is_some_and(|id| self.shown_turns.get(sid) == Some(id))
                })
                .map(|n| n.position)
                .max()
                .unwrap_or(0);
            let shown = self.shown_nodes.entry(sid.into()).or_default();
            *shown = (*shown).max(ended_at);
            let shown = *shown;
            self.recovery_notices(sid, &h, shown);
            for n in h.nodes.iter().filter(|n| n.position > shown) {
                self.node_line(n);
            }
        } else {
            self.recovered_away(sid, &h, running);
        }
        self.recovering.remove(sid);
        Vec::new()
    }
}
