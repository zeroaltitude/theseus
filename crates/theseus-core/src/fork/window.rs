//! A fork's window in the view (issue 44): a fork carries the run it was made from. A fork speaks in no place
//! of its source's (the CLI's or the web UI's), so the place's ladder alone would show the fork's place, not
//! the conversation it forked from. The exchanges of the run it inherits (the source's own records, keyed by
//! the source's turn ids, each through the fork point) are candidates of its window beside its place's own,
//! with no count of their own: the window's admission and the token budgeter choose among them, as among any
//! window's exchanges.
//!
//! **Who sees the inherited run.** Where the owner is alone in a private place, the inherited run is the fork's
//! OWN history for the place rule, whatever its source's place (a shared channel's included): the permit reads
//! it as the anchor's (`view::permit::Permit::owning`). Its source's seal, a seal pending, a source that cannot
//! be read and a message moved into a thread not proven clear still hold each exchange out, and every piece
//! keeps its source's place and ids. Anywhere else (others present) the inherited run passes the rule as any
//! other place's exchange does, so nothing of a private source is shown in company, and a harness line beside
//! the message says the fork's source is not shown, rather than presenting a silently empty fork.
//!
//! **A fork never holds what an ancestor said after the point it was forked from it** (the conductor's rule,
//! 2026-11-22: a fork exists to take the other road). The cut is `fork::cut`: each ancestor is cut at its own
//! point, the earliest on the way down; an exchange it began after the point is no candidate of any selector,
//! one the point falls in is laid through the point and never by its summary, and a fold that may cover what
//! came after is not laid. The inherited run holds the exchange the point falls in (the person's message at
//! the point, its answer not), wherever the fork's place is. Everything else of the place is the view's as for
//! any session: the place's other sessions, the owner's other places (slot F) and the ladder's other folds
//! show what was said since, by others or elsewhere. A read the cut needs that fails fails the turn.

use std::collections::HashSet;

use crate::fork::cut::{Cut, Reach};

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
    cut: Cut,
    alone: bool,
    session: String,
    turn: String,
}

impl Fork {
    /// The fork of `session`'s turn `turn` and its permit, for the point of view `verified`. A session that
    /// is no fork inherits nothing. A fork whose inherited run or cut cannot be read is an error: the turn
    /// fails before anything is sent, never with its point unenforced.
    pub fn permit(
        store: &Store,
        session: &str,
        turn: &str,
        verified: Verified,
    ) -> anyhow::Result<(Self, Permit)> {
        let inherited = inherited(store, session)?;
        let cut = Cut::of(store, session)?;
        let pov = verified.pov();
        let alone = pov.present == Present::OwnerAlone && pov.place == Place::Private;
        let own = inherited.iter().map(|e| e.turn_id.clone());
        let spoken = crate::view_gate::own_turns(store, session, turn, &pov.anchor);
        let permit = Permit::of(verified).owning(own.collect::<Vec<_>>());
        let permit = permit.speaking(spoken);
        let fork = Self {
            inherited,
            cut,
            alone,
            session: session.into(),
            turn: turn.into(),
        };
        Ok((fork, permit))
    }

    /// The ladder the view reads: the store's, with the inherited run among its window's candidates.
    pub fn ladder<'a>(&'a self, store: &'a Store) -> ForkLadder<'a> {
        ForkLadder {
            inner: StoreLadder(store),
            inherited: &self.inherited,
            cut: &self.cut,
        }
    }

    /// The sessions an assembly of this fork reads, cut at its point (`Sessions::cut`).
    pub fn sessions(&self) -> crate::view::sources_store::Sessions {
        crate::view::sources_store::Sessions::with_cut(self.cut.clone())
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
    cut: &'a Cut,
}

/// The exchanges of the run `session` inherits as a fork, oldest first: each inherited turn whose exchange
/// the ledger holds (keyed by the source's turn id, run by the session that wrote the node). The exchange the
/// fork point falls in is among them, however it ended: it is laid through the point (`fork::cut`), so a fork
/// at the person's message holds that message wherever the fork's place is. None for a session that is no fork.
pub fn inherited(store: &Store, session: &str) -> anyhow::Result<Vec<ExchangeRef>> {
    if crate::fork::origin_of(store, session)?.is_none() {
        return Ok(Vec::new());
    }
    let nodes = store.transcript(session)?;
    let run: Vec<_> = (nodes.iter().map(|(_, n)| n))
        .filter(|n| n.session_id != session)
        .collect();
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
        if ex.session_id == n.session_id {
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
    /// The place's ladder, less what an ancestor began after its point and any fold that may cover it; its window with
    /// every inherited exchange after the place's `/new` mark (by the whole position, as the store's ladder
    /// compares it) beside the place's own, oldest first. No count is cut here: the window's admission and
    /// the budgeter choose.
    fn read(&self, place: &str, n: usize, since: Option<&Pos>) -> anyhow::Result<LadderRead> {
        let mut read = self.inner.read(place, n, since)?;
        if !self.cut.is_empty() {
            // Nothing an ancestor began after its point, nor a fold that may cover what came after it.
            let mut window = Vec::with_capacity(read.window.len());
            for e in std::mem::take(&mut read.window) {
                let beyond = match crate::exchange::exchange(self.inner.0, &e.turn_id)? {
                    Some(ex) => self.cut.reach(self.inner.0, &ex)? == Reach::Beyond,
                    None => false,
                };
                if !beyond {
                    window.push(e);
                }
            }
            read.window = window;
            read.nodes.retain(|f| !self.cut.folds_past(f));
        }
        if self.inherited.is_empty() {
            return Ok(read);
        }
        let own: HashSet<String> = read.window.iter().map(|e| e.turn_id.clone()).collect();
        let after = |e: &ExchangeRef| since.is_none_or(|s| pos(e) > *s);
        let mut window: Vec<ExchangeRef> = (self.inherited.iter())
            .filter(|e| after(e) && !own.contains(&e.turn_id))
            .cloned()
            .chain(std::mem::take(&mut read.window))
            .collect();
        window.sort_by(|a, b| (a.at_ms, &a.turn_id).cmp(&(b.at_ms, &b.turn_id)));
        read.window = window;
        Ok(read)
    }
}
