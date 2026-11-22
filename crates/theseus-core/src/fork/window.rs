//! A fork's window in the view (issue 44): a fork carries the run it was made from. A fork speaks in no place
//! of its source's (the CLI's or the web UI's), so the place's ladder alone would show the fork's place, not
//! the conversation it forked from. The exchanges of the run it inherits (the source's own records, keyed by
//! the source's turn ids, each ended inside the run) are candidates of its window beside its place's own, with
//! no count of their own: the window's admission and the token budgeter choose among them, as among any
//! window's exchanges, and the place's own folds stay as they are.
//!
//! **Who sees the inherited run.** Where the owner is alone in a private place, the inherited run is the fork's
//! OWN history for the place rule, whatever its source's place (a shared channel's included): the permit reads
//! it as the anchor's (`view::permit::Permit::owning`). Its source's seal, a seal pending, a source that cannot
//! be read and a message moved into a thread not proven clear still hold each exchange out, and every piece
//! keeps its source's place and ids. Anywhere else (others present) the inherited run passes the rule as any
//! other place's exchange does, so nothing of a private source is shown in company, and a harness line beside
//! the message says the fork's source is not shown, rather than presenting a silently empty fork.
//!
//! **A fork is a live view of its place with an inherited prefix, NOT a historical snapshot.** Only the run
//! this module adds stops at the fork point. The rest of the view reads the store as it is now: the place's own
//! window (another session of the fork's place, its source included when it shares the place), the owner's
//! other places (slot F) and the ladder's folds all show what was said after the fork point. Nothing here
//! claims fork-point isolation.

use std::collections::HashSet;

use theseus_memory::gate::Present;
use theseus_memory::recall::Place;

use crate::ladder::view::{listed, StoreLadder};
use crate::node::{Body, Node};
use crate::store::Store;
use crate::stub::Kind;
use crate::view::fold::{Ladder, LadderRead};
use crate::view::permit::Permit;
use crate::view::timeline::ExchangeRef;
use crate::view::Pos;
use crate::view_gate::Verified;

/// The line laid beside the message of a fork's turn whose source is not shown because others are present.
pub const NOT_SHOWN: &str =
    "This conversation is a fork of another. What was said in the conversation it was \
                             made from is not shown here, because others are present.";

/// A turn's view of its session as a fork: the run it inherits, and whether the owner is alone in a private
/// place (where that run is the fork's own history).
pub struct Fork {
    inherited: Vec<ExchangeRef>,
    moved_in: Vec<ExchangeRef>,
    alone: bool,
    session: String,
    turn: String,
}

impl Fork {
    /// The fork of `session`'s turn `turn` and its permit, for the point of view `verified`. A session that
    /// is no fork inherits nothing; nor does a fork whose inherited run cannot be read (nothing of it is
    /// shown, and the reason is logged).
    pub fn permit(store: &Store, session: &str, turn: &str, verified: Verified) -> (Self, Permit) {
        let inherited = inherited(store, session).unwrap_or_else(|e| {
            tracing::warn!(session, error = %format!("{e:#}"),
                "a fork's inherited run cannot be read; its window holds its place's exchanges alone");
            Vec::new()
        });
        let pov = verified.pov();
        let alone = pov.present == Present::OwnerAlone && pov.place == Place::Private;
        let own = inherited.iter().map(|e| e.turn_id.clone());
        let spoken = crate::view_gate::own_turns(store, session, turn, &pov.anchor);
        let permit = Permit::of(verified).owning(own.collect::<Vec<_>>());
        let permit = permit.speaking(spoken);
        // A thread a split moved turns into lays them (`fork::split`); one that cannot be read lays none.
        let moved_in = super::split::moved_in(store, session).unwrap_or_else(|e| {
            tracing::warn!(session, error = %format!("{e:#}"), "a split's moved turns cannot be read here");
            Vec::new()
        });
        let fork = Self {
            inherited,
            moved_in,
            alone,
            session: session.into(),
            turn: turn.into(),
        };
        (fork, permit)
    }

    /// The ladder the view reads: the store's, with the inherited run among its window's candidates.
    pub fn ladder<'a>(&'a self, store: &'a Store) -> ForkLadder<'a> {
        ForkLadder {
            inner: StoreLadder(store),
            inherited: &self.inherited,
            session: &self.session,
            moved_in: &self.moved_in,
        }
    }

