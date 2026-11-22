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

mod moved;
pub use moved::{moved_at_all, sources_moved, Moved};

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

/// Whether thread `slug`, or one it was merged into, is sealed or a seal is
/// proposed; one the board does not hold, or a chain too long, is unread.
fn thread_seal(board: &Board, slug: &str) -> Seal {
    let mut at = slug.to_string();
    for _ in 0..DEPTH {
        let Some(t) = board.get(&at) else {
            return Seal::Unread;
        };
        if t.sealed.is_some() || t.seal_pending.is_some() {
            return Seal::Held;
        }
        match &t.merged_into {
            Some(next) => at = next.clone(),
            None => return Seal::Clear,
        }
    }
    Seal::Unread
}

/// Whether a reader in `asker` may be told of thread `slug` (its name, its
/// state, that it exists), decided on `board`, the one snapshot the caller
/// then words its answer from: only when its seal is proven clear for that
/// reader. A sealed or seal-pending thread, or one that cannot be read, is
/// to this reader as if there were no such thread (issue 44).
pub fn may_name_on(store: &Store, board: &Board, asker: &str, slug: &str) -> bool {
    seal_for(store, board, Some(asker), slug) == Seal::Clear
}

/// The seal of `session` for a reader in `asker`, whatever `[threads]
/// enabled` says: `Held` when it serves a sealed thread, or one whose seal
/// is proposed, and the asker is not that thread; `Unread` when that cannot
/// be proven either way.
pub fn seal_of(runner: &TurnRunner, asker: Option<&str>, session: &str) -> Seal {
    match board(runner, session) {
        Some(b) => seal_on(&runner.store, &b, asker, session),
        None => Seal::Unread,
    }
}

fn board(runner: &TurnRunner, what: &str) -> Option<std::sync::Arc<Board>> {
    match runner.threads.all(&runner.store) {
        Ok(b) => Some(b),
        Err(e) => {
            tracing::warn!(what, error = %format!("{e:#}"),
                "the threads cannot be read: what they may seal is held out");
            None
        }
    }
}

/// The seal of a slug for a reader in `asker`: its own thread is clear.
fn seal_for(store: &Store, board: &Board, asker: Option<&str>, slug: &str) -> Seal {
    let own = asker.and_then(|a| thread_of(store, board, a, 0).ok().flatten());
    match thread_seal(board, slug) {
        Seal::Held if own.as_deref() == Some(slug) => Seal::Clear,
        other => other,
    }
}

fn seal_on(store: &Store, board: &Board, asker: Option<&str>, session: &str) -> Seal {
    match thread_of(store, board, session, 0) {
        Ok(None) => Seal::Clear,
        Ok(Some(slug)) => seal_for(store, board, asker, &slug),
        Err(e) => {
            tracing::warn!(session, error = %format!("{e:#}"),
                "a session's thread cannot be proven: it is held out");
            Seal::Unread
        }
    }
}

/// The seal of one node, for a reader in `asker`: its session's, and, where
/// a split or a claim moved it into a thread (`moved.of.<node>`), that
/// thread's too. A `moved.of` record that does not decode, or names a thread
/// the board does not hold, is `Unread`.
pub fn node_seal(
    store: &Store,
    board: &Board,
    asker: Option<&str>,
    session: &str,
    node: &str,
) -> Seal {
    let own = seal_on(store, board, asker, session);
    if own != Seal::Clear {
        return own;
    }
    match store.get_meta::<Option<crate::moved::MovedTo>>(&crate::moved::moved_of_key(node)) {
        Ok(Some(Some(m))) => seal_for(store, board, asker, &m.thread),
        Ok(_) => Seal::Clear,
        Err(e) => {
            tracing::warn!(node, error = %format!("{e:#}"),
                "where a node was moved cannot be read: it is held out");
            Seal::Unread
        }
    }
}

/// The nodes of `nodes` a reader in `asker` may read for their seal (by
/// session and by node, `node_seal`): what a read by id or by session shows.
pub fn readable_nodes<T>(
    runner: &TurnRunner,
    asker: Option<&str>,
    nodes: Vec<T>,
    of: impl Fn(&T) -> (&str, &str),
) -> Vec<T> {
    let Some(b) = board(runner, "a read of nodes") else {
        return Vec::new();
    };
    nodes
        .into_iter()
        .filter(|n| {
            let (session, node) = of(n);
            node_seal(&runner.store, &b, asker, session, node) == Seal::Clear
        })
        .collect()
}

