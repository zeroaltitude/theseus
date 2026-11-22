//! Threads in recall, as a label (theseus-f2g0; p17): which thread a recalled
//! note's session belongs to, so its heading names the thread it came from.
//! Threads no longer keep anything out (anything goes, 2026-11-22): no seal
//! and no distance.
//!
//! - **A session's thread** is read from memory first (`Board::of_session`:
//!   the session a thread's place runs on now), then from its record: the
//!   place a supersession link names (`thread:<slug>`), then its task parent.
//!   A session's thread never changes once known, so the answer, `None`
//!   included, is kept (`Memo`); an unreadable record is not kept.
//! - **Nothing is read** while no thread exists: [`TurnRunner::thread_scene`]
//!   gives none, and a note is labelled as before threads.

use std::collections::{BTreeSet, HashMap};
use std::sync::{Arc, Mutex, PoisonError};

use theseus_protocol::threads::{slug_of_place, ThreadInfo};

use crate::generic_folder::is_generic_folder;
use crate::session::SessionRecord;
use crate::store::Store;
use crate::threads::Board;
use crate::turn::TurnRunner;

/// The most sessions the memo keeps before it starts again.
const MEMO_MAX: usize = 20_000;
/// The longest a task chain is followed to its thread.
const DEPTH: usize = 4;
/// The shortest name or entity that counts as a mention.
const MIN_NAME: usize = 3;

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

/// Whether `text` (lower case) holds `name` (lower case) as whole words.
fn mentions(text: &str, name: &str) -> bool {
    if name.chars().count() < MIN_NAME {
        return false;
    }
    let word = |c: char| c.is_alphanumeric() || c == '_' || c == '-';
    let mut from = 0;
    while let Some(at) = text[from..].find(name) {
        let (a, b) = (from + at, from + at + name.len());
        let before = text[..a].chars().next_back().is_none_or(|c| !word(c));
        let after = text[b..].chars().next().is_none_or(|c| !word(c));
        if before && after {
            return true;
        }
        from = a + name.chars().next().map_or(1, char::len_utf8);
    }
    false
}

/// The threads `message` names: by slug, title, or a working-set entity. A
/// generic root (`notes`) names none (theseus-2uhsy). Recall reads it no more
/// (p17); `generic_folder`'s tests still do, until the sweep.
pub fn named(board: &Board, message: &str) -> BTreeSet<String> {
    let text = message.to_lowercase();
    let ws = |t: &ThreadInfo| -> Vec<String> {
        let w = &t.working_set;
        let roots = w.roots.iter().filter(|r| !is_generic_folder(r));
        [&w.repos, &w.branches, &w.refs, &w.files, &w.hosts]
            .into_iter()
            .flatten()
            .chain(roots)
            .cloned()
            .collect()
    };
    board
        .all()
        .filter(|t| {
            [t.slug.clone(), t.title.clone()]
                .into_iter()
                .chain(ws(t))
                .any(|n| mentions(&text, &n.to_lowercase()))
        })
        .map(|t| t.slug.clone())
        .collect()
}

/// What a recall knows of the threads, taken when its scene is made: the
/// board as it was, and the asking turn's own thread.
#[derive(Clone)]
pub struct ThreadScene {
    here: Option<String>,
    board: Arc<Board>,
    store: Store,
    memo: Arc<Memo>,
}

impl ThreadScene {
    /// The slug of the thread `session` belongs to: a category a note may
    /// share with the asker (`category_recall`).
    pub fn slug_of(&self, session: &str) -> Option<String> {
        self.memo.slug_of(&self.store, &self.board, session)
    }

    /// The title of the thread `session` belongs to, when it is another
    /// thread than the asking turn's: what a recalled note's provenance
    /// header names.
    pub fn other_title(&self, session: &str) -> Option<String> {
        let slug = self.memo.slug_of(&self.store, &self.board, session)?;
        (self.here.as_deref() != Some(slug.as_str()))
            .then(|| self.board.get(&slug).map(|t| t.title.clone()))
            .flatten()
    }

    /// The title of the thread `slug`, when it is another thread than the asking turn's: what the
    /// provenance of a node a split moved names (theseus-q9b85).
    pub fn other_title_of(&self, slug: &str) -> Option<String> {
        (self.here.as_deref() != Some(slug))
            .then(|| self.board.get(slug).map(|t| t.title.clone()))
            .flatten()
    }
}

impl TurnRunner {
    /// The threads' part of a recall for `session`, or none while threads are
    /// off or none exists (a note is then labelled as before threads).
    pub fn thread_scene(&self, session: Option<&str>) -> Option<ThreadScene> {
        if !self.cfg.threads.enabled {
            return None;
        }
        let board = self.threads.all(&self.store).ok()?;
        if board.is_empty() {
            return None;
        }
        let memo = self.memory.thread_memo();
        let here = session.and_then(|s| memo.slug_of(&self.store, &board, s));
        Some(ThreadScene {
            here,
            board,
            store: self.store.clone(),
            memo,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_name_counts_as_whole_words_in_any_case() {
        assert!(mentions("fix the security-fix port", "security-fix"));
        assert!(mentions("on pr-round we", "pr-round"));
        assert!(mentions("see #4210.", "#4210"));
        assert!(!mentions("the transport", "port"));
        assert!(!mentions("my-security-fix2", "security-fix"));
        assert!(!mentions("a b", "a"), "under three characters is no name");
        assert!(mentions("café menu", "café"));
    }
}
