//! Moved messages (issue 44). Anything goes (2026-11-22): a message a split or a claim moved into a thread
//! (`moved.of.<node>`) is read wherever it is; nothing here holds it. What is left are pass-throughs for the
//! readers in other files until they go (the sweep).

use std::marker::PhantomData;
use std::sync::Arc;

use crate::exchange::Exchange;
use crate::store::Store;
use crate::threads::Board;
use crate::turn::TurnRunner;

/// Moved messages, as one reader may read them: nothing is held.
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

/// Whether any exchange of `turns` is missing or cannot be read, before a summary's request: a moved
/// message holds nothing.
pub fn sources_moved(store: &Store, turns: &[String]) -> bool {
    (turns.iter()).any(|t| !matches!(crate::exchange::exchange(store, t), Ok(Some(_))))
}

/// Whether a message or reply of `ex` was moved into a thread, for the writers' rule: never held now.
pub fn moved_at_all(_store: &Store, _ex: &Exchange) -> bool {
    false
}
