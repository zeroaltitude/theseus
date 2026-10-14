//! The router's facts (theseus-rha2): where an unaddressed message ran and on
//! what grounds (`thread.routed`, in the routed turn's first frame), the
//! question when it could not tell (`thread.asked`, which is also the
//! `thread.asked` notification) and its answer (`thread.answered`), a move
//! (`thread.moved`), and a call held because its path is not in the thread
//! the turn was routed to on a guess (`thread.misroute_held`). The rows hold
//! slugs, scores and ids, never the message's words.

use serde_json::{json, Value};
use theseus_protocol::thread_route::{RouteHow, ThreadAsk};
use theseus_protocol::threads::slug_ok;
use theseus_protocol::NarrativePart::Session;
use theseus_protocol::{notify, Event, LedgerKind};

use super::{Fact, Say};

/// Jev's part in a choice.
#[derive(Debug, Clone, PartialEq)]
pub struct JevRow {
    pub pick: String,
    pub p: f64,
    pub judgment: String,
    pub acts: bool,
}

/// Where a message ran and on what grounds.
#[derive(Debug, Clone, PartialEq)]
pub struct RoutedRow {
    /// The place's own session.
    pub home: String,
    /// The thread it ran in; none: at home.
    pub to: Option<String>,
    pub session: String,
    pub how: RouteHow,
    pub confidence: f64,
    /// The pre-router's score for each thread: (slug, score, entities hit).
    pub pre: Vec<(String, f64, Vec<String>)>,
    pub jev: Option<JevRow>,
    /// Jev was waited for and did not answer in `[threads] route_wait_ms`.
    pub timed_out: bool,
    /// How long the choice waited for Jev, when it did.
    pub waited_ms: Option<u64>,
    /// The question that settled it.
    pub ask: Option<String>,
    /// The inbound point was begun before the choice (no added call: the
    /// turn took it over).
    pub early: bool,
    pub movable: bool,
}

/// `thread.routed`: an unaddressed message found its thread.
pub struct ThreadRouted<'a>(pub &'a RoutedRow);

impl Fact for ThreadRouted<'_> {
    const KIND: Option<LedgerKind> = Some(LedgerKind::ThreadRouted);

    fn row(&self) -> Value {
        let r = self.0;
        json!({
            "home": r.home, "to": r.to.as_deref().unwrap_or("home"), "session": r.session,
            "how": r.how, "confidence": r.confidence, "movable": r.movable,
            "pre": r.pre.iter().map(|(s, p, h)| json!({"thread": s, "score": p, "hits": h})).collect::<Vec<_>>(),
            "jev": r.jev.as_ref().map(|j| json!({"pick": j.pick, "p": j.p, "judgment": j.judgment, "acts": j.acts})),
            "timed_out": r.timed_out, "waited_ms": r.waited_ms, "ask": r.ask, "early": r.early,
        })
    }

    fn narrate(&self, say: &mut Say<'_>) {
        let r = self.0;
        say.line(
            Session,
            format!(
                "Thread: the message ran in {} ({:?}, {:.2}).",
                r.to.as_deref().unwrap_or("the place's own conversation"),
                r.how,
                r.confidence
            ),
        );
    }
}

/// `thread.asked`: the router could not tell, and asks in one line.
pub struct ThreadAsked<'a>(pub &'a ThreadAsk);

impl Fact for ThreadAsked<'_> {
    const KIND: Option<LedgerKind> = Some(LedgerKind::ThreadAsked);
    const METHOD: Option<&'static str> = Some(notify::THREAD_ASKED);

    fn row(&self) -> Value {
        let a = self.0;
        json!({"ask": a.ask_id, "place": a.place, "question": a.question,
               "options": a.options.iter().map(|o| &o.label).collect::<Vec<_>>(),
               "expires_ms": a.expires_ms})
    }

    fn event(&self) -> Option<Event> {
        Some(Event::ThreadAsked(self.0.clone()))
    }

    fn narrate(&self, say: &mut Say<'_>) {
        say.line(Session, format!("Thread: asked: {}", self.0.question));
    }
}

/// `thread.answered`: the question was answered, or ran out.
pub struct ThreadAnswered<'a> {
    pub ask: &'a str,
    /// The choice's key, or `expired`.
    pub choice: &'a str,
    /// The thread it went to; none: home.
    pub routed: Option<&'a str>,
    pub who: &'a str,
    pub via: &'a str,
    pub waited_ms: u64,
}

impl Fact for ThreadAnswered<'_> {
    const KIND: Option<LedgerKind> = Some(LedgerKind::ThreadAnswered);

    fn row(&self) -> Value {
        json!({"ask": self.ask, "choice": self.choice, "routed": self.routed.unwrap_or("home"),
               "who": self.who, "via": self.via, "waited_ms": self.waited_ms})
    }

    fn narrate(&self, say: &mut Say<'_>) {
        say.line(
            Session,
            format!(
                "Thread: the question was {}: the message went to {}.",
                if self.choice == "expired" {
                    "left unanswered"
                } else {
                    "answered"
                },
                self.routed.unwrap_or("the place's own conversation")
            ),
        );
    }
}

/// `thread.moved`: the owner said a message was routed wrong.
pub struct ThreadMoved<'a> {
    pub turn: &'a str,
    pub from: &'a str,
    pub to: &'a str,
    pub who: &'a str,
    pub via: &'a str,
    /// The new turn that ran it in the right thread.
    pub resubmitted: &'a str,
    /// The wrong thread's session was told to disregard it.
    pub redirected: bool,
    /// The label written on the `thread` question, when Jev was asked.
    pub labeled: Option<&'a str>,
}

impl Fact for ThreadMoved<'_> {
    const KIND: Option<LedgerKind> = Some(LedgerKind::ThreadMoved);

    fn row(&self) -> Value {
        json!({"turn": self.turn, "from": self.from, "to": self.to, "who": self.who,
               "via": self.via, "resubmitted": self.resubmitted, "redirected": self.redirected,
               "labeled": self.labeled})
    }

    fn narrate(&self, say: &mut Say<'_>) {
        say.line(
            Session,
            format!(
                "Thread: {} moved a message from {} to {}.",
                self.who, self.from, self.to
            ),
        );
    }
}

/// `thread.misroute_held`: the turn was routed below the act bar, and its
/// first write or program names a path outside the thread's working set.
pub struct MisrouteHeld<'a> {
    pub thread: &'a str,
    pub tool: &'a str,
    pub path: &'a str,
}

impl Fact for MisrouteHeld<'_> {
    const KIND: Option<LedgerKind> = Some(LedgerKind::ThreadMisrouteHeld);

    fn row(&self) -> Value {
        json!({"thread": self.thread, "tool": self.tool, "path": self.path})
    }

    fn narrate(&self, say: &mut Say<'_>) {
        say.line(
            Session,
            format!(
                "Thread: {} names {}, which is not in the {} thread; the call waits for your word.",
                self.tool, self.path, self.thread
            ),
        );
    }
}

/// Whether `s` could be a slug (a `thread.move` target is a slug or `home`).
pub fn is_target(s: &str) -> bool {
    s == "home" || slug_ok(s)
}
