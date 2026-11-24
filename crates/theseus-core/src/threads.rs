//! Threads (theseus-nhew; the owner, 2026-10-09): the streams of separable
//! thought, each addressable and none burdening another's context. A thread is
//! a durable record bound to a **virtual place** `thread:<slug>`, so it reuses
//! what a place has: a chain of sessions through `Core::bind_place_to`,
//! supersession when its session is replaced, the states, and the place rule.
//! Two threads are two sessions, so two executions, and run at once.
//!
//! Threads were retired on 2026-11-22 (issue 44): nothing writes a thread or binds
//! one any more. What is left reads the records an older store holds.
//!
//! - **Records** are META keys `thread:<slug>`, each a `ThreadInfo`; an edit
//!   wrote the key again (the WAL keeps every version), in one frame with its
//!   ledger row. The record keeps neither the session nor the last activity
//!   current: the place's own record (`discord.session.thread:<slug>`, written
//!   by the bind) says which session the thread runs on, and a session's
//!   record says when it last ran, so a turn writes nothing for its thread.
//! - **The board** (`Board`, held by `Threads`) is read from the store once
//!   and kept in memory, as standing's is: nothing on a turn's path reads the
//!   store. `Board::of_session` and `Board::live` are what the context,
//!   recall and routing rows call.
//! - **Private always.** `places::PlaceRule::class` classes `thread:*` private,
//!   `Board::live` offers a shared place none, and every method that reads a
//!   thread is the owner's, from a private place (`owner_in_private`).
//! - **The outbox ignores the place** (`outbox::virtual_place`): a thread's
//!   session has no Discord target to post to, as a CLI session has none.

use std::collections::{BTreeMap, HashMap};
use std::sync::{Arc, Mutex, OnceLock, RwLock, Weak};

use anyhow::Result;
use theseus_protocol::threads::{place_of, ThreadInfo, ThreadState, PLACE_PREFIX};
use theseus_store::{kinds, Store as _};

use crate::outbox::PLACE_META_PREFIX;
use crate::places::PlaceClass;
use crate::store::Store;

/// The META key prefix of a thread's record.
pub const PREFIX: &str = PLACE_PREFIX;

pub fn key(slug: &str) -> String {
    format!("{PREFIX}{slug}")
}

/// The META key prefix of the places the threads run on (`bind_place_to`'s
/// record of `thread:<slug>`).
fn place_prefix() -> String {
    format!("{PLACE_META_PREFIX}{PLACE_PREFIX}")
}

/// The threads in memory: an immutable snapshot of the stored records, read once.
#[derive(Debug, Default, Clone)]
pub struct Board {
    threads: BTreeMap<String, ThreadInfo>,
    by_session: HashMap<String, String>,
}

impl Board {
    pub(crate) fn build(records: Vec<ThreadInfo>, sessions: &HashMap<String, String>) -> Self {
        let mut b = Board::default();
        for mut t in records {
            t.session_id = sessions.get(&t.slug).cloned();
            if let Some(s) = &t.session_id {
                b.by_session.insert(s.clone(), t.slug.clone());
            }
            b.threads.insert(t.slug.clone(), t);
        }
        b
    }

    pub fn get(&self, slug: &str) -> Option<&ThreadInfo> {
        self.threads.get(slug)
    }

    /// Every thread, by slug.
    pub fn all(&self) -> impl Iterator<Item = &ThreadInfo> {
        self.threads.values()
    }

    pub fn len(&self) -> usize {
        self.threads.len()
    }

    pub fn is_empty(&self) -> bool {
        self.threads.is_empty()
    }

    /// The thread a session serves: the session its place runs on now. A
    /// session a thread's place has moved on from serves none.
    pub fn of_session(&self, session_id: &str) -> Option<&ThreadInfo> {
        self.by_session
            .get(session_id)
            .and_then(|slug| self.threads.get(slug))
    }

    /// The threads a message in a place of `class` may reach at all: every
    /// one in a private place, none in a shared one (a thread is private
    /// always). The one reading of the place for [`Board::live`].
    fn in_place(&self, class: PlaceClass) -> impl Iterator<Item = &ThreadInfo> {
        self.threads
            .values()
            .filter(move |_| class == PlaceClass::Private)
    }

