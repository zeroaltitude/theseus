//! The gate (issue 44, step 2; the design note is `docs/design/view-gate.md`):
//! the one place that decides, for a turn's point of view, what of memory may
//! reach a model. The rule is pure and lives in `theseus_memory::gate`, beside
//! today's place rule; this module reads what the rule needs from the daemon
//! and is the rule's one caller. What is shown is `view::gate::Admitted`, made
//! only by the view's gate in that module.
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
//!   writes it as `said.<node>` (`exchange::mark`).
//! - **A turn's presence** ([`Asking`]): computed once per turn, after its
//!   input is written (`turn::gate_step`), from the input's mark, or for a
//!   wake, a task's report or a task's own turn from the turn that set it up
//!   (its exchange's ask and that ask's mark, [`set_up`]). It is held per
//!   session in [`Presences`] for the tools the turn runs.
//! - **A piece's marks** ([`session_marks`]): the place its session arrived
//!   in, as the ledger names it (`exchange::place_of`: a binding's target,
//!   else `local:<dir>`), today's reading of it, its seal ([`seal_of`]) and
//!   its veil.
//! - **Seals on every path** ([`seal_of`], [`seal_out`]): whatever `[threads]
//!   enabled` says, a candidate of a sealed or seal-pending thread is held
//!   out before ranking, unless the asker is that thread; a thread that
//!   cannot be proven is `Unread`, and held out too.
//! - **Recall's places** ([`RecallPlaces`]): a channel bound private draws on
//!   that channel alone (decision 5: "someone could join after and read the
//!   history"); with the ledger on, a turn that is not the owner alone draws
//!   on its exact arrival place alone. With `[context] ledger` off, a turn at
//!   the terminal or in a DM reads places as before.
//!
//! Today's decisions by place elsewhere in the core stay as they are; the
//! registry test (`tests_view_gate::no_new_decision_by_place_outside_the_gate`)
//! names each file's count and fails on a new one.

use std::collections::{BTreeMap, HashMap};
use std::sync::Mutex;

use anyhow::Result;
use theseus_memory::gate::{self as rule, Denied, Marks, PointOfView, Present, Seal, Veil};
use theseus_memory::recall::Place;

use crate::exchange::OwnerMark;
use crate::node::Body;
use crate::recall::Answer;
use crate::session::SessionRecord;
use crate::store::Store;
use crate::threads::Board;
use crate::turn::TurnRunner;

/// What the gate names an imported session's place: the owner's own history,
/// from no place of this daemon's.
pub const IMPORTED: &str = "imported";
/// The longest a task's chain is followed to its thread.
const DEPTH: usize = 4;

/// What the gate reads of the owner's mark of one message.
pub trait OwnerMarks {
    /// Whether the owner typed `node` in person; none when no mark was
    /// written for it.
    fn in_person(&self, node: &str) -> Result<Option<bool>>;
}

