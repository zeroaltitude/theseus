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
//! - **A fold** whose last exchange began at or after the start of an exchange the cut runs through is not
//!   laid: it may summarise what came after. That is conservative (a fold of the place's other sessions at the
//!   same time goes too); the exchanges before the point come in as the inherited run.
//!
//! What it reads, and what bounds it: [`Cut::of`] reads the chain of fork origins (one session record per
//! ancestor, at most [`DEPTH`]) and each fork point's node and exchange (two keyed reads per ancestor); no
//! transcript. [`Cut::reach`] reads the positions of one exchange's message and reply nodes (keyed reads,
//! as many as the exchange has messages). None of it grows with what an ancestor said after its point.
//! A read that fails is an error: the turn fails before anything is sent, never with the cut switched off.

use std::collections::HashMap;

use crate::exchange::Exchange;
use crate::store::Store;
use crate::view::fold::FoldNode;

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

/// A fork's cut: each ancestor's last inherited position, and the earliest exchange start a fold may not reach.
#[derive(Debug, Clone, Default)]
pub struct Cut {
    at: HashMap<String, u64>,
    since_ms: Option<u64>,
}

impl Cut {
    /// The cut of `session`: empty for a session that is no fork.
    pub fn of(store: &Store, session: &str) -> anyhow::Result<Self> {
        let mut out = Self::default();
        let (mut at, mut cut) = (session.to_string(), u64::MAX);
        let mut point: Option<String> = None;
        for _ in 0..DEPTH {
            let Some(origin) = crate::fork::origin_of(store, &at)? else {
                break;
            };
            if origin.position < cut {
                cut = origin.position;
                point = Some(origin.node.clone());
            }
            out.at.insert(origin.session.clone(), cut);
            if let Some(node) = point.take() {
                let ms = point_ms(store, &node)?;
                out.since_ms = Some(out.since_ms.map_or(ms, |s| s.min(ms)));
            }
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
        let ids = (ex.messages.iter().chain(&ex.ask).chain(&ex.reply)).map(String::as_str);
        let mut positions = Vec::new();
        for id in ids {
            if let Some((p, _)) = store.get_node(id)? {
                positions.push(p);
            }
        }
        let (Some(first), Some(last)) = (positions.iter().min(), positions.iter().max()) else {
            return Ok(Reach::Inside);
        };
        Ok(match (*first > *cut, *last > *cut || ex.reply.is_none()) {
            (true, _) => Reach::Beyond,
            (false, true) => Reach::Through,
            (false, false) => Reach::Inside,
        })
    }

    /// Whether `fold` may summarise something past the cut: its last exchange began at or after the start of
    /// the exchange a fork point falls in.
    pub fn folds_past(&self, fold: &FoldNode) -> bool {
        self.since_ms.is_some_and(|s| fold.last.at_ms >= s)
    }
}

/// When the exchange holding fork point `node` began (the node's own time when it is of no exchange).
fn point_ms(store: &Store, node: &str) -> anyhow::Result<u64> {
    let Some((_, n)) = store.get_node(node)? else {
        anyhow::bail!("a fork's point {node} cannot be read");
    };
    let start = match n.turn_id.as_deref() {
        Some(turn) => crate::exchange::exchange(store, turn)?.map(|e| e.start_ms),
        None => None,
    };
    Ok(start.unwrap_or(n.created_at_ms))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::view::fold::Writer;
    use crate::view::{Audience, Author, Handle, Marks, Pos};

    fn fold(last_ms: u64) -> FoldNode {
        FoldNode {
            level: 1,
            place: "local".into(),
            first: Pos {
                at_ms: 1,
                id: "t1".into(),
            },
            last: Pos {
                at_ms: last_ms,
                id: "t9".into(),
            },
            exchanges: 9,
            his_words: vec![],
            his_counted: 0,
            account: "answered the later question".into(),
            marks: Marks {
                place: Some("local".into()),
                audience: Audience::Private,
                author: Author::Theseus,
                outside: false,
                sealed: false,
                veiled: false,
            },
            handle: Handle("fold.p1".into()),
            title: String::new(),
            writer: Writer {
                by: "m".into(),
                version: "1".into(),
            },
        }
    }

    /// A fold is not laid for a fork when it may cover the exchange its point falls in or anything after it:
    /// one whose last exchange began at or after that exchange's start. One wholly before it is laid.
    #[test]
    fn a_fold_over_the_point_or_past_it_is_not_laid_and_one_before_it_is() {
        let mut cut = Cut::default();
        cut.at.insert("ses_a".into(), 10);
        cut.since_ms = Some(1_000);
        assert!(!cut.folds_past(&fold(999)));
        assert!(
            cut.folds_past(&fold(1_000)),
            "the exchange the point falls in"
        );
        assert!(cut.folds_past(&fold(5_000)), "after it");
        assert!(
            !Cut::default().folds_past(&fold(5_000)),
            "no fork: every fold"
        );
        assert!(cut.past("ses_a", 11) && !cut.past("ses_a", 10) && !cut.past("ses_b", 99));
    }
}