/// Take out of the index's hits every one a reader in `asker` may not draw
/// on for its seal, by session and by node, before anything ranks it,
/// reranks it, or sends it to a model: whatever `[threads] enabled` says,
/// and whatever the thread lens would say. What was taken out, as `sealed`
/// drops (ids and scores, never text), for the recall's manifest to say.
pub fn seal_out(
    runner: &TurnRunner,
    asker: Option<&str>,
    answer: &mut Answer,
) -> Vec<theseus_protocol::memory::RecallDrop> {
    let Answer::Hits(r) = answer else {
        return Vec::new();
    };
    let board = board(runner, "recall's hits");
    let mut by_session: BTreeMap<String, Seal> = BTreeMap::new();
    let mut out = Vec::new();
    r.hits.retain(|h| {
        let clear = board.as_ref().is_some_and(|b| {
            let session = *by_session
                .entry(h.session_id.clone())
                .or_insert_with(|| seal_on(&runner.store, b, asker, &h.session_id));
            session == Seal::Clear
                && node_seal(&runner.store, b, asker, &h.session_id, &h.node_id) == Seal::Clear
        });
        if !clear {
            out.push(theseus_protocol::memory::RecallDrop {
                node_id: h.node_id.clone(),
                chunk: h.chunk,
                session_id: h.session_id.clone(),
                reason: "sealed".into(),
                fused: h.fused,
                tokens: 0,
                retention: None,
                own_top: None,
                share: None,
                paid: None,
            });
        }
        clear
    });
    out
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

/// One turn's presence, and where it arrived: made only here, from proof
/// ([`begin`], [`steered`]), and read whole by the turn's readers.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Asking {
    turn_id: String,
    present: Present,
    /// The exact arrival place, as the ledger names it.
    place: String,
}

impl Asking {
    pub fn turn_id(&self) -> &str {
        &self.turn_id
    }

    pub fn present(&self) -> Present {
        self.present
    }

    pub fn place(&self) -> &str {
        &self.place
    }
}

/// Each session's presence, as its running or latest turn computed it: what
/// the tools of that turn read, taken whole when a call starts. Lost at a
/// restart, when a session with none reads as not the owner alone. Written
/// only by [`begin`] and [`steered`].
#[derive(Default)]
pub struct Presences(Mutex<HashMap<String, Asking>>);

/// The most sessions held before the table starts again.
const PRESENCES_MAX: usize = 4_096;

impl Presences {
    fn put(&self, session: &str, a: Asking) {
        let mut m = self.0.lock().unwrap_or_else(|e| e.into_inner());
        if m.len() >= PRESENCES_MAX && !m.contains_key(session) {
            m.clear();
        }
        m.insert(session.to_string(), a);
    }

    /// The presence of `session`'s turn, taken whole.
    pub fn get(&self, session: &str) -> Option<Asking> {
        self.0
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .get(session)
            .cloned()
    }
}

/// What a turn's presence is computed from.
#[derive(Clone, Copy, Debug)]
pub enum Proof<'a> {
    /// The mark its opening message's submission carried.
    Mark(&'a OwnerMark),
    /// No message opened it: the presence of what set it up, when found.
    SetUp(Option<Present>),
}

/// The turn `turn_id` of `session` begins: its presence, computed from
/// `proof` at today's reading of the place it arrived in (`place`), is held
/// for its readers. The one way a turn's presence is made.
pub fn begin(
    runner: &TurnRunner,
    session: &str,
    turn_id: &str,
    place: String,
    proof: Proof<'_>,
) -> Asking {
    let today = runner.place_of(session);
    let present = match proof {
        Proof::Mark(m) => present_of_mark(&today, m),
        Proof::SetUp(s) => present(&today, Some(&place), Input::SetUp(s), &runner.store),
    };
    let a = Asking {
        turn_id: turn_id.into(),
        present,
        place,
    };
    runner.presence.put(session, a.clone());
    a
}

