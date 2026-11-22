//! The node seal for readers that show exchanges (issue 44): a message a split or a claim moved from a place
//! into a thread (`moved.of.<node>`) is that thread's, so an exchange holding one is read in its place only
//! when the thread is proven clear for the reader. The window, the history tool and the ladder's read-time
//! gate decide a seal by the exchange's session; [`Moved::holds`] is the node half of the same rule, the
//! gate's `node_seal` with the board read once: any message or reply of the exchange moved into a thread
//! that is sealed, seal-pending, unknown to the board, or whose record does not read, holds the exchange
//! whole. With the board unread, every moved node is held. Each node is looked up once (one keyed read,
//! then the memo): its cost is bounded by what is shown, never by how many messages were ever moved. A
//! `Moved` is read for one moment: it is built after the last await before the text it gates is read,
//! and never carried across one (its memo is a snapshot).
//!
//! [`moved_at_all`] is the writers' rule (the ladder's `did` lines and its first-level folds, written once
//! and kept): an exchange with any message or reply moved into any thread is folded by code and read by no
//! model, whatever the thread's seal today, since a summary outlives a seal set later.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use theseus_memory::gate::Seal;

use crate::exchange::Exchange;
use crate::moved::{moved_of_key, MovedTo};
use crate::store::Store;
use crate::threads::Board;
use crate::turn::TurnRunner;

/// The nodes of an exchange that carry its words: every message, the ask, the reply.
fn nodes(ex: &Exchange) -> impl Iterator<Item = &String> {
    ex.messages.iter().chain(&ex.ask).chain(&ex.reply)
}

/// Moved messages, as one reader may read them: the board read once, each node looked up once.
pub struct Moved<'a> {
    store: &'a Store,
    board: Option<Arc<Board>>,
    /// The thread the reader serves: its own thread is clear to it.
    own: Option<String>,
    memo: Mutex<HashMap<String, bool>>,
}

impl<'a> Moved<'a> {
    /// For a reader in `asker`, from the runner's board.
    pub fn read(runner: &'a TurnRunner, asker: Option<&str>) -> Self {
        Self::of(&runner.store, super::board(runner, "moved messages"), asker)
    }

    /// For a reader in `asker`, with `board` (none: unread, and every moved node is held).
    pub fn of(store: &'a Store, board: Option<Arc<Board>>, asker: Option<&str>) -> Self {
        let own = board
            .as_ref()
            .zip(asker)
            .and_then(|(b, a)| super::thread_of(store, b, a, 0).ok().flatten());
        Self {
            store,
            board,
            own,
            memo: Mutex::new(HashMap::new()),
        }
    }

    /// Whether node `id` is held from this reader: moved into a thread not proven clear for it.
    fn held(&self, id: &str) -> bool {
        if let Some(h) = self.memo.lock().ok().and_then(|m| m.get(id).copied()) {
            return h;
        }
        let seal = match (
            self.store.get_meta::<Option<MovedTo>>(&moved_of_key(id)),
            &self.board,
        ) {
            (Ok(None | Some(None)), _) => Seal::Clear,
            (Ok(Some(Some(m))), Some(b)) => match super::thread_seal(b, &m.thread) {
                Seal::Held if self.own.as_deref() == Some(m.thread.as_str()) => Seal::Clear,
                other => other,
            },
            (Ok(Some(Some(_))), None) => Seal::Unread,
            (Err(e), _) => {
                tracing::warn!(node = id, error = %format!("{e:#}"),
                    "where a node was moved cannot be read: its exchange is held out");
                Seal::Unread
            }
        };
        let h = seal != Seal::Clear;
        if let Ok(mut m) = self.memo.lock() {
            m.insert(id.to_string(), h);
        }
        h
    }

    /// Whether exchange `ex` is held whole from this reader for a moved message or reply.
    pub fn holds(&self, ex: &Exchange) -> bool {
        nodes(ex).any(|n| self.held(n))
    }

    /// The sessions of the threads `ex`'s messages and reply were moved into, as the board knows them: a
    /// reader that shows `ex` depends on them (a seal of one later revokes what it showed). None when they
    /// cannot all be proven: a move that does not read, an unread board, a thread the board does not hold, or
    /// one with no session.
    pub fn sessions(&self, ex: &Exchange) -> Option<Vec<String>> {
        let mut out = Vec::new();
        for n in nodes(ex) {
            match self.store.get_meta::<Option<MovedTo>>(&moved_of_key(n)) {
                Ok(None | Some(None)) => {}
                Ok(Some(Some(m))) => {
                    let t = self.board.as_ref()?.get(&m.thread)?;
                    out.push(t.session_id.clone()?);
                }
                Err(_) => return None,
            }
        }
        Some(out)
    }
}

/// Whether any exchange of `turns` holds a message or reply moved into a thread, is missing, or cannot be
/// read: the
/// writers' rule again at dispatch, after the last await before a summary's request.
pub fn sources_moved(store: &Store, turns: &[String]) -> bool {
    turns
        .iter()
        .any(|t| match crate::exchange::exchange(store, t) {
            // Only a readable exchange whose nodes are all clear proves a source: a missing one holds.
            Ok(Some(e)) => moved_at_all(store, &e),
            Ok(None) | Err(_) => true,
        })
}

/// Whether any message or reply of `ex` was moved into a thread, or where it went cannot be read: the
/// writers' rule (above).
pub fn moved_at_all(store: &Store, ex: &Exchange) -> bool {
    nodes(ex).any(|n| {
        !matches!(
            store.get_meta::<Option<MovedTo>>(&moved_of_key(n)),
            Ok(None | Some(None))
        )
    })
}
