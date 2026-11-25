//! The gate (issue 44; the design note is `docs/design/view-gate.md`). Anything goes (2026-11-22): nothing
//! is withheld from a model request because of who is present, where it is, a seal, a move or a veil. What
//! is left here is the rule's one caller ([`Gate::check`]), the seal and move readers' pass-throughs until
//! their callers go, and each turn's point of view ([`Verified`]): the widest, at its own anchor. Recall reads
//! every place for every turn; nothing here narrows it.
//!
//! The one reading of who is present that stays is authority, not privacy: whether the owner typed a
//! message in person at a surface he can be alone at, which a task's objective change requires. It lives
//! beside its user, `task_graph::words::present_in_person`; [`present`] passes through to it.

use anyhow::Result;
use theseus_memory::gate::{self as rule, Denied, Marks, PointOfView, Present, Seal, Veil};
use theseus_memory::recall::Place;

use crate::session::SessionRecord;
use crate::store::Store;
use crate::turn::TurnRunner;

/// What the gate names an imported session's place: the owner's own history,
/// from no place of this daemon's.
pub const IMPORTED: &str = "imported";

pub use crate::task_graph::words::OwnerMarks;

/// What opened the turn, for its presence.
#[derive(Clone, Copy, Debug)]
pub enum Input<'a> {
    /// A person's message, by its node.
    Message(&'a str),
    /// A wake or a task's report: the presence of the turn that set it up,
    /// when one was recorded.
    SetUp(Option<Present>),
}

/// Who is present for a turn at `place` (today's reading), whose input arrived at `target`, opened by
/// `input`: the objective rule's reading (`task_graph::words::present_in_person`), and, for a turn no
/// message opened, the presence it was set up with where that reading would be the owner alone. A
/// pass-through for the history tool and the task tests until they go (the sweep): it decides no context.
pub fn present(
    place: &Place,
    target: Option<&str>,
    input: Input<'_>,
    marks: &dyn OwnerMarks,
) -> Present {
    use crate::task_graph::words::present_in_person;
    struct InPerson;
    impl OwnerMarks for InPerson {
        fn in_person(&self, _: &str) -> Result<Option<bool>> {
            Ok(Some(true))
        }
    }
    match input {
        Input::Message(node) => present_in_person(place, target, node, marks),
        Input::SetUp(set_up) => match present_in_person(place, target, "", &InPerson) {
            Present::OwnerAlone => set_up.unwrap_or(Present::Unknown),
            p => p,
        },
    }
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

/// A whole session's marks: its place, today's reading of it, no seal (anything goes, 2026-11-22), and
/// `veil` as its caller read it.
pub fn session_marks(
    runner: &TurnRunner,
    _asker: Option<&str>,
    session: &str,
    veil: Veil,
) -> Marks {
    Marks {
        place: place_name(runner, session),
        class: runner.place_of(session),
        time_ms: 0,
        seal: Seal::Clear,
        veil,
    }
}

/// The standing records, of `(id, source)`, a turn may not be shown: each kept from a message
/// (`capture:<node>`) whose message is gone or cannot be read (a removed thing is not sent again). A record
/// from no conversation (the owner's own surfaces) is source-free. Anything goes (2026-11-22): no place,
/// presence or seal holds one.
pub fn standing_held<'r>(
    runner: &TurnRunner,
    _asker: &str,
    records: impl Iterator<Item = (&'r str, &'r str)>,
) -> std::collections::HashSet<String> {
    records
        .filter(|(_, source)| {
            source
                .strip_prefix("capture:")
                .is_some_and(|node| !matches!(runner.store.get_node(node), Ok(Some(_))))
        })
        .map(|(id, _)| id.to_string())
        .collect()
}

/// The point of view of one turn, for the view's gate: made here alone, the widest at the turn's own
/// place ([`widest`]), never from what a caller says of itself; and whether the turn is a session the self
/// loop opened to judge (a reviewer, an approver), which lays its own place only
/// ([`Verified::own_place_only`]).
#[derive(Clone, Debug)]
pub struct Verified(PointOfView, Scope);