    /// The threads a turn in a place of `class` may be routed to: the active
    /// ones of [`Board::in_place`], newest activity first.
    pub fn live(&self, class: PlaceClass) -> Vec<&ThreadInfo> {
        let mut live: Vec<&ThreadInfo> = self
            .in_place(class)
            .filter(|t| t.state == ThreadState::Active)
            .collect();
        live.sort_by(|a, b| (b.last_active_ms, &a.slug).cmp(&(a.last_active_ms, &b.slug)));
        live
    }
}

/// The board, and the lock its one load takes.
#[derive(Default)]
pub struct Threads {
    board: RwLock<Option<Arc<Board>>>,
    writes: Mutex<()>,
    /// The core, given once it is built: the turn's steps reach it through
    /// this.
    core: OnceLock<Weak<crate::rpc::Core>>,
}

impl Threads {
    pub fn attach(&self, core: &Arc<crate::rpc::Core>) {
        let _ = self.core.set(Arc::downgrade(core));
    }

    pub fn core(&self) -> Option<Arc<crate::rpc::Core>> {
        self.core.get().and_then(Weak::upgrade)
    }

    /// The board: memory once built; built now, by two META prefix scans,
    /// when nothing has built it yet. A record that does not read is left
    /// out and warned of.
    pub fn all(&self, store: &Store) -> Result<Arc<Board>> {
        if let Some(b) = self.held() {
            return Ok(b);
        }
        theseus_store::blocking(|| {
            let _w = self.writes.lock().unwrap();
            self.load(store)
        })
    }

    pub fn held(&self) -> Option<Arc<Board>> {
        self.board.read().unwrap().clone()
    }

    /// Drop the board, so the next reader reads the store again: a test's old records
    /// (`tests_old_threads`), written after the board was read.
    #[cfg(test)]
    pub(crate) fn forget(&self) {
        *self.board.write().unwrap() = None;
    }

    /// Build the board under the load's lock (the caller holds it).
    fn load(&self, store: &Store) -> Result<Arc<Board>> {
        if let Some(b) = self.held() {
            return Ok(b);
        }
        let mut records = Vec::new();
        for r in store.inner().latest_with_prefix(kinds::META, PREFIX)? {
            match r.decode::<ThreadInfo>() {
                Ok(t) => records.push(t),
                Err(e) => tracing::warn!(key = ?r.key, error = %e,
                    "a thread record does not read: left out"),
            }
        }
        let prefix = place_prefix();
        let mut sessions = HashMap::new();
        for r in store.inner().latest_with_prefix(kinds::META, &prefix)? {
            let Some(slug) = r.key.as_deref().and_then(|k| k.strip_prefix(&prefix)) else {
                continue;
            };
            if let Ok(sid) = r.decode::<String>() {
                sessions.insert(slug.to_string(), sid);
            }
        }
        let board = Arc::new(Board::build(records, &sessions));
        *self.board.write().unwrap() = Some(board.clone());
        Ok(board)
    }
}

/// The virtual place of `slug` (`thread:<slug>`), as `bind_place_to` names it.
pub fn place(slug: &str) -> String {
    place_of(slug)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn info(slug: &str, state: ThreadState, active: u64) -> ThreadInfo {
        ThreadInfo {
            slug: slug.into(),
            title: slug.into(),
            goal: String::new(),
            topics: vec![],
            working_set: Default::default(),
            card: vec![],
            persona: None,
            state,
            sealed: None,
            seal_pending: None,
            surfaces: vec![],
            parent: None,
            merged_into: None,
            split_from: None,
            links: vec![],
            session_id: None,
            last_active_ms: active,
            created_at_ms: 1,
        }
    }

    /// A thread is found by the session its place runs on; a shared place is
    /// offered none; a private one the active ones, newest first.
    #[test]
    fn the_board_finds_a_sessions_thread_and_offers_none_to_a_shared_place() {
        let sessions: HashMap<String, String> = [("tides".to_string(), "ses_a".to_string())].into();
        let b = Board::build(
            vec![
                info("tides", ThreadState::Active, 10),
                info("moons", ThreadState::Active, 20),
                info("paused", ThreadState::Paused, 30),
                info("done", ThreadState::Done, 40),
            ],
            &sessions,
        );
        assert_eq!(
            b.of_session("ses_a").map(|t| t.slug.as_str()),
            Some("tides")
        );
        assert!(b.of_session("ses_b").is_none());
        let live: Vec<&str> = b
            .live(PlaceClass::Private)
            .iter()
            .map(|t| t.slug.as_str())
            .collect();
        assert_eq!(live, ["moons", "tides"]);
        assert!(b.live(PlaceClass::Shared).is_empty());
    }
}