    /// The block beside the message, with the line that says a fork's source is not shown when others are
    /// present and it inherits a run; otherwise `beside` as it is.
    pub fn note(&self, beside: Option<Node>) -> Option<Node> {
        if self.alone || self.inherited.is_empty() {
            return beside;
        }
        let Some(mut node) = beside else {
            let block = crate::compiler::view::Frozen::block;
            return Some(block(&self.session, &self.turn, "", NOT_SHOWN));
        };
        if let Body::UserMessage { text, .. } = &mut node.body {
            text.push_str("\n\n");
            text.push_str(NOT_SHOWN);
        }
        Some(node)
    }
}

/// The ladder a view of a session reads: the store's, with a fork's inherited run in its window.
pub struct ForkLadder<'a> {
    inner: StoreLadder<'a>,
    inherited: &'a [ExchangeRef],
    /// The session read, and the exchanges a live split moved into it (`fork::split`).
    session: &'a str,
    moved_in: &'a [ExchangeRef],
}

/// The exchanges of the run `session` inherits as a fork, oldest first: each inherited turn whose exchange
/// the ledger holds (keyed by the source's turn id, run by the session that wrote the node) and whose final
/// reply is in the run, so no turn unfinished at the fork point is laid. None for a session that is no fork.
pub fn inherited(store: &Store, session: &str) -> anyhow::Result<Vec<ExchangeRef>> {
    if crate::fork::origin_of(store, session)?.is_none() {
        return Ok(Vec::new());
    }
    let nodes = store.transcript(session)?;
    let run: Vec<_> = (nodes.iter().map(|(_, n)| n))
        .filter(|n| n.session_id != session)
        .collect();
    let ids: HashSet<&str> = run.iter().map(|n| n.id.as_str()).collect();
    let (mut turns, mut out) = (HashSet::new(), Vec::new());
    for n in &run {
        let Some(turn) = n.turn_id.as_deref() else {
            continue;
        };
        if n.kind != Kind::UserMessage || !turns.insert(turn) {
            continue;
        }
        let Some(ex) = crate::exchange::exchange(store, turn)? else {
            continue;
        };
        let ended = ex.reply.as_deref().is_some_and(|r| ids.contains(r));
        if ex.session_id == n.session_id && ended {
            out.push(listed(&ex));
        }
    }
    out.sort_by(|a, b| (a.at_ms, &a.turn_id).cmp(&(b.at_ms, &b.turn_id)));
    Ok(out)
}

/// An exchange's position, as the ladder orders it and the `/new` mark is compared.
fn pos(e: &ExchangeRef) -> Pos {
    Pos {
        at_ms: e.at_ms,
        id: e.turn_id.clone(),
    }
}

impl Ladder for ForkLadder<'_> {
    /// The place's ladder (its folds as they are); its window with every inherited exchange after the place's
    /// `/new` mark (by the whole position, as the store's ladder compares it) beside the place's own, oldest
    /// first. No count is cut here: the window's admission and the budgeter choose.
    fn read(&self, place: &str, n: usize, since: Option<&Pos>) -> anyhow::Result<LadderRead> {
        let mut read = self.inner.read(place, n, since)?;
        // An exchange is laid where its opening message lives now (`fork::split`).
        let store = self.inner.0;
        (read.window).retain(|e| !super::split::moved_away(store, self.session, &e.turn_id));
        if self.inherited.is_empty() && self.moved_in.is_empty() {
            return Ok(read);
        }
        let own: HashSet<String> = read.window.iter().map(|e| e.turn_id.clone()).collect();
        let after = |e: &ExchangeRef| since.is_none_or(|s| pos(e) > *s);
        let mut window: Vec<ExchangeRef> = (self.inherited.iter())
            .chain(self.moved_in.iter())
            .filter(|e| after(e) && !own.contains(&e.turn_id))
            .cloned()
            .chain(std::mem::take(&mut read.window))
            .collect();
        window.sort_by(|a, b| (a.at_ms, &a.turn_id).cmp(&(b.at_ms, &b.turn_id)));
        read.window = window;
        Ok(read)
    }
}