/// A message absorbed as a steer into the running turn of `session`, its
/// submission's mark `mark`: the turn's presence narrows to it at once, and
/// never widens. A steer with no mark is not the owner alone.
pub fn steered(runner: &TurnRunner, session: &str, mark: Option<&OwnerMark>) {
    let today = runner.place_of(session);
    let p = mark.map_or(Present::PrivateChannel, |m| present_of_mark(&today, m));
    let now = runner.presence.get(session);
    if let Some(mut a) = now {
        a.present = a.present.max(p);
        runner.presence.put(session, a);
    }
}

/// The exchange of the turn of `session` that made the call `call_id` (a
/// `wake.at` or a `task.create`).
fn set_up_exchange(
    store: &Store,
    session: &str,
    call_id: &str,
) -> Option<crate::exchange::Exchange> {
    let nodes = store.session_nodes(session).ok()?;
    let turn = nodes.iter().find_map(|(_, n)| match &n.body {
        Body::ToolCall { tool_use_id, .. } if tool_use_id == call_id => n.turn_id.clone(),
        _ => None,
    })?;
    crate::exchange::exchange(store, &turn).ok()?
}

/// The presence of an exchange at `class`: the narrowest of its messages'
/// marks, its steers included, so a steer that narrowed it narrows what it
/// set up. None when its ask has no mark; a steer with none is not alone.
fn exchange_presence(
    store: &Store,
    class: &Place,
    ex: &crate::exchange::Exchange,
) -> Option<Present> {
    let ask = ex.ask.as_deref()?;
    let ids: Vec<&str> = match ex.messages.is_empty() {
        true => vec![ask],
        false => ex.messages.iter().map(String::as_str).collect(),
    };
    let mut out = None::<Present>;
    for id in ids {
        let p = match crate::exchange::mark(store, id).ok().flatten() {
            Some(m) => present_of_mark(class, &m),
            None if id == ask => return None,
            None => Present::PrivateChannel,
        };
        out = Some(out.map_or(p, |o| o.max(p)));
    }
    out
}

/// The presence of the turn of `session` that made the call `call_id` (a
/// `wake.at` or a `task.create`), read from what the ledger wrote of it: that
/// turn's exchange, its messages and their owner's marks (`exchange_presence`),
/// at `class` (today's reading of `session`'s place). None when any of it is
/// not there: a turn no message opened, or one from before the ledger.
pub fn set_up(store: &Store, class: &Place, session: &str, call_id: &str) -> Option<Present> {
    let ex = set_up_exchange(store, session, call_id)?;
    exchange_presence(store, class, &ex)
}

/// The presence of the turn `turn_id`, which a turn no message opened continues (issue 44: it was
/// interrupted, or its call is answered now): its exchange's, read as [`set_up`] reads one, at `class`.
/// None when it has no exchange, or its ask no mark.
pub fn continued(store: &Store, class: &Place, turn_id: &str) -> Option<Present> {
    let ex = crate::exchange::exchange(store, turn_id).ok()??;
    exchange_presence(store, class, &ex)
}

/// Whether the report of the task `task` (its session) may be written as the
/// input of a turn of `delivery`, the session it reports to. Not when the
/// place it reports to has moved since the task started, nor when the
/// presence of the turn that started it would be narrower there now than it
/// was then: its sources may not be shown there. With the ledger off, as
/// before.
pub fn report_shown(runner: &TurnRunner, delivery: &str, task: &str) -> bool {
    if !runner.cfg.context.ledger_on() {
        return true;
    }
    let Ok(Some(rec)) = runner.store.get_session::<SessionRecord>(task) else {
        return false;
    };
    let Some(of) = rec.task else {
        return true;
    };
    let now = runner.outbox.try_target(delivery).ok().flatten();
    if now != of.target {
        return false;
    }
    let Some(ex) = set_up_exchange(&runner.store, &of.parent_session, &of.by) else {
        // No proof of what started it: as before, where it has not moved.
        return true;
    };
    // A task's exchange is at the task's own place, not a binding: then, it spoke where it speaks now.
    let then_class = match (ex.private, crate::task_graph::place::task_of(&ex.place)) {
        (true, _) => Place::Private,
        (false, Some(_)) => Place::Shared(now.unwrap_or_default()),
        (false, None) => Place::Shared(ex.place.clone()),
    };
    let today = runner.place_of(delivery);
    match (
        exchange_presence(&runner.store, &then_class, &ex),
        exchange_presence(&runner.store, &today, &ex),
    ) {
        (Some(then), Some(now)) => now <= then,
        (None, None) => today == then_class || today == Place::Private,
        _ => false,
    }
}

