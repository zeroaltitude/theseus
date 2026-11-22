//! A fork's window in the view (issue 44): a fork carries the run it was made from. A fork speaks in no place
//! of its source's (the CLI's or the web UI's), so the place's ladder alone would show the fork's place, not
//! the conversation it forked from. The exchanges of the run it inherits (the source's own records, keyed by
//! the source's turn ids, each ended inside the run) are candidates of its window beside its place's own, with
//! no count of their own: the window's admission and the token budgeter choose among them, as among any
//! window's exchanges, and the place's own folds stay as they are.
//!
//! **Who sees the inherited run.** Every turn of the fork (anything goes, 2026-11-22): the permit admits it
//! as any piece, and every piece keeps its source's place and ids.
//!
//! **A fork is a live view of its place with an inherited prefix, NOT a historical snapshot.** Only the run
//! this module adds stops at the fork point. The rest of the view reads the store as it is now: the place's own
//! window (another session of the fork's place, its source included when it shares the place), the owner's
//! other places (slot F) and the ladder's folds all show what was said after the fork point. Nothing here
//! claims fork-point isolation.

use std::collections::HashSet;

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
}

impl Fork {
    /// The fork of `session`'s turn and its permit, for the point of view `verified`. A session that is no
    /// fork inherits nothing; nor does a fork whose inherited run cannot be read (nothing of it is shown, and
    /// the reason is logged).
    pub fn permit(store: &Store, session: &str, turn: &str, verified: Verified) -> (Self, Permit) {
        let inherited = inherited(store, session).unwrap_or_else(|e| {
            tracing::warn!(session, error = %format!("{e:#}"),
                "a fork's inherited run cannot be read; its window holds its place's exchanges alone");
            Vec::new()
        });
        let permit = Permit::scoped(store, session, turn, &verified);
        (Self { inherited }, permit)
    }

    /// The ladder the view reads: the store's, with the inherited run among its window's candidates.
    pub fn ladder<'a>(&'a self, store: &'a Store) -> ForkLadder<'a> {
        ForkLadder {
            inner: StoreLadder(store),
            inherited: &self.inherited,
        }
    }
}

/// The ladder a view of a session reads: the store's, with a fork's inherited run in its window.
pub struct ForkLadder<'a> {
    inner: StoreLadder<'a>,
    inherited: &'a [ExchangeRef],
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