impl OwnerMarks for Store {
    fn in_person(&self, node: &str) -> Result<Option<bool>> {
        Ok(crate::exchange::mark(self, node)?.map(|m| m.in_person))
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
/// and the web page (no target, or the ledger's `local…`), or a direct
/// conversation.
fn alone_surface(target: Option<&str>) -> bool {
    target.is_none_or(|t| t.starts_with("local") || t.starts_with("discord:dm:"))
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
        Place::Shared(_) => return Present::Shared,
        Place::Private => {}
    }
    // A channel is never alone, whoever is in it (the owner's decision 5).
    if !alone_surface(target) {
        return Present::PrivateChannel;
    }
    let node = match input {
        // Inherited, it is never wider than this place allows.
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

/// `present`, for a mark already read: the owner alone only where it arrived
/// at a surface he can be alone at and the owner typed it in person.
pub fn present_of_mark(place: &Place, mark: &OwnerMark) -> Present {
    struct One(bool);
    impl OwnerMarks for One {
        fn in_person(&self, _: &str) -> Result<Option<bool>> {
            Ok(Some(self.0))
        }
    }
    present(
        place,
        Some(&mark.place),
        Input::Message(&mark.node),
        &One(mark.in_person),
    )
}

/// The place a session's target names, as the gate compares them: the
/// ledger's name with no directory.
pub fn anchor_of(target: Option<&str>) -> String {
    crate::exchange::place_of(target, None)
}

/// The gate.
pub struct Gate;

impl Gate {
    /// Whether a piece of `marks` may reach `pov`'s model; why not
    /// otherwise. The one call of the rule: what is shown is made only by
    /// the view's gate (`view::gate`), from this answer.
    pub fn check(pov: &PointOfView, marks: &Marks) -> Result<(), Denied> {
        rule::check(pov, marks)
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

/// The place `session` arrived in, as the ledger names it (a binding's
/// target, else `local:<dir>`); none when it cannot be read.
pub fn place_name(runner: &TurnRunner, session: &str) -> Option<String> {
    if crate::import::is_imported(session) {
        return Some(IMPORTED.into());
    }
    let target = target_of(runner, session).ok()?;
    if let Some(t) = target {
        return Some(t);
    }
    let rec: Option<SessionRecord> = runner.store.get_session(session).ok()?;
    let dir = rec.and_then(|r| r.dir);
    Some(crate::exchange::place_of(None, dir.as_deref()))
}

/// The thread `session` serves, read so that nothing unread reads as none:
/// a session, ancestry or binding that does not decode, a chain too long to
/// follow, or a thread the board does not hold is an error.
fn thread_of(store: &Store, board: &Board, session: &str, depth: usize) -> Result<Option<String>> {
    if crate::import::is_imported(session) {
        return Ok(None);
    }
    if board.unread_session(session) {
        anyhow::bail!("session {session} is bound to a thread whose record does not read");
    }
    let slug = match board.of_session(session) {
        Some(t) => Some(t.slug.clone()),
        None => {
            let Some(rec) = store.get_session::<SessionRecord>(session)? else {
                return Ok(None);
            };
            let linked = [&rec.superseded_by, &rec.supersedes]
                .into_iter()
                .flatten()
                .filter_map(|l| l.place.as_deref())
                .find_map(theseus_protocol::threads::slug_of_place)
                .map(str::to_string);
            match (linked, rec.task) {
                (Some(slug), _) => Some(slug),
                (None, Some(t)) if depth < DEPTH => {
                    return thread_of(store, board, &t.parent_session, depth + 1)
                }
                (None, Some(_)) => anyhow::bail!("session {session}'s task chain is too long"),
                (None, None) => None,
            }
        }
    };
    match slug {
        Some(s) if board.get(&s).is_none() || board.unread_slug(&s) => {
            anyhow::bail!("session {session} names the thread {s}, whose record is not held")
        }
        Some(s) => Ok(Some(s)),
        // A binding that did not read may be this session's.
        None if board.unread_binding() => anyhow::bail!("a thread's binding does not read"),
        None => Ok(None),
    }
}

/// The seal of `session` for a reader in `asker`, whatever `[threads]
/// enabled` says: `Held` when it serves a sealed thread, or one whose seal
/// is proposed, and the asker is not that thread; `Unread` when that cannot
/// be proven either way.
pub fn seal_of(runner: &TurnRunner, asker: Option<&str>, session: &str) -> Seal {
    let board = match runner.threads.all(&runner.store) {
        Ok(b) => b,
        Err(e) => {
            tracing::warn!(session, error = %format!("{e:#}"),
                "the threads cannot be read: the session is held out");
            return Seal::Unread;
        }
    };
    seal_on(&runner.store, &board, asker, session)
}

fn seal_on(store: &Store, board: &Board, asker: Option<&str>, session: &str) -> Seal {
    let slug = match thread_of(store, board, session, 0) {
        Ok(s) => s,
        Err(e) => {
            tracing::warn!(session, error = %format!("{e:#}"),
                "a session's thread cannot be proven: it is held out");
            return Seal::Unread;
        }
    };
    let Some(slug) = slug else {
        return Seal::Clear;
    };
    let held = board
        .get(&slug)
        .is_some_and(|t| t.sealed.is_some() || t.seal_pending.is_some());
    let own = asker.and_then(|a| thread_of(store, board, a, 0).ok().flatten()) == Some(slug);
    match held && !own {
        true => Seal::Held,
        false => Seal::Clear,
    }
}

/// The index's hits a reader in `asker` may not draw on for their seal,
/// taken out before anything ranks them: every hit whose seal is not clear,
/// but a `Held` one when the recall's thread lens will drop it for `sealed`
/// itself (`lens`), so its count is said.
pub fn seal_out(runner: &TurnRunner, asker: Option<&str>, answer: &mut Answer, lens: bool) {
    let Answer::Hits(r) = answer else {
        return;
    };
    let board = runner.threads.all(&runner.store);
    let mut seen: BTreeMap<String, Seal> = BTreeMap::new();
    r.hits.retain(|h| {
        let seal = *seen
            .entry(h.session_id.clone())
            .or_insert_with(|| match &board {
                Ok(b) => seal_on(&runner.store, b, asker, &h.session_id),
                Err(_) => Seal::Unread,
            });
        seal == Seal::Clear || (lens && seal == Seal::Held)
    });
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

/// One turn's presence, and where it arrived.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Asking {
    pub turn_id: String,
    pub present: Present,
    /// The exact arrival place, as the ledger names it.
    pub place: String,
}

/// Each session's presence, as its latest turn computed it: what the tools
/// of that turn read. Lost at a restart, when a session with none reads as
/// not the owner alone.
#[derive(Default)]
pub struct Presences(Mutex<HashMap<String, Asking>>);

/// The most sessions held before the table starts again.
const PRESENCES_MAX: usize = 4_096;

impl Presences {
    pub fn set(&self, session: &str, a: Asking) {
        let mut m = self.0.lock().unwrap_or_else(|e| e.into_inner());
        if m.len() >= PRESENCES_MAX && !m.contains_key(session) {
            m.clear();
        }
        m.insert(session.to_string(), a);
    }

    pub fn get(&self, session: &str) -> Option<Asking> {
        self.0
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .get(session)
            .cloned()
    }
}

/// The presence of the turn of `session` that made the call `call_id` (a
/// `wake.at` or a `task.create`), read from what the ledger wrote of it: that
/// turn's exchange, its ask, and the ask's owner's mark, at `class` (today's
/// reading of `session`'s place). None when any of it is not there: a turn
/// no message opened, or one from before the ledger.
pub fn set_up(store: &Store, class: &Place, session: &str, call_id: &str) -> Option<Present> {
    let nodes = store.session_nodes(session).ok()?;
    let turn = nodes.iter().find_map(|(_, n)| match &n.body {
        Body::ToolCall { tool_use_id, .. } if tool_use_id == call_id => n.turn_id.clone(),
        _ => None,
    })?;
    let ask = crate::exchange::exchange(store, &turn).ok()??.ask?;
    let mark = crate::exchange::mark(store, &ask).ok()??;
    Some(present_of_mark(class, &mark))
}

/// Of several set-ups, the narrowest; any not recorded is `Unknown`.
pub fn narrowest(all: impl IntoIterator<Item = Option<Present>>) -> Option<Present> {
    all.into_iter().map(|p| p.unwrap_or(Present::Unknown)).max()
}

/// How recall reads places for one asker. Narrowed to one place, the asker
/// reads as that place and its own sessions as it, and every other session
/// as it always has, which the place rule then drops for `place`. A channel
/// bound private is always narrowed to itself (decision 5); with the ledger
/// on, so is a turn that is not the owner alone, to its exact arrival place.
/// Otherwise places read as before.
#[derive(Clone, Debug, Default)]
pub struct RecallPlaces {
    own: Option<String>,
    channel: bool,
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
        let target = target_of(runner, asker).ok().flatten();
        if let Some(t) = target.filter(|t| t.starts_with("discord:channel:")) {
            return RecallPlaces {
                own: Some(t),
                channel: true,
            };
        }
        if !runner.cfg.context.ledger_on() {
            return Self::default();
        }
        match runner.presence.get(asker) {
            Some(a) if a.present == Present::OwnerAlone => Self::default(),
            Some(a) => RecallPlaces {
                own: Some(a.place),
                channel: false,
            },
            // No proof: the asker's own place alone, or nothing.
            None => RecallPlaces {
                own: Some(place_name(runner, asker).unwrap_or_else(|| "unknown".into())),
                channel: false,
            },
        }
    }

    /// The channel bound private the asker speaks in, when it does.
    pub fn channel(&self) -> Option<&str> {
        self.own.as_deref().filter(|_| self.channel)
    }

    /// The one place the asker draws on, when it is narrowed.
    pub fn narrowed(&self) -> Option<&str> {
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
                    && place_name(runner, session).as_deref() == Some(own) =>
            {
                Place::Shared(own.clone())
            }
            _ => today,
        }
    }

    /// Whether a reader in this asker may read `session` whole: any session
    /// unless the asker is narrowed, when only its own place's.
    pub fn may_read(&self, runner: &TurnRunner, session: &str) -> bool {
        match &self.own {
            None => true,
            Some(own) => place_name(runner, session).as_deref() == Some(own),
        }
    }
}

/// The verified point of view of one turn, for the view's gate: made here
/// alone, from the turn's presence and today's reading of its place, never
/// from what a caller says of itself.
#[derive(Clone, Debug)]
pub struct Verified(PointOfView);

impl Verified {
    /// The turn of `session`'s: its presence as the turn computed it; none
    /// computed, `Unknown`.
    pub fn of_session(runner: &TurnRunner, session: &str) -> Self {
        let place = runner.place_of(session);
        let (present, anchor) = match runner.presence.get(session) {
            Some(a) => (a.present, a.place),
            None => (
                Present::Unknown,
                place_name(runner, session).unwrap_or_else(|| "unknown".into()),
            ),
        };
        Verified(PointOfView {
            anchor,
            place,
            present,
        })
    }

    /// `pov` as given, for the gate's own tests.
    #[cfg(test)]
    pub fn for_tests(pov: PointOfView) -> Self {
        Verified(pov)
    }

    pub fn pov(&self) -> &PointOfView {
        &self.0
    }
}

/// Stage the mark `turn.submit` would make for `text` in `session`: typed by
/// the owner in person, or by an agent's process at the terminal.
#[cfg(test)]
pub fn stage_mark(runner: &TurnRunner, session: &str, text: &str, in_person: bool) {
    let mark = OwnerMark {
        node: String::new(),
        in_person,
        owner_handle: None,
        peer: match in_person {
            true => crate::exchange::Peer::Person,
            false => crate::exchange::Peer::Agent,
        },
        agent: (!in_person).then(|| "agent:claude".to_string()),
        surface: "cli".into(),
        // Where it arrived: a chat binding's target, else the terminal (a
        // thread's message arrived where its owner typed it).
        place: crate::exchange::place_of(
            target_of(runner, session)
                .ok()
                .flatten()
                .filter(|t| t.starts_with("discord:"))
                .as_deref(),
            None,
        ),
        arrived_session: session.into(),
        ran_session: session.into(),
        author: None,
        at_ms: 0,
    };
    runner.exchanges.arrivals.stage(session, text, mark);
}

/// Seat the owner alone in `session`, as a turn of his in person would, for
/// a test that calls a tool past any turn.
#[cfg(test)]
pub fn seat_owner(runner: &TurnRunner, session: &str) {
    let place = place_name(runner, session).unwrap_or_else(|| "local".into());
    runner.presence.set(
        session,
        Asking {
            turn_id: "turn_test".into(),
            present: Present::OwnerAlone,
            place,
        },
    );
}
