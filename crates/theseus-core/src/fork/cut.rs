//! A fork's point as a cut (issue 44; the conductor's rule of 2026-11-22: a fork never holds what an
//! ancestor said after the point it was forked from it). Each ancestor session is cut at a WAL position:
//! the earliest point on the way down from it to the fork, as `fork::prefix_records` cuts the inherited run.
//! Every reader that lays an ancestor's words asks the cut, raw nodes and what is derived from them alike:
//!
//! - **A node** past its session's cut is not read (`view::sources_store::Sessions::exchange`, recall).
//! - **An exchange** is [`Reach::Beyond`] when its first node is past the cut: it is no candidate of any
//!   selector (its facts are none). It is [`Reach::Through`] when it began before the cut and did not end
//!   inside it (a fork at the person's message): its nodes are laid through the cut, and its `did` line, which
//!   summarises the whole exchange, is never laid. Otherwise it is [`Reach::Inside`].
//! - **A fold** is laid only when every exchange it stands for (the stored node's own list) is PROVEN inside
//!   the cut, by position: never by its exchanges' start times, which are the wall clock and may step back.
//!   An ancestor's exchange is proven inside only when every node it names (its messages, its ask, its reply)
//!   is found and all of them are at or before the cut ([`Cut::proven_inside`]): a missing node, or none read,
//!   proves nothing, and the fold is not laid. A fold that cannot be read, or one of whose exchanges cannot, is
//!   not laid; no summary is regenerated. A fold of other sessions alone is laid as for any session.
//! - **A `did` line**, which summarises a whole exchange as a fold does, is laid on the same proof.
//!
//! What it reads, and what bounds it: [`Cut::of`] reads the chain of fork origins (one session record per
//! ancestor, at most [`DEPTH`]) and each fork point's node (one keyed read per ancestor); no transcript.
//! [`Cut::reach`] reads the positions of one exchange's message and reply nodes (keyed reads, as many as the
//! exchange has messages); [`Cut::fold_inside`] reads a fold's record and asks the reach of each exchange
//! it stands for.
//! A read that fails is an error: the turn fails before anything is sent, never with the cut switched off.

use std::collections::HashMap;

use crate::exchange::Exchange;
use crate::store::Store;

/// The deepest chain of forks followed (`fork::MAX_DEPTH`'s bound).
pub const DEPTH: usize = 64;

/// Where an exchange stands against a fork's cut.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Reach {
    /// Wholly at or before its session's cut, or of a session the cut does not name.
    Inside,
    /// Begun before the cut and not ended inside it: laid through the cut, never summarised.
    Through,
    /// Begun after the cut: not laid at all.
    Beyond,
}

/// A fork's cut: each ancestor's last inherited position.
#[derive(Debug, Clone, Default)]
pub struct Cut {
    at: HashMap<String, u64>,
}

impl Cut {
    /// The cut of `session`: empty for a session that is no fork.
    pub fn of(store: &Store, session: &str) -> anyhow::Result<Self> {
        let mut out = Self::default();
        let (mut at, mut cut) = (session.to_string(), u64::MAX);
        for _ in 0..DEPTH {
            let Some(origin) = crate::fork::origin_of(store, &at)? else {
                break;
            };
            if store.get_node(&origin.node)?.is_none() {
                anyhow::bail!("a fork's point {} cannot be read", origin.node);
            }
            cut = cut.min(origin.position);
            out.at.insert(origin.session.clone(), cut);
            at = origin.session;
        }
        Ok(out)
    }

    /// Whether the cut names no ancestor: the session is no fork.
    pub fn is_empty(&self) -> bool {
        self.at.is_empty()
    }

    /// Whether the node `session` wrote at `position` is past its cut.
    pub fn past(&self, session: &str, position: u64) -> bool {
        self.at.get(session).is_some_and(|c| position > *c)
    }

    /// Where `ex` stands, by the positions of its own message and reply nodes. A node no longer in the store
    /// (erased) is passed over; an exchange with none read is inside, having nothing to lay.
    pub fn reach(&self, store: &Store, ex: &Exchange) -> anyhow::Result<Reach> {
        let Some(cut) = self.at.get(&ex.session_id) else {
            return Ok(Reach::Inside);
        };
        let (positions, _) = positions(store, ex)?;
        let (Some(first), Some(last)) = (positions.iter().min(), positions.iter().max()) else {
            return Ok(Reach::Inside);
        };
        Ok(match (*first > *cut, *last > *cut || ex.reply.is_none()) {
            (true, _) => Reach::Beyond,
            (false, true) => Reach::Through,
            (false, false) => Reach::Inside,
        })
    }

    /// Whether a summary of `ex` (its fold, its `did` line) may be laid: an exchange of a session the cut does
    /// not name, always; an ancestor's only when every node it names is found and the exchange is
    /// [`Reach::Inside`]. A missing node, or none at all, proves nothing; a read that fails is an error.
    pub fn proven_inside(&self, store: &Store, ex: &Exchange) -> anyhow::Result<bool> {
        if !self.at.contains_key(&ex.session_id) {
            return Ok(true);
        }
        let (positions, missing) = positions(store, ex)?;
        Ok(!missing && !positions.is_empty() && self.reach(store, ex)? == Reach::Inside)
    }

    /// Whether the stored fold `handle` may be laid: every exchange it stands for is proven inside the cut
    /// ([`Cut::proven_inside`]). A fold that cannot be found, or one of whose exchanges cannot, is not laid.
    pub fn fold_inside(&self, store: &Store, handle: &str) -> anyhow::Result<bool> {
        if self.is_empty() {
            return Ok(true);
        }
        let Some(node) = crate::ladder::fold::node(store, handle)? else {
            return Ok(false);
        };
        for turn in &node.exchanges {
            let Some(ex) = crate::exchange::exchange(store, turn)? else {
                return Ok(false);
            };
            if !self.proven_inside(store, &ex)? {
                return Ok(false);
            }
        }
        Ok(true)
    }
}

/// The positions of the nodes `ex` names (its messages, its ask, its reply), and whether any of them is not in
/// the store. A read that fails is an error.
fn positions(store: &Store, ex: &Exchange) -> anyhow::Result<(Vec<u64>, bool)> {
    let ids = (ex.messages.iter().chain(&ex.ask).chain(&ex.reply)).map(String::as_str);
    let (mut out, mut missing) = (Vec::new(), false);
    for id in ids {
        match store.get_node(id)? {
            Some((p, _)) => out.push(p),
            None => missing = true,
        }
    }
    Ok((out, missing))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A node position past its own session's cut, and only there.
    #[test]
    fn a_position_is_past_only_its_own_sessions_cut() {
        let mut cut = Cut::default();
        cut.at.insert("ses_a".into(), 10);
        assert!(cut.past("ses_a", 11) && !cut.past("ses_a", 10) && !cut.past("ses_b", 99));
        assert!(Cut::default().is_empty() && !cut.is_empty());
    }
}
