//! Moved messages (issue 44). Anything goes (2026-11-22): a message a split or a claim moved into a thread
//! (`moved.of.<node>`) is read wherever it is, and [`Moved::holds`] holds nothing.
//!
//! [`moved_at_all`] is the writers' rule (the ladder's `did` lines and its first-level folds, written once
//! and kept): an exchange with any message or reply moved into any thread is folded by code and read by no
//! model, whatever the thread's seal today, since a summary outlives a seal set later.

use std::marker::PhantomData;
use std::sync::Arc;

use crate::exchange::Exchange;
use crate::moved::{moved_of_key, MovedTo};
use crate::store::Store;
use crate::threads::Board;
use crate::turn::TurnRunner;

/// The nodes of an exchange that carry its words: every message, the ask, the reply.
fn nodes(ex: &Exchange) -> impl Iterator<Item = &String> {
    ex.messages.iter().chain(&ex.ask).chain(&ex.reply)
}

/// Moved messages, as one reader may read them. Anything goes (2026-11-22): a move holds nothing.
pub struct Moved<'a>(PhantomData<&'a ()>);

impl<'a> Moved<'a> {
    /// For a reader in `asker`.
    pub fn read(_runner: &'a TurnRunner, _asker: Option<&str>) -> Self {
        Moved(PhantomData)
    }

    /// For a reader in `asker`, with `board`.
    pub fn of(_store: &'a Store, _board: Option<Arc<Board>>, _asker: Option<&str>) -> Self {
        Moved(PhantomData)
    }

    /// Whether exchange `ex` is held whole from this reader for a moved message or reply: never.
    pub fn holds(&self, _ex: &Exchange) -> bool {
        false
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
