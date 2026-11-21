//! The gate (issue 44, step 2; the design note is `docs/design/view-gate.md`):
//! the one place that decides, for a turn's point of view, what of memory may
//! reach a model. The rule and its type, `Admitted<T>`, are pure and live in
//! `theseus_memory::gate`, beside today's place rule; this module reads what
//! the rule needs from the daemon and is the rule's one caller.
//!
//! - **Who is present** ([`present`]): today's reading of the place first (a
//!   shared place is `Shared`, the open view `Open`, an unreadable one
//!   `Unknown`); then **owner alone** only on the terminal, the web page or a
//!   direct conversation with the owner, for an input that carries the
//!   owner's mark. A channel is never alone, whoever is in it; an agent's
//!   process at the terminal is not alone (its input's mark says it was not
//!   the owner in person); a wake or a task's report takes the presence of
//!   the turn that set it up, and none recorded is `Unknown`.
//! - **The owner's mark** is read through [`OwnerMarks`]: issue 44's step 1
//!   writes it as `said.<node>`. Until that step's reader is on this line,
//!   the store's impl reads the key itself, its one field this gate needs.
//! - **A piece's marks** ([`session_marks`]): the place its session speaks
//!   in, today's reading of it, its seal ([`seal_of`]) and its veil.
//! - **The repairs that stand on their own**: reading a session by id holds
//!   out a sealed or seal-pending thread's ([`seal_of`], called by
//!   `memory_lookup`), and recall in a channel bound private draws on that
//!   channel alone ([`RecallPlaces`], the owner's decision of 2026-11-21:
//!   "someone could join after and read the history").
//!
//! Today's decisions by place elsewhere in the core stay as they are; the
//! registry test (`tests_view_gate::no_new_decision_by_place_outside_the_gate`)
//! names each file's count and fails on a new one.

use anyhow::Result;
use serde::Deserialize;
use theseus_memory::gate::{
    self as rule, Admitted, Denied, Marks, PointOfView, Present, Seal, Veil,
};
use theseus_memory::recall::Place;

use crate::store::Store;
use crate::turn::TurnRunner;

/// The owner's mark of a node, as issue 44's step 1 writes it.
pub const SAID_PREFIX: &str = "said.";
/// What the gate names the terminal's and the web page's place.
pub const LOCAL: &str = "local";
/// What the gate names an imported session's place: the owner's own history,
/// from no place of this daemon's.
pub const IMPORTED: &str = "imported";

/// What the gate reads of the owner's mark of one message.
pub trait OwnerMarks {
    /// Whether the owner typed `node` in person; none when no mark was
    /// written for it.
    fn in_person(&self, node: &str) -> Result<Option<bool>>;
}

/// The one field of `said.<node>` the gate reads.
#[derive(Deserialize)]
struct Said {
    in_person: bool,
}

impl OwnerMarks for Store {
    fn in_person(&self, node: &str) -> Result<Option<bool>> {
        Ok(self
            .get_meta::<Said>(&format!("{SAID_PREFIX}{node}"))?
            .map(|s| s.in_person))
    }
}

/// What opened the turn, for its presence.
#[derive(Clone, Copy, Debug)]
pub enum Input<'a> {
    /// A person's message, by its node.
    Message(&'a str),
    /// A wake or a task's report: the presence of the turn that set it up,
    /// when one was recorded.
    SetUp(Option<Present>),
}

/// Whether `target` is a surface where the owner can be alone: the terminal
/// and the web page (no target), or a direct conversation.
fn alone_surface(target: Option<&str>) -> bool {
    target.is_none_or(|t| t.starts_with("discord:dm:"))
}

/// Who is present for a turn at `place` (today's reading), whose input
/// arrived at `target` (before any thread routing), opened by `input`.
pub fn present(
    place: &Place,
    target: Option<&str>,
    input: Input<'_>,
    marks: &dyn OwnerMarks,
) -> Present {
    match place {
        Place::Unknown => return Present::Unknown,
        Place::Open(_) => return Present::Open,
        Place::Shared(_) => return Present::Shared,
        Place::Private => {}
    }
    // A channel is never alone, whoever is in it (the owner's decision 5).
    if !alone_surface(target) {
        return Present::PrivateChannel;
    }
    let node = match input {
        Input::SetUp(set_up) => return set_up.unwrap_or(Present::Unknown),
        Input::Message(node) => node,
    };
    match marks.in_person(node) {
        Ok(Some(true)) => Present::OwnerAlone,
        Ok(_) => Present::PrivateChannel,
        Err(e) => {
            tracing::warn!(node, error = %format!("{e:#}"),
                "the owner's mark cannot be read: the turn's view admits nothing");
            Present::Unknown
        }
    }
}