/// What a turn is given in place of a report it may not read here.
pub fn withheld_report(short: &str) -> String {
    format!(
        "[task {short} has finished, and its report is waiting where the owner is alone: it may hold \
         words from a place this conversation may not see, so it is not shown here]"
    )
}

/// The sources of earlier `Recall` nodes a turn of `asker` may still be
/// shown, under its presence and the seals as they are now: a note admitted
/// when the owner was alone does not ride into an agent's turn, nor one
/// whose thread was sealed since. A source taken out renders as one that
/// could not be read, with no text.
pub fn readmit_sources(
    runner: &TurnRunner,
    asker: &str,
    sources: &mut crate::recall::render::Sources,
) {
    if sources.is_empty() {
        return;
    }
    let places = RecallPlaces::of(runner, Some(asker));
    let here = places.asker(runner.place_of(asker));
    let board = board(runner, "earlier recalls");
    sources.retain(|_, n| {
        let Some(b) = &board else {
            return false;
        };
        here.may_draw_on(&places.of_session(runner, &n.session_id))
            && node_seal(&runner.store, b, Some(asker), &n.session_id, &n.id) == Seal::Clear
    });
}

/// The threads of a place digest a turn of `session` may be shown: only
/// where the owner is alone does it name other threads (they are other
/// places); otherwise its own thread `here` alone. With the ledger off, as
/// before.
pub fn digest_threads<'b>(
    runner: &TurnRunner,
    session: &str,
    here: &str,
    live: Vec<&'b theseus_protocol::threads::ThreadInfo>,
) -> Vec<&'b theseus_protocol::threads::ThreadInfo> {
    if !runner.cfg.context.ledger_on() {
        return live;
    }
    let alone = runner
        .presence
        .get(session)
        .is_some_and(|a| a.present == Present::OwnerAlone);
    live.into_iter()
        .filter(|t| alone || t.slug == here)
        .collect()
}