/// What a turn's view may lay: the widest, or (a session the self loop opened to judge) its own place only;
/// or the session's execution or its self-loop role could not be read, so that cannot be decided, and the
/// view is not assembled ([`Verified::scope_unread`]).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Scope {
    Wide,
    OwnPlace,
    Unread,
}

impl Verified {
    /// The turn of `session`'s, read outside the turn: [`Self::of_turn`] at the session's own place.
    pub fn of_session(runner: &TurnRunner, session: &str) -> Self {
        Self::of_turn(runner, session, None)
    }

    /// The turn of `session`'s: the widest ([`widest`]), at `place`, the place the turn arrived in (its
    /// exchange's, `turn::exchange_step::turn_place`); none given, the session's own place. A task's is at its
    /// own place.
    pub fn of_turn(runner: &TurnRunner, session: &str, place: Option<&str>) -> Self {
        let anchor = match crate::task_graph::place::of_task_session(&runner.store, session) {
            Some(own) => own,
            None => (place.map(str::to_string))
                .or_else(|| place_name(runner, session))
                .unwrap_or_else(|| "unknown".into()),
        };
        Verified(widest(anchor), scope_of(runner, session))
    }

    /// The point of view a next turn in `session` at `place` would get, for a preview of its view (no turn
    /// runs): the widest, at `place`.
    pub fn of_preview(runner: &TurnRunner, session: &str, place: &str) -> Self {
        Verified(widest(place.into()), scope_of(runner, session))
    }

    /// [`Self::of_preview`] for a session that does not exist yet, at `place` (a census of a fresh session's
    /// first request): the widest, at `place` (anything goes, 2026-11-22).
    pub fn of_fresh(_today: Place, place: &str) -> Self {
        Verified(widest(place.into()), Scope::Wide)
    }

    /// The point of view a next turn of the task whose session is `session` would get, for a census of it (no
    /// turn runs): the widest, at the task's own place. None for a session that is not a task's.
    pub fn of_task(runner: &TurnRunner, session: &str) -> Option<Self> {
        // Anything goes (2026-11-22): the widest, at the task's own place.
        let rec: SessionRecord = runner.store.get_session(session).ok()??;
        rec.task?;
        Some(Verified(
            widest(crate::task_graph::place::of_session(session)),
            scope_of(runner, session),
        ))
    }

    /// `pov` as given, for the gate's own tests.
    #[cfg(test)]
    pub fn for_tests(pov: PointOfView) -> Self {
        Verified(pov, Scope::Wide)
    }

    pub fn pov(&self) -> &PointOfView {
        &self.0
    }

    /// Whether the turn is of a session the self loop opened to JUDGE (its execution's principal is `self`
    /// and its role a reviewer or an approver, `rsi::roles::Role::own_place_only`; a fork of one keeps it).
    /// Such a turn lays ITS OWN PLACE ONLY, as at the base: its own session's exchanges there, its opening
    /// and what it is handed (for a fork, the run it inherits, through the fork's cut); no other session's
    /// exchange, no card, nothing waiting, elsewhere, past or recalled (`view::permit::Permit::scoped`). This
    /// is a SCOPE rule of the self loop (the reviewer's and the approver's independence from the builder's and
    /// the fixer's words and everyone else's), not a withholding rule: it reads no presence, mark, seal or
    /// move. The self loop's building sessions (a builder, whose session has no role, and a fixer) read at
    /// the widest, as an ordinary task, one a person's turn started, does.
    pub fn own_place_only(&self) -> bool {
        self.1 == Scope::OwnPlace
    }

    /// Whether the session's execution, or the self-loop role of a session the self loop opened, could not
    /// be read, so its scope cannot be decided: its view is not assembled (a plain failure), never laid at
    /// the widest by default.
    pub fn scope_unread(&self) -> bool {
        self.1 == Scope::Unread
    }
}

