//! The router's question, and where it placed a message (theseus-wapgp,
//! theseus-0rfjx).
//!
//! A message that names no thread, sent from a place that has threads, finds
//! its own; when the router cannot tell it asks "which thread?" (`thread.asked`)
//! and the turn waits in the daemon for the answer, `[threads] ask_wait_secs`
//! (900 s) at most. The chat never showed the question, so a message of about
//! 150 characters or more (enough words to match two threads) was echoed and
//! then held for a quarter of an hour, with every later message behind it on
//! the connection's session lane. The chat now says the question as `ask`
//! does, and a line that is one of its keys answers it (`thread.answer`, on
//! this connection: it is not an ordered request, so the held turn does not
//! hold it). Anything else is a message, and the question stays until it is
//! answered, expires, or its turn begins.
//!
//! A turn the router placed in a thread says so: `→ thread <title>`, and how
//! to go there.

use serde_json::json;
use theseus_protocol::method;
use theseus_protocol::thread_route::{ThreadAsk, ThreadTag};
use theseus_protocol::{Event, ThreadPlaced, TurnSubmitResult};

use super::state::{call, Chat, Effect, Purpose, AUTHOR};
use super::style::Sty;

/// An option's key from what was typed: its key, `n` or `h`, or its label.
fn choice_of(a: &ThreadAsk, typed: &str) -> Option<usize> {
    let t = typed.trim().to_lowercase();
    let t = match t.as_str() {
        "n" => "new",
        "h" => "home",
        other => other,
    };
    a.options
        .iter()
        .position(|o| o.key == t || o.label.to_lowercase() == t)
}

/// The options as the CLI words them: `[1] pr round   [n]ew …   [h]ome`.
fn keys_line(a: &ThreadAsk) -> String {
    let opts: Vec<String> = a
        .options
        .iter()
        .map(|o| match o.key.as_str() {
            "new" => format!("[n]ew {}", o.label),
            "home" => "[h]ome".to_string(),
            k => format!("[{k}] {}", o.label),
        })
        .collect();
    opts.join("   ")
}

/// How long the wait has left, in words.
fn left(ms: u64) -> String {
    let s = ms / 1000;
    if s >= 90 {
        format!("{} min", s.div_ceil(60))
    } else {
        format!("{s} s")
    }
}

impl Chat {
    /// `thread.asked`: the question, in the transcript. Heard again (the
    /// connection and the session's watch both carry it), it is said once.
    pub(super) fn thread_asked(&mut self, a: ThreadAsk) {
        if !self.ours(&a.session_id) || self.now_ms >= a.expires_ms {
            return;
        }
        if self
            .thread_ask
            .as_ref()
            .is_some_and(|t| t.ask_id == a.ask_id)
        {
            return;
        }
        self.gap();
        self.say_words(Sty::Yellow, &format!("? {}", a.question));
        if !a.message.is_empty() {
            self.say_words(Sty::Dim, &format!("  > {}", a.message));
        }
        self.say_words(Sty::Plain, &format!("  {}", keys_line(&a)));
        self.say_words(
            Sty::Dim,
            &format!(
                "  type a key and Enter · unanswered, it runs at home in {}",
                left(a.expires_ms - self.now_ms)
            ),
        );
        self.thread_ask = Some(a);
    }

    /// A line typed while the question waits: a key answers it (the
    /// `thread.answer` to send); anything else is a message, which the
    /// question's keys are said to once.
    pub(super) fn thread_answered_by(&mut self, typed: &str) -> Option<Vec<Effect>> {
        let a = self.thread_ask.as_ref()?;
        if self.now_ms >= a.expires_ms {
            self.thread_ask = None;
            return None;
        }
        let Some(at) = choice_of(a, typed) else {
            let keys = keys_line(a);
            self.flash(format!("the router still asks which thread · {keys}"));
            return None;
        };
        let (ask_id, option) = (a.ask_id.clone(), a.options[at].clone());
        self.thread_ask = None;
        self.say_plain(Sty::Dim, &format!("  → {}", option.label));
        Some(vec![call(
            method::THREAD_ANSWER,
            json!({ "ask_id": ask_id, "choice": option.key, "author": AUTHOR }),
            Purpose::Route,
        )])
    }

    /// The `thread.answer` was refused (the question ended meanwhile).
    pub(super) fn thread_answer_refused(&mut self, why: &str) {
        self.thread_ask = None;
        self.say_words(Sty::Red, &format!("✗ the answer was not taken: {why}"));
    }

    /// The turn began, or its submit failed: the question it waited on is over.
    pub(super) fn thread_ask_over(&mut self) {
        self.thread_ask = None;
    }

    /// The turn ran in a thread the router chose: say so, and how to go there.
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
            self.say_words(
                Sty::Dim,
                "  a guess: say \"that was for <thread>\" to move it",
            );
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

    /// The router's question, or where it placed this chat's message.
    pub(super) fn thread_event(&mut self, e: Event) -> Vec<Effect> {
        match e {
            Event::ThreadAsked(a) => {
                self.thread_asked(a);
                Vec::new()
            }
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
