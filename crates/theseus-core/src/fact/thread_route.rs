//! The router's facts (theseus-rha2): where a message ran, by its address or
//! at home (`thread.routed`, in the routed turn's first frame), and a move
//! (`thread.moved`). The rows hold slugs and ids, never the message's words.
//! Rows of `thread.asked`, `thread.answered`, `thread.claimed` and
//! `thread.misroute_held`, which the router and its guard wrote while it
//! guessed, stay in a store and read as any row of a kind this build does not
//! write.

use serde_json::{json, Value};
use theseus_protocol::thread_route::RouteHow;
use theseus_protocol::threads::slug_ok;
use theseus_protocol::LedgerKind;
use theseus_protocol::NarrativePart::Session;

use super::{Fact, Say};

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
    /// The inbound point was begun before the turn (no added call: the turn
    /// took it over).
    pub early: bool,
    pub movable: bool,
    /// What was just said in the place, given to a turn that ran elsewhere
    /// (`compiler::place_tail`): from `home` or a `thread`, and its tokens.
    pub tail: Option<(String, u64)>,
}

/// `thread.routed`: where a message in a place with threads ran.
pub struct ThreadRouted<'a>(pub &'a RoutedRow);

impl Fact for ThreadRouted<'_> {
    const KIND: Option<LedgerKind> = Some(LedgerKind::ThreadRouted);

    fn row(&self) -> Value {
        let r = self.0;
        let mut row = json!({
            "home": r.home, "to": r.to.as_deref().unwrap_or("home"), "session": r.session,
            "how": r.how, "confidence": r.confidence, "movable": r.movable, "early": r.early,
        });
        if let Some((from, tokens)) = &r.tail {
            row["tail"] = json!({"from": from, "tokens": tokens});
        }
        row
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

/// `thread.moved`: the owner said a message belonged elsewhere.
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
}

impl Fact for ThreadMoved<'_> {
    const KIND: Option<LedgerKind> = Some(LedgerKind::ThreadMoved);

    fn row(&self) -> Value {
        json!({"turn": self.turn, "from": self.from, "to": self.to, "who": self.who,
               "via": self.via, "resubmitted": self.resubmitted, "redirected": self.redirected})
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

/// Whether `s` could be a slug (a `thread.move` target is a slug or `home`).
pub fn is_target(s: &str) -> bool {
    s == "home" || slug_ok(s)
}