/// The scope of `session`'s turns: its own place only when its execution's principal is `self` (the self
/// loop opened it) and its role is a judging one (`TurnRunner::role_read`: a reviewer or an approver, or a
/// fork of one); the widest for a self session with no role (a builder) or a fixer's, for any other
/// principal, and for a session with no execution yet (a fresh or legacy one, which a turn gives an operator
/// execution before it runs, `turn.rs`). A session record, an execution or a self session's role that cannot
/// be read is `Unread`: never the widest by default. Read straight from the kernel, not through
/// `task_graph::principal_of`, whose `operator` default is for act gates.
fn scope_of(runner: &TurnRunner, session: &str) -> Scope {
    let Ok(Some(rec)) = runner.store.get_session::<SessionRecord>(session) else {
        return Scope::Unread;
    };
    let Some(exe) = rec.execution_id.as_deref() else {
        return Scope::Wide;
    };
    match runner.kernel.execution(exe) {
        Ok(Some(e)) if e.authority.principal == crate::rsi::SELF_PRINCIPAL => {
            match runner.role_read(session) {
                Ok(Some(role)) if role.own_place_only() => Scope::OwnPlace,
                Ok(_) => Scope::Wide,
                Err(_) => Scope::Unread,
            }
        }
        Ok(Some(_)) => Scope::Wide,
        Ok(None) | Err(_) => Scope::Unread,
    }
}

/// How many of a session's newest earlier turns [`own_turns`] looks at: twice the 40 a task's window reads when
/// it has no folds (`crate::ladder::TASK_WINDOW`), and more than the 49 a place's window holds at most.
const OWN_TURNS: usize = 2 * crate::ladder::TASK_WINDOW;

/// The exchanges `session` itself ran at `place` before `turn`, among its newest [`OWN_TURNS`] turns: what a
/// judging self-loop turn of it is shown at that place ([`Verified::own_place_only`]). An exchange whose record cannot
/// be read is not among them.
pub fn own_turns(store: &Store, session: &str, turn: &str, place: &str) -> Vec<String> {
    let Ok(nodes) = store.transcript(session) else {
        return Vec::new();
    };
    let mut seen = std::collections::HashSet::new();
    let turns = (nodes.iter().rev())
        .filter_map(|(_, n)| n.turn_id.as_deref())
        .filter(|t| *t != turn && seen.insert(*t))
        .take(OWN_TURNS)
        .map(str::to_string)
        .collect::<Vec<_>>();
    (turns.into_iter())
        .filter(|t| match crate::exchange::exchange(store, t) {
            Ok(Some(e)) => own_exchange(&e, session, place),
            _ => false,
        })
        .collect()
}

/// Whether `ex` is one `session` itself ran at `place`: the judging self-loop sessions' own-place rule, read by
/// the view's permit ([`own_turns`]) and by the history tool's lens (`history_tool::page::Lens::in_scope`). Not
/// a place alone: another session at the same place is not the asker's.
pub fn own_exchange(ex: &crate::exchange::Exchange, session: &str, place: &str) -> bool {
    ex.session_id == session && ex.place == place
}

/// Whether `session` is a worker's: a task's session, or one the self loop opened (its execution's principal
/// is `self`). Its tools read its point of view as its view does ([`Verified::of_session`]).
pub fn worker(runner: &TurnRunner, session: &str) -> bool {
    let Ok(Some(rec)) = runner.store.get_session::<SessionRecord>(session) else {
        return false;
    };
    rec.task.is_some()
        || (rec.execution_id.as_deref()).is_some_and(|e| {
            crate::task_graph::principal_of(&runner.kernel, e) == crate::rsi::SELF_PRINCIPAL
        })
}

/// Every turn's point of view (anything goes, 2026-11-22): the widest, the owner alone in a private place,
/// at its own anchor, whoever is present and wherever it speaks. The place's class still decides tools and
/// publishing; it no longer decides what a model is shown.
fn widest(anchor: String) -> PointOfView {
    PointOfView {
        anchor,
        place: Place::Private,
        present: Present::OwnerAlone,
    }
}

/// The mark `turn.submit` would make for a message to `session`: typed by
/// the owner in person, or by an agent's process at the terminal.
#[cfg(test)]
pub fn test_mark(
    runner: &TurnRunner,
    session: &str,
    in_person: bool,
) -> crate::exchange::OwnerMark {
    crate::exchange::OwnerMark {
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
    }
}