/// The place a session's target names, as the gate compares them.
pub fn anchor_of(target: Option<&str>) -> String {
    target.unwrap_or(LOCAL).to_string()
}

/// The gate.
pub struct Gate;

impl Gate {
    /// `value`, admitted for `pov` when a piece of `marks` may reach its
    /// model; why not otherwise. The one call of the rule.
    pub fn admit<T>(pov: &PointOfView, marks: &Marks, value: T) -> Result<Admitted<T>, Denied> {
        rule::admit(pov, marks, value)
    }
}

/// Where `session` speaks: its target, or where its wakes and reports
/// answer, as `TurnRunner::place_of` reads it.
fn target_of(runner: &TurnRunner, session: &str) -> Result<Option<String>> {
    match runner.outbox.try_target(session)? {
        Some(t) => Ok(Some(t)),
        None => runner.outbox.try_wake_target(session),
    }
}

/// The place the gate names for `session`; none when it cannot be read.
pub fn place_name(runner: &TurnRunner, session: &str) -> Option<String> {
    if crate::import::is_imported(session) {
        return Some(IMPORTED.into());
    }
    target_of(runner, session)
        .ok()
        .map(|t| anchor_of(t.as_deref()))
}

/// The seal of `session` for a reader in `asker`: `Held` when it serves a
/// sealed thread, or one whose seal is proposed, and the asker is not that
/// thread; `Unread` when the threads cannot be read.
pub fn seal_of(runner: &TurnRunner, asker: Option<&str>, session: &str) -> Seal {
    let board = match runner.threads.all(&runner.store) {
        Ok(b) => b,
        Err(e) => {
            tracing::warn!(session, error = %format!("{e:#}"),
                "the threads cannot be read: a session read by id is held out");
            return Seal::Unread;
        }
    };
    if board.is_empty() {
        return Seal::Clear;
    }
    let memo = runner.memory.thread_memo();
    let Some(slug) = memo.slug_of(&runner.store, &board, session) else {
        return Seal::Clear;
    };
    let held = board
        .get(&slug)
        .is_some_and(|t| t.sealed.is_some() || t.seal_pending.is_some());
    let own = asker.and_then(|a| memo.slug_of(&runner.store, &board, a)) == Some(slug);
    match held && !own {
        true => Seal::Held,
        false => Seal::Clear,
    }
}

/// A whole session's marks, for a reader in `asker`: its place, today's
/// reading of it, its seal, and `veil` as its caller read it.
pub fn session_marks(runner: &TurnRunner, asker: Option<&str>, session: &str, veil: Veil) -> Marks {
    Marks {
        place: place_name(runner, session),
        class: runner.place_of(session),
        time_ms: 0,
        seal: seal_of(runner, asker, session),
        veil,
    }
}

/// How recall reads places for one asker. A channel bound private recalls as
/// a place of one (decision 5): the asker reads as that channel, its own
/// sessions as it, and every other session as it always has, which the place
/// rule then drops for `place`. Any other asker reads places as before.
#[derive(Clone, Debug, Default)]
pub struct RecallPlaces {
    own: Option<String>,
}

impl RecallPlaces {
    /// The places recall reads for a turn of `asker`.
    pub fn of(runner: &TurnRunner, asker: Option<&str>) -> Self {
        let Some(asker) = asker else {
            return Self::default();
        };
        if runner.place_of(asker) != Place::Private {
            return Self::default();
        }
        let own = target_of(runner, asker)
            .ok()
            .flatten()
            .filter(|t| t.starts_with("discord:channel:"));
        RecallPlaces { own }
    }

    /// The channel bound private the asker speaks in, when it does.
    pub fn channel(&self) -> Option<&str> {
        self.own.as_deref()
    }

    /// The asker's own place, read as recall reads it.
    pub fn asker(&self, today: Place) -> Place {
        match &self.own {
            Some(own) => Place::Shared(own.clone()),
            None => today,
        }
    }

    /// `session`'s place, read as recall reads it for this asker.
    pub fn of_session(&self, runner: &TurnRunner, session: &str) -> Place {
        let today = runner.place_of(session);
        match &self.own {
            Some(own)
                if today == Place::Private
                    && target_of(runner, session).ok().flatten().as_deref() == Some(own) =>
            {
                Place::Shared(own.clone())
            }
            _ => today,
        }
    }

    /// Whether a reader in this asker may read `session` whole: any session
    /// unless the asker is a channel bound private, which reads its own.
    pub fn may_read(&self, runner: &TurnRunner, session: &str) -> bool {
        match &self.own {
            None => true,
            Some(own) => target_of(runner, session).ok().flatten().as_deref() == Some(own),
        }
    }
}
