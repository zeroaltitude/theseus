//! A fork's window in the view (issue 44): a fork carries the run it was made from. A fork speaks in no place
//! of its source's (the CLI's or the web UI's), so the place's ladder alone would show the fork's place, not
//! the conversation it forked from. The exchanges of the run it inherits (the source's own records, keyed by
//! the source's turn ids, each through the fork point) are candidates of its window beside its place's own,
//! with no count of their own: the window's admission and the token budgeter choose among them, as among any
//! window's exchanges.
//!
//! **Who sees the inherited run.** Every turn of the fork (anything goes, 2026-11-22): the permit admits it
//! as any piece, and every piece keeps its source's place and ids. A fork of a self-loop session that does
//! not build lays its own place only, as that session does (`view_gate::Verified::own_place_only`), and its
//! inherited run besides: the run is handed material, as a brief is, so the scoped permit admits exactly the exchanges
//! [`inherited`] yields, each laid through the fork's cut, and nothing else of another session
//! (`view::permit::Permit::scoped`).
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
use crate::ladder::view::{listed, StoreLadder};
use crate::store::Store;
use crate::stub::Kind;
use crate::view::fold::{Ladder, LadderRead};
use crate::view::permit::Permit;
use crate::view::timeline::ExchangeRef;
use crate::view::Pos;
use crate::view_gate::Verified;

/// A turn's view of its session as a fork: the run it inherits.
pub struct Fork {
    inherited: Vec<ExchangeRef>,
    cut: Cut,
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
        // A fork of a self-loop session keeps the run it was handed: the scoped permit admits exactly these
        // exchanges besides its own place's, each laid through the cut (`Sessions::with_cut`) and no further.
        let handed: Vec<&str> = inherited.iter().map(|e| e.turn_id.as_str()).collect();
        let permit = Permit::scoped(store, (session, turn), &verified, &handed);
        Ok((Self { inherited, cut }, permit))
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
            let mut nodes = Vec::with_capacity(read.nodes.len());
            for f in std::mem::take(&mut read.nodes) {
                if self.cut.fold_inside(self.inner.0, &f.handle.0)? {
                    nodes.push(f);
                }
            }
            read.nodes = nodes;
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
