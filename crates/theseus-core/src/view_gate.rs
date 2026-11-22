//! The gate (issue 44; the design note is `docs/design/view-gate.md`). Anything goes (2026-11-22): nothing
//! is withheld from a model request because of who is present, where it is, a seal, a move or a veil. What
//! is left here is the rule's one caller ([`Gate::check`]), the seal and move readers' pass-throughs until
//! their callers go, recall's places (every turn reads every place, [`RecallPlaces`]), and each turn's point
//! of view ([`Verified`]): the widest, at its own anchor.
//!
//! The one reading of who is present that stays is authority, not privacy: whether the owner typed a
//! message in person at a surface he can be alone at, which a task's objective change requires. It lives
//! beside its user, `task_graph::words::present_in_person`; [`present`] passes through to it.

use anyhow::Result;
use theseus_memory::gate::{self as rule, Denied, Marks, PointOfView, Present, Seal, Veil};
use theseus_memory::recall::Place;

use crate::recall::Answer;
use crate::session::SessionRecord;
use crate::store::Store;
use crate::threads::Board;
use crate::turn::TurnRunner;

mod moved;
pub use moved::{moved_at_all, sources_moved, Moved};

/// What the gate names an imported session's place: the owner's own history,
/// from no place of this daemon's.
pub const IMPORTED: &str = "imported";
/// The longest a task's chain is followed to its thread.
const DEPTH: usize = 4;

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
/// a session, ancestry or binding that does not decode, a native session
/// with no record, a chain too long to follow, a binding to a thread record
/// that is absent or did not decode, or a thread the board does not hold is
/// an error.
pub fn thread_of(
    store: &Store,
    board: &Board,
    session: &str,
    depth: usize,
) -> Result<Option<String>> {
    if crate::import::is_imported(session) {
        return Ok(None);
    }
    if board.unread_session(session) {
        anyhow::bail!(
            "session {session} is bound to a thread whose record is absent or does not read"
        );
    }
    let slug = match board.of_session(session) {
        Some(t) => Some(t.slug.clone()),
        None => {
            let Some(rec) = store.get_session::<SessionRecord>(session)? else {
                anyhow::bail!("session {session} has no record: its thread cannot be proven");
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

/// Whether a reader in `asker` may be told of thread `slug` (its name, its
/// state, that it exists), decided on `board`, the one snapshot the caller
/// then words its answer from: only when its seal is proven clear for that
/// reader. A sealed or seal-pending thread, or one that cannot be read, is
/// to this reader as if there were no such thread (issue 44).
pub fn may_name_on(_store: &Store, _board: &Board, _asker: &str, _slug: &str) -> bool {
    // Anything goes (2026-11-22): no seal holds a thread's name.
    true
}

/// The seal of `session` for a reader in `asker`, whatever `[threads]
/// enabled` says: `Held` when it serves a sealed thread, or one whose seal
/// is proposed, and the asker is not that thread; `Unread` when that cannot
/// be proven either way.
pub fn seal_of(_runner: &TurnRunner, _asker: Option<&str>, _session: &str) -> Seal {
    // Anything goes (2026-11-22): no seal or pending seal holds anything.
    Seal::Clear
}

/// The seal of one node, for a reader in `asker`: its session's, and, where
/// a split or a claim moved it into a thread (`moved.of.<node>`), that
/// thread's too. A `moved.of` record that does not decode, or names a thread
/// the board does not hold, is `Unread`.
pub fn node_seal(
    _store: &Store,
    _board: &Board,
    _asker: Option<&str>,
    _session: &str,
    _node: &str,
) -> Seal {
    // Anything goes (2026-11-22): no seal, pending seal or move holds a node.
    Seal::Clear
}

/// The nodes of `nodes` a reader in `asker` may read: every one (anything goes, 2026-11-22). A pass-through
/// until its callers go (the sweep).
pub fn readable_nodes<T>(
    _runner: &TurnRunner,
    _asker: Option<&str>,
    nodes: Vec<T>,
    _of: impl Fn(&T) -> (&str, &str),
) -> Vec<T> {
    nodes
}

/// What of the index's hits a reader in `asker` may not draw on for a seal: nothing (anything goes,
/// 2026-11-22). A pass-through until its callers go (the sweep).
pub fn seal_out(
    _runner: &TurnRunner,
    _asker: Option<&str>,
    _answer: &mut Answer,
) -> Vec<theseus_protocol::memory::RecallDrop> {
    Vec::new()
}

/// `seal_out`'s drops, said in a recall's manifest as `sealed`.
pub fn count_sealed(
    m: &mut theseus_protocol::memory::RecallManifest,
    sealed: Vec<theseus_protocol::memory::RecallDrop>,
) {
    if sealed.is_empty() {
        return;
    }
    *m.drops.entry("sealed".into()).or_default() += sealed.len() as u64;
    m.dropped.extend(sealed);
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

/// Whether the report of the task `task` may be written as the input of a turn of `delivery`: always
/// (anything goes, 2026-11-22). A pass-through until its caller goes (the sweep).
pub fn report_shown(_runner: &TurnRunner, _delivery: &str, _task: &str) -> bool {
    true
}

/// What a turn is given in place of a report it may not read here.
pub fn withheld_report(short: &str) -> String {
    format!(
        "[task {short} has finished, and its report is waiting where the owner is alone: it may hold \
         words from a place this conversation may not see, so it is not shown here]"
    )
}

/// The sources of earlier `Recall` nodes a turn of `asker` may still be shown: every one (anything goes,
/// 2026-11-22). A pass-through until its callers go (the sweep).
pub fn readmit_sources(
    _runner: &TurnRunner,
    _asker: &str,
    _sources: &mut crate::recall::render::Sources,
) {
}

/// The threads of a place digest a turn of `session` may be shown: every one (anything goes,
/// 2026-11-22). A pass-through until its caller goes (the sweep).
pub fn digest_threads<'b>(
    _runner: &TurnRunner,
    _session: &str,
    _here: &str,
    live: Vec<&'b theseus_protocol::threads::ThreadInfo>,
) -> Vec<&'b theseus_protocol::threads::ThreadInfo> {
    live
}

/// The standing records, of `(id, source)`, a turn of `asker` may not be shown: none (anything goes,
/// 2026-11-22). A pass-through until its caller goes (the sweep).
pub fn standing_held<'r>(
    _runner: &TurnRunner,
    _asker: &str,
    _records: impl Iterator<Item = (&'r str, &'r str)>,
) -> std::collections::HashSet<String> {
    std::collections::HashSet::new()
}

/// How recall reads places for one asker. Narrowed to one place, the asker
/// reads as that place and its own sessions as it, and every other session
/// as it always has, which the place rule then drops for `place`. A channel
/// bound private is always narrowed to itself (decision 5); so is a turn that
/// is not the owner alone, to its exact arrival place.
/// Otherwise places read as before.
#[derive(Clone, Debug, Default)]
pub struct RecallPlaces {
    own: Option<String>,
    channel: bool,
}

impl RecallPlaces {
    /// The places recall reads for a turn of `asker`: every place (anything goes, 2026-11-22).
    pub fn of(_runner: &TurnRunner, _asker: Option<&str>) -> Self {
        Self::default()
    }

    /// The channel bound private the asker speaks in, when it does.
    pub fn channel(&self) -> Option<&str> {
        self.own.as_deref().filter(|_| self.channel)
    }

    /// The one place the asker draws on, when it is narrowed.
    pub fn narrowed(&self) -> Option<&str> {
        self.own.as_deref()
    }

    /// The asker's own place, read as recall reads it: the widest, so that it draws on every place
    /// (anything goes, 2026-11-22).
    pub fn asker(&self, _today: Place) -> Place {
        Place::Private
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
    pub fn may_read(&self, _runner: &TurnRunner, _session: &str) -> bool {
        true
    }
}

/// The verified point of view of one turn, for the view's gate: made here
/// alone, from the turn's presence and today's reading of its place, never
/// from what a caller says of itself.
#[derive(Clone, Debug)]
pub struct Verified(PointOfView);

impl Verified {
    /// The turn of `session`'s: the widest ([`widest`]), at its arrival place;
    /// a task's at its own place.
    pub fn of_session(runner: &TurnRunner, session: &str) -> Self {
        let anchor = match crate::task_graph::place::of_task_session(&runner.store, session) {
            Some(own) => own,
            None => place_name(runner, session).unwrap_or_else(|| "unknown".into()),
        };
        Verified(widest(anchor))
    }

    /// The point of view a next turn in `session` at `place` would get, for a preview of its view (no turn
    /// runs): the widest, at `place`.
    pub fn of_preview(_runner: &TurnRunner, _session: &str, place: &str) -> Self {
        Verified(widest(place.into()))
    }

    /// [`Self::of_preview`] for a session that does not exist yet, at `place` (a census of a fresh session's
    /// first request): the widest, at `place` (anything goes, 2026-11-22).
    pub fn of_fresh(_today: Place, place: &str) -> Self {
        Verified(widest(place.into()))
    }

    /// The point of view a next turn of the task whose session is `session` would get, for a census of it (no
    /// turn runs): the widest, at the task's own place. None for a session that is not a task's.
    pub fn of_task(runner: &TurnRunner, session: &str) -> Option<Self> {
        // Anything goes (2026-11-22): the widest, at the task's own place.
        let rec: SessionRecord = runner.store.get_session(session).ok()??;
        rec.task?;
        Some(Verified(widest(crate::task_graph::place::of_session(
            session,
        ))))
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

/// Seat `session`'s turn with `present` at `place`, for a test: nothing now (anything goes, 2026-11-22;
/// no presence is held). Kept for the tests that call it until the sweep.
#[cfg(test)]
pub fn seat_for_tests(
    _runner: &TurnRunner,
    _session: &str,
    _turn_id: &str,
    _present: Present,
    _place: &str,
) {
}

/// Seat the owner alone in `session`, for a test: nothing now (no presence is held). Kept for the tests
/// that call it until the sweep.
#[cfg(test)]
pub fn seat_owner(_runner: &TurnRunner, _session: &str) {}
