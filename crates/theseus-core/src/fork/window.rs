//! A fork's window in the view (issue 44): the exchanges of the run a fork inherits are in its window, beside
//! its place's own. A fork speaks in no place of its source's (the CLI's or the web UI's), so the place's
//! ladder alone would show the fork's place, not the conversation it forked from. The inherited exchanges are
//! the source's own records, keyed by the source's turn ids; each passes the gate as any window exchange
//! does (its session's seal, a move into a thread, its place's class against who is present), so a sealed
//! source gives nothing.

use std::collections::HashSet;

use crate::ladder::view::{listed, StoreLadder};
use crate::store::Store;
use crate::stub::Kind;
use crate::view::fold::{Ladder, LadderRead};
use crate::view::timeline::ExchangeRef;
use crate::view::Pos;

/// The ladder a view of a session reads: the store's, with a fork's inherited run in its window.
pub struct ForkLadder<'a> {
    inner: StoreLadder<'a>,
    inherited: Vec<ExchangeRef>,
}

impl<'a> ForkLadder<'a> {
    /// `session`'s. A session that is no fork reads the store's ladder alone; so does a fork whose inherited
    /// run cannot be read (nothing of it is shown, and the reason is logged).
    pub fn of(store: &'a Store, session: &str) -> Self {
        let inherited = inherited(store, session).unwrap_or_else(|e| {
            tracing::warn!(session, error = %format!("{e:#}"),
                "a fork's inherited run cannot be read; its window holds its place's exchanges alone");
            Vec::new()
        });
        Self {
            inner: StoreLadder(store),
            inherited,
        }
    }
}

/// The exchanges of the run `session` inherits as a fork, oldest first: each inherited turn whose exchange
/// the ledger holds (keyed by the source's turn id, run by the session that wrote the node) and whose final
/// reply is in the run, so nothing past the fork point is laid. None for a session that is no fork.
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

impl Ladder for ForkLadder<'_> {
    /// The place's ladder; its window with the inherited exchanges after the place's `/new` mark beside the
    /// place's own, oldest first, at most the newest the window holds before a fold is due (`2n - 1`).
    fn read(&self, place: &str, n: usize, since: Option<&Pos>) -> anyhow::Result<LadderRead> {
        let mut read = self.inner.read(place, n, since)?;
        if self.inherited.is_empty() {
            return Ok(read);
        }
        let own: HashSet<String> = read.window.iter().map(|e| e.turn_id.clone()).collect();
        let after = |e: &ExchangeRef| since.is_none_or(|s| e.at_ms > s.at_ms);
        let mut window: Vec<ExchangeRef> = (self.inherited.iter())
            .filter(|e| after(e) && !own.contains(&e.turn_id))
            .cloned()
            .chain(std::mem::take(&mut read.window))
            .collect();
        window.sort_by(|a, b| (a.at_ms, &a.turn_id).cmp(&(b.at_ms, &b.turn_id)));
        let most = (2 * n).saturating_sub(1).max(1);
        read.window = window.split_off(window.len().saturating_sub(most));
        Ok(read)
    }
}
