//! A session's thread, read once (theseus-f2g0): which thread an old
//! session belongs to. Recall no longer reads it (issue 46): an item is
//! ranked and headed by where and by whom it was said, never by a thread.
//! What still asks is the old seal's crossing report (`rpc/thread_seal.rs`).
//!
//! - **A session's thread** is read from memory first (`Board::of_session`:
//!   the session a thread's place runs on now), then from its record: the
//!   place a supersession link names (`thread:<slug>`), then its task parent.
//!   A session's thread never changes once known, so the answer, `None`
//!   included, is kept (`Memo`); an unreadable record is not kept.

use std::collections::HashMap;
use std::sync::{Mutex, PoisonError};

use theseus_protocol::threads::slug_of_place;

use crate::session::SessionRecord;
use crate::store::Store;
use crate::threads::Board;

/// The most sessions the memo keeps before it starts again.
const MEMO_MAX: usize = 20_000;
/// The longest a task chain is followed to its thread.
const DEPTH: usize = 4;

/// Each session's thread, once read.
#[derive(Default)]
pub struct Memo(Mutex<HashMap<String, Option<String>>>);

impl Memo {
    /// The slug of the thread `session` belongs to.
    pub fn slug_of(&self, store: &Store, board: &Board, session: &str) -> Option<String> {
        if let Some(t) = board.of_session(session) {
            return Some(t.slug.clone());
        }
        if let Some(known) = self
            .0
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .get(session)
        {
            return known.clone();
        }
        let read = resolve(store, board, session, 0)?;
        let mut memo = self.0.lock().unwrap_or_else(PoisonError::into_inner);
        if memo.len() >= MEMO_MAX {
            memo.clear();
        }
        memo.insert(session.to_string(), read.clone());
        read
    }
}

/// `Some(thread)` when the record answers, `Some(None)` when the session has
/// none, `None` when it cannot be read (not kept).
fn resolve(store: &Store, board: &Board, session: &str, depth: usize) -> Option<Option<String>> {
    if crate::import::is_imported(session) {
        return Some(None);
    }
    if let Some(t) = board.of_session(session) {
        return Some(Some(t.slug.clone()));
    }
    let rec: SessionRecord = match store.get_session(session) {
        Ok(Some(r)) => r,
        Ok(None) => return Some(None),
        Err(_) => return None,
    };
    let linked = [&rec.superseded_by, &rec.supersedes]
        .into_iter()
        .flatten()
        .filter_map(|l| l.place.as_deref())
        .find_map(slug_of_place);
    if let Some(slug) = linked {
        return Some(Some(slug.to_string()));
    }
    match rec.task {
        Some(t) if depth < DEPTH => resolve(store, board, &t.parent_session, depth + 1),
        _ => Some(None),
    }
}