/// The standing records, of `(id, source)`, a turn of `asker` may not be
/// shown: each kept from a message (`capture:<node>`) of a conversation the
/// gate does not admit for it, or whose message cannot be read. A record
/// from no conversation (the owner's own surfaces) is source-free.
pub fn standing_held<'r>(
    runner: &TurnRunner,
    asker: &str,
    records: impl Iterator<Item = (&'r str, &'r str)>,
) -> std::collections::HashSet<String> {
    let places = RecallPlaces::of(runner, Some(asker));
    let board = board(runner, "standing records");
    records
        .filter(|(_, source)| {
            let Some(node) = source.strip_prefix("capture:") else {
                return false;
            };
            let Ok(Some((_, n))) = runner.store.get_node(node) else {
                return true;
            };
            if n.session_id == asker {
                return false;
            }
            let Some(b) = &board else {
                return true;
            };
            !places.may_read(runner, &n.session_id)
                || node_seal(&runner.store, b, Some(asker), &n.session_id, &n.id) != Seal::Clear
        })
        .map(|(id, _)| id.to_string())
        .collect()
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
    /// The places recall reads for a turn of `asker`, its presence as its
    /// turn holds it now.
    pub fn of(runner: &TurnRunner, asker: Option<&str>) -> Self {
        let asking = asker.and_then(|a| runner.presence.get(a));
        Self::of_asking(runner, asker, asking.as_ref())
    }

    /// The places recall reads for a turn of `asker` whose presence is
    /// `asking`, taken whole when its reader started.
    pub fn of_asking(runner: &TurnRunner, asker: Option<&str>, asking: Option<&Asking>) -> Self {
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
        Self::narrowed_to(runner, asker, asking)
    }

    fn narrowed_to(runner: &TurnRunner, asker: &str, asking: Option<&Asking>) -> Self {
        match asking {
            Some(a) if a.present == Present::OwnerAlone => Self::default(),
            Some(a) => RecallPlaces {
                own: Some(a.place.clone()),
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
    /// computed, `Unknown`. A task's is at its own place ([`task_pov`]).
    pub fn of_session(runner: &TurnRunner, session: &str) -> Self {
        let place = runner.place_of(session);
        let (present, anchor) = match runner.presence.get(session) {
            Some(a) => (a.present, a.place),
            None => (
                Present::Unknown,
                place_name(runner, session).unwrap_or_else(|| "unknown".into()),
            ),
        };
        let pov = PointOfView {
            anchor,
            place,
            present,
        };
        match crate::task_graph::place::of_task_session(&runner.store, session) {
            Some(own) => Verified(task_pov(pov, own)),
            None => Verified(pov),
        }
    }

    /// The point of view a next turn in `session` at `place` would get, for a preview of its view (no turn
    /// runs): today's class of `session`'s place, and a message typed there by the owner in person, read by
    /// the rule a turn's mark is read by ([`present_of_mark`]). A channel is never the owner alone.
    pub fn of_preview(runner: &TurnRunner, session: &str, place: &str) -> Self {
        struct InPerson;
        impl OwnerMarks for InPerson {
            fn in_person(&self, _: &str) -> Result<Option<bool>> {
                Ok(Some(true))
            }
        }
        let today = runner.place_of(session);
        let present = present(&today, Some(place), Input::Message(""), &InPerson);
        Verified(PointOfView {
            anchor: place.into(),
            place: today,
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

/// How many of a session's newest earlier turns [`own_turns`] looks at: twice the most a window holds.
const OWN_TURNS: usize = 4 * crate::ladder::N;

/// The exchanges `session` itself ran at `place` before `turn`, among its newest [`OWN_TURNS`] turns: what a
/// turn of it is always shown at that place, whoever is present (`view::permit::Permit::speaking`). An
/// exchange whose record cannot be read is not among them.
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
            Ok(Some(e)) => e.session_id == session && e.place == place,
            _ => false,
        })
        .collect()
}

/// A task's point of view (issue 44, a task's own view): anchored at its own
/// place `own` (`task_graph::place`), today's reading of where it speaks
/// renamed to it, and its presence as its set-up proved it. A set-up that
/// could not be proven reads as not the owner alone (`PrivateChannel`), not
/// `Unknown`: at the task's own place that admits only what is of that place,
/// the task's own exchanges and card, which only its own turns write (the
/// trust principle; nothing of another place is admitted without the owner
/// alone).
fn task_pov(pov: PointOfView, own: String) -> PointOfView {
    let place = match pov.place {
        Place::Shared(_) => Place::Shared(own.clone()),
        p => p,
    };
    let present = match pov.present {
        Present::Unknown if place != Place::Unknown => Present::PrivateChannel,
        p => p,
    };
    PointOfView {
        anchor: own,
        place,
        present,
    }
}

/// The mark `turn.submit` would make for a message to `session`: typed by
/// the owner in person, or by an agent's process at the terminal.
#[cfg(test)]
pub fn test_mark(runner: &TurnRunner, session: &str, in_person: bool) -> OwnerMark {
    OwnerMark {
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

/// Seat `session`'s turn `turn_id` with `present` at `place`, for a test of
/// a READER of the presence that needs one no proof would give it (a turn in
/// company, an unknown one). Tests only: a turn's presence is made by
/// [`begin`] and [`steered`] alone.
#[cfg(test)]
pub fn seat_for_tests(
    runner: &TurnRunner,
    session: &str,
    turn_id: &str,
    present: Present,
    place: &str,
) {
    runner.presence.put(
        session,
        Asking {
            turn_id: turn_id.into(),
            present,
            place: place.into(),
        },
    );
}

/// Seat the owner alone in `session`, as a turn of his in person would, for
/// a test that calls a tool past any turn.
#[cfg(test)]
pub fn seat_owner(runner: &TurnRunner, session: &str) {
    let mark = test_mark(runner, session, true);
    let place = mark.place.clone();
    begin(runner, session, "turn_test", place, Proof::Mark(&mark));
}
