//! Where the router placed a message (theseus-wapgp, theseus-0rfjx).
//!
//! A message that names a thread, sent from a place that has threads, runs
//! in that thread's session: `→ thread <title>`, and how to go there. The
//! router's question ("which thread?") and its keys are gone: a message with
//! no address runs at home (the owner, 2026-11-21).

use theseus_protocol::thread_route::ThreadTag;
use theseus_protocol::{Event, ThreadPlaced, TurnSubmitResult};

use super::state::{Chat, Effect};
use super::style::Sty;

impl Chat {
    /// The turn ran in a thread its address named: say so, and how to go there.
    pub(super) fn placed_line(&mut self, tag: &ThreadTag, session_id: &str) {
        let ThreadTag {
            slug: Some(_),
            title,
            movable,
            ..
        } = tag
        else {
            return;
        };
        self.say_plain(Sty::Cyan, &format!("→ thread {title}"));
        self.say_words(Sty::Dim, &format!("  go there: theseus chat {session_id}"));
        if *movable {
            self.say_words(Sty::Dim, "  say \"that was for <thread>\" to move it");
        }
    }

    /// Whether an event of `session_id` is this chat's: its own session's,
    /// or the session of a turn the router placed for it (theseus-umb8a).
    /// Another turn of that session (one sent from Discord, say) is not: an
    /// event that names its turn must name one of ours.
    pub(super) fn ours_in(&self, session_id: &str, turn_id: Option<&str>) -> bool {
        self.ours(session_id)
            || (self.placed.also.iter().any(|s| s == session_id)
                && turn_id.is_none_or(|t| self.mine.contains(t)))
    }

    /// The daemon placed this chat's message in a thread: say so now, and
    /// take the turn and its session in as the chat's own.
    pub(super) fn thread_placed(&mut self, p: &ThreadPlaced) -> Vec<Effect> {
        if self.placed.told.len() > ALSO * 8 {
            self.placed.told.clear();
        }
        self.placed.told.insert(p.turn_id.clone());
        self.placed_line(&p.tag, &p.placed);
        self.adopt(&p.placed, &p.turn_id)
    }

    /// Where the router placed this chat's message.
    pub(super) fn thread_event(&mut self, e: Event) -> Vec<Effect> {
        match e {
            Event::ThreadPlaced(p) if self.ours(&p.session_id) => self.thread_placed(&p),
            _ => Vec::new(),
        }
    }

    /// A submit's answer: the turn's end, and the placed session's tasks
    /// taken in if the chat was not told of the placement.
    pub(super) fn submit_ended(&mut self, r: &TurnSubmitResult) -> Vec<Effect> {
        let adopted = self.adopt_result(r);
        self.turn_ended(r);
        adopted
    }

    /// A submit's answer for a turn placed in a thread the chat was not told
    /// of (an older daemon): take it in the same way.
    pub(super) fn adopt_result(&mut self, r: &TurnSubmitResult) -> Vec<Effect> {
        let placed = r.thread.as_ref().is_some_and(|t| t.slug.is_some());
        if !placed || self.ours(&r.session_id) || self.placed.told.contains(&r.turn_id) {
            return Vec::new();
        }
        self.adopt(&r.session_id, &r.turn_id)
    }

    /// A question of a placed session's, which this chat's turn raised: the
    /// answer will start a continuation turn there (theseus-nde5n).
    pub(super) fn placed_asked(&mut self, c: &theseus_protocol::ConfirmRequest) {
        let ours = c.turn_id.as_ref().is_some_and(|t| self.mine.contains(t));
        if ours && self.placed.also.contains(&c.session_id) {
            self.placed
                .owed
                .insert(c.correlation_id.clone(), c.session_id.clone());
        }
    }

    /// A question closed with no answer (a stop, new input over it) starts
    /// no continuation: nothing is owed for it. An answer or an expiry wakes
    /// the execution, so those keep their mark for the continuation.
    pub(super) fn placed_closed(&mut self, r: &theseus_protocol::ConfirmResolved) {
        if r.option.is_none() && !r.expired {
            self.placed.owed.remove(&r.correlation_id);
        }
    }

    /// The continuation that goes on after an answered question of a placed
    /// session is this chat's turn too, and only that one: a turn another
    /// surface sent there is not a continuation (theseus-nde5n).
    pub(super) fn placed_continues(&mut self, e: &Event) {
        if let Event::TurnStarted(t) = e {
            let due = self
                .placed
                .owed
                .iter()
                .find(|(_, s)| **s == t.session_id)
                .map(|(c, _)| c.clone());
            if let Some(c) = due.filter(|_| t.continuation) {
                self.placed.owed.remove(&c);
                self.mine.insert(t.turn_id.clone());
            }
        }
    }

    /// The placed turn is the chat's, and so are the tasks of its session:
    /// the board is watched again with that session among its roots, and the
    /// session itself, for the answer to its question and the turn that goes
    /// on after it (a connection hears only the sessions it watches).
    fn adopt(&mut self, session_id: &str, turn_id: &str) -> Vec<Effect> {
        self.mine.insert(turn_id.to_string());
        let also = &mut self.placed.also;
        if also.iter().any(|s| s == session_id) {
            return Vec::new();
        }
        also.push(session_id.to_string());
        let evicted = (also.len() > ALSO).then(|| also.remove(0));
        if let Some(old) = &evicted {
            self.placed.owed.retain(|_, s| s != old);
        }
        match self.session.clone() {
            Some(sid) => {
                let mut out = vec![
                    Self::watch_with(&sid, &self.placed.also),
                    Self::watch_session(session_id),
                ];
                out.extend(
                    evicted
                        .filter(|old| *old != sid)
                        .map(|old| Self::unwatch_session(&old)),
                );
                out
            }
            None => Vec::new(),
        }
    }
}

/// The sessions kept in `Placed::also`.
const ALSO: usize = 8;

/// What the router's placements brought into the chat.
#[derive(Default)]
pub(super) struct Placed {
    /// Sessions of turns placed in threads for this chat, the last `ALSO`.
    pub(super) also: Vec<String>,
    /// Turns whose placement was told, so it is told once.
    pub(super) told: std::collections::HashSet<String>,
    /// Open questions of this chat's placed turns, by correlation id, with
    /// their session: the next continuation turn there is this chat's.
    pub(super) owed: std::collections::HashMap<String, String>,
}
