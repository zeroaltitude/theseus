//! A split on the view (issue 44): an exchange is laid in the view of the conversation its opening message
//! lives in now. `theseus thread split` moves a range's nodes (`moved.of.<node>`, the source's `Moved` node,
//! the thread's arrangement) and leaves the ledger's exchanges as they are, so the window reads where each
//! exchange's opening message lives:
//!
//! - **Moved away**: an exchange whose opening message a live split moved to another session leaves the
//!   window of every other session; in the source its place holds the split's one line (`moved::line`, as
//!   the source's `Moved` node renders it), where the range began ([`with_lines`]).
//! - **Moved in**: the thread's window holds the exchanges its arrangement names whose opening message a
//!   live split moved into this session ([`moved_in`]), in their order, at the split's time (an existing
//!   thread's tail). They are not its own: the gate admits
//!   each to the thread's point of view by its marks, and a sealed thread's range stays held from any reader
//!   the thread is not clear to (`view_gate::Moved`).
//!
//! An unsplit clears `moved.of.<node>` and marks the record undone: both windows read as before the split.
//! Nothing is written here and no stored exchange changes.

use std::collections::HashSet;

use crate::exchange;
use crate::ladder::view::listed;
use crate::moved::{self, MovedTo, SplitRecord};
use crate::node::Body;
use crate::store::Store;
use crate::stub::Stub;
use crate::view::timeline::ExchangeRef;

/// The session a live split moved node `id` into; none when it was never moved or its split is undone.
fn moved_into(store: &Store, id: &str) -> anyhow::Result<Option<String>> {
    let Some(Some(to)) = store.get_meta::<Option<MovedTo>>(&moved::moved_of_key(id))? else {
        return Ok(None);
    };
    let rec: Option<SplitRecord> = store.get_meta(&moved::split_key(&to.split))?;
    Ok(rec.filter(|r| !r.undone).map(|r| r.target_session))
}

/// Whether exchange `turn` is laid in another session than `session` now: its opening message was moved
/// away by a live split. An unreadable record holds it out (fail closed).
pub fn moved_away(store: &Store, session: &str, turn: &str) -> bool {
    let into = exchange::exchange(store, turn).and_then(|ex| match ex.and_then(|e| e.ask) {
        Some(ask) => moved_into(store, &ask),
        None => Ok(None),
    });
    match into {
        Ok(Some(target)) => target != session,
        Ok(None) => false,
        Err(e) => {
            tracing::warn!(turn, error = %format!("{e:#}"),
                "where an exchange's message was moved cannot be read: it is held out of the window");
            true
        }
    }
}

/// The exchanges a live split moved into `session`, in order: each its arrangements name by its opening
/// message, whose move targets this session, placed at the split's time (a split into an existing thread lays
/// them at its tail), their own order kept.
pub fn moved_in(store: &Store, session: &str) -> anyhow::Result<Vec<ExchangeRef>> {
    let (mut seen, mut out) = (HashSet::new(), Vec::new());
    for (_, n) in store.session_nodes(session)? {
        let Body::Arrangement { pieces, .. } = &n.body else {
            continue;
        };
        for p in pieces {
            let Some((_, node)) = store.get_node(&p.node)? else {
                continue;
            };
            let Some(turn) = node
                .turn_id
                .as_deref()
                .filter(|_| matches!(node.body, Body::UserMessage { .. }))
            else {
                continue;
            };
            let Some(ex) = exchange::exchange(store, turn)? else {
                continue;
            };
            if ex.ask.as_deref() != Some(node.id.as_str()) {
                continue;
            }
            let Some(Some(to)) =
                store.get_meta::<Option<MovedTo>>(&moved::moved_of_key(&node.id))?
            else {
                continue;
            };
            let Some(rec) = store.get_meta::<SplitRecord>(&moved::split_key(&to.split))? else {
                continue;
            };
            if !rec.undone && rec.target_session == session && seen.insert(ex.turn_id.clone()) {
                // At the thread's tail: a moved exchange stands where the split put it, its own order kept.
                out.push(ExchangeRef {
                    at_ms: rec.at_ms,
                    ..listed(&ex)
                });
            }
        }
    }
    out.sort_by(|a, b| (a.at_ms, &a.turn_id).cmp(&(b.at_ms, &b.turn_id)));
    Ok(out)
}

/// `window` with the one line of each live split out of `session`, where its range began: before the first
/// laid node after the range's first position, else at the end. A source with no live split is unchanged.
pub fn with_lines(
    store: &Store,
    session: &str,
    window: Vec<(u64, Stub)>,
) -> anyhow::Result<Vec<(u64, Stub)>> {
    let nodes = store.transcript(session)?;
    let moves = moved::read(&nodes);
    if moves.live.is_empty() {
        return Ok(window);
    }
    let mut window = window;
    for (p, n) in nodes.iter().filter(|(_, n)| moves.live.contains(&n.id)) {
        let first = match &n.body {
            Body::Moved {
                quiet: false,
                first,
                ..
            } => *first,
            _ => continue,
        };
        let at = (window.iter())
            .position(|(q, s)| s.session_id == session && *q > first)
            .unwrap_or(window.len());
        window.insert(at, (*p, n.clone()));
    }
    Ok(window)
}
