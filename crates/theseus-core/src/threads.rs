//! Threads (theseus-nhew; the owner, 2026-10-09): the streams of separable
//! thought, each addressable and none burdening another's context. A thread is
//! a durable record bound to a **virtual place** `thread:<slug>`, so it reuses
//! what a place has: a chain of sessions through `Core::bind_place_to`,
//! supersession when its session is replaced, the states, and the place rule.
//! Two threads are two sessions, so two executions, and run at once.
//!
//! - **Records** are META keys `thread:<slug>`, each a `ThreadInfo`; an edit
//!   writes the key again (the WAL keeps every version), in one frame with its
//!   ledger row. The record keeps neither the session nor the last activity
//!   current: the place's own record (`discord.session.thread:<slug>`, written
//!   by the bind) says which session the thread runs on, and a session's
//!   record says when it last ran, so a turn writes nothing for its thread.
//! - **The board** (`Board`, held by `Threads`) is read from the store once
//!   and kept in memory, as standing's is: nothing on a turn's path reads the
//!   store. `Board::of_session` and `Board::live` are what the context,
//!   recall and routing rows call.
//! - **Private always.** `places::PlaceRule::class` classes `thread:*` private,
//!   `Board::live` offers a shared place none, and every method that reads or
//!   writes a thread is the owner's, from a private place (`owner_in_private`).
//! - **The outbox ignores the place** (`outbox::virtual_place`): a thread's
//!   session has no Discord target to post to, as a CLI session has none.

use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::sync::{Arc, Mutex, OnceLock, RwLock, Weak};

use anyhow::Result;
use theseus_protocol::threads::{place_of, ThreadInfo, ThreadState, PLACE_PREFIX};
use theseus_store::{kinds, NewRecord, Store as _};

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

/// The threads in memory: an immutable snapshot, swapped whole on a write.
#[derive(Debug, Default, Clone)]
pub struct Board {
    threads: BTreeMap<String, ThreadInfo>,
    by_session: HashMap<String, String>,
}

impl Board {
    fn build(records: Vec<ThreadInfo>, sessions: &HashMap<String, String>) -> Self {
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

    /// The threads a turn in a place of `class` may be routed to: the active
    /// ones in a private place, none in a shared one (a thread is private
    /// always), newest activity first.
    pub fn live(&self, class: PlaceClass) -> Vec<&ThreadInfo> {
        if class != PlaceClass::Private {
            return Vec::new();
        }
        let mut live: Vec<&ThreadInfo> = self
            .threads
            .values()
            .filter(|t| t.state == ThreadState::Active)
            .collect();
        live.sort_by(|a, b| (b.last_active_ms, &a.slug).cmp(&(a.last_active_ms, &b.slug)));
        live
    }

    /// The thread a thread now lives in: itself, or, when it was merged away,
    /// the thread it was merged into, followed to the end (theseus-vt9k). What
    /// the distance rule treats as the live thread of a retired one's nodes.
    pub fn live_of(&self, slug: &str) -> Option<&ThreadInfo> {
        let mut t = self.threads.get(slug)?;
        // A chain is as long as the merges, and a cycle cannot form (a merged
        // thread takes none), but a walk that cannot end is never the answer.
        for _ in 0..self.threads.len() {
            match t.merged_into.as_deref().and_then(|s| self.threads.get(s)) {
                Some(next) => t = next,
                None => break,
            }
        }
        Some(t)
    }

    /// The threads merged into `slug`, and those split from it, by slug.
    pub fn links_of(&self, slug: &str) -> (Vec<String>, Vec<String>) {
        let by = |f: fn(&ThreadInfo) -> Option<&str>| -> Vec<String> {
            self.threads
                .values()
                .filter(|t| f(t) == Some(slug))
                .map(|t| t.slug.clone())
                .collect()
        };
        (
            by(|t| t.merged_into.as_deref()),
            by(|t| t.split_from.as_deref()),
        )
    }

    /// The threads the owner linked with `here` (theseus-98bz.11), by slug: the ends of its own
    /// `links` and every thread whose `links` hold it, each with the live thread a merge left it
    /// in. Both directions are read, so a half-written pair can never make the lens one-sided.
    pub fn linked_with(&self, here: &str) -> BTreeSet<String> {
        let mut out = BTreeSet::new();
        let mut add = |slug: &str| {
            out.insert(slug.to_string());
            out.extend(self.live_of(slug).map(|t| t.slug.clone()));
        };
        let holds_here = |l: &String| l == here || self.live_of(l).is_some_and(|t| t.slug == here);
        if let Some(t) = self.get(here) {
            t.links.iter().for_each(|l| add(l));
        }
        for t in self.all().filter(|t| t.links.iter().any(holds_here)) {
            add(&t.slug);
        }
        out.remove(here);
        out
    }

    /// Threads not done: what `[threads] max_live` bounds.
    /// Every thread as the work board groups it.
    pub fn groups(&self) -> Vec<theseus_protocol::ThreadGroup> {
        self.threads.values().map(group_of).collect()
    }

    pub fn open(&self) -> usize {
        self.threads
            .values()
            .filter(|t| t.state != ThreadState::Done)
            .count()
    }
}

/// A thread as the work board groups it (theseus-5i8i): the record's words
/// and the session it runs on. The stats a session holds (context tokens,
/// today's dollars) are read where a person asks (`Core::thread_groups`), not
/// here: this is memory only.
pub fn group_of(t: &ThreadInfo) -> theseus_protocol::ThreadGroup {
    theseus_protocol::ThreadGroup {
        slug: t.slug.clone(),
        title: t.title.clone(),
        state: t.state,
        persona: t.persona.clone(),
        sealed: t.sealed.is_some(),
        seal_pending: t.seal_pending.clone(),
        session_id: t.session_id.clone(),
        last_active_ms: t.last_active_ms,
        context_tokens: None,
        today_usd: None,
    }
}

/// The board, and the lock its writers take.
#[derive(Default)]
pub struct Threads {
    board: RwLock<Option<Arc<Board>>>,
    writes: Mutex<()>,
    /// What the router remembers (theseus-rha2).
    pub route: crate::thread_flow::State,
    /// Told the board after each write or bind (the work board's feed,
    /// theseus-5i8i), set once.
    feed: std::sync::OnceLock<Feed>,
    /// The core, given once it is built: `thread.split`, a tool of the
    /// model's, opens a thread and its session through it.
    core: OnceLock<Weak<crate::rpc::Core>>,
}

/// What a board change is told to.
pub type Feed = Box<dyn Fn(&Board) + Send + Sync>;

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

    /// Tell `feed` the board after each write or bind. Set once; a second
    /// call is ignored.
    pub fn on_change(&self, feed: Feed) {
        let _ = self.feed.set(feed);
    }

    fn told(&self, board: &Board) {
        if let Some(f) = self.feed.get() {
            f(board);
        }
    }

    pub fn held(&self) -> Option<Arc<Board>> {
        self.board.read().unwrap().clone()
    }

    /// Build the board under the writers' lock (the caller holds it).
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

    /// Write `check`'s threads in one frame with `rows`, which sees the board
    /// as it was and the threads written; then hold the new board. `check`
    /// runs under the lock against that board, and a refusal writes nothing.
    pub fn write(
        &self,
        store: &Store,
        check: impl FnOnce(&Board) -> Result<Vec<ThreadInfo>>,
        rows: impl FnOnce(&Board, &[ThreadInfo]) -> Result<Vec<NewRecord>>,
    ) -> Result<Vec<ThreadInfo>> {
        self.write_bound(store, check, rows, None)
    }

    /// `write`, and the thread `bound.0` is held as running on the session
    /// `bound.1` from the same swap: a thread made with its session already
    /// open (a split's), whose place record the caller put in `rows`' frame.
    /// No reader sees the thread without its session.
    pub fn write_bound(
        &self,
        store: &Store,
        check: impl FnOnce(&Board) -> Result<Vec<ThreadInfo>>,
        rows: impl FnOnce(&Board, &[ThreadInfo]) -> Result<Vec<NewRecord>>,
        bound: Option<(&str, &str)>,
    ) -> Result<Vec<ThreadInfo>> {
        theseus_store::blocking(|| {
            let _w = self.writes.lock().unwrap();
            let now = self.load(store)?;
            let threads = check(&now)?;
            if threads.is_empty() {
                return Ok(threads);
            }
            let mut frame = Vec::with_capacity(threads.len() + 1);
            for t in &threads {
                // The session is the place's record, never the thread's.
                let stored = ThreadInfo {
                    session_id: None,
                    ..t.clone()
                };
                frame.push(NewRecord::json(kinds::META, Some(&key(&t.slug)), &stored)?);
            }
            frame.extend(rows(&now, &threads)?);
            store.append(&frame)?;
            let mut next = (*now).clone();
            for t in &threads {
                let mut t = t.clone();
                t.session_id = now.get(&t.slug).and_then(|o| o.session_id.clone());
                if let Some((slug, session)) = bound.filter(|(s, _)| *s == t.slug) {
                    t.session_id = Some(session.to_string());
                    next.by_session
                        .insert(session.to_string(), slug.to_string());
                }
                next.threads.insert(t.slug.clone(), t);
            }
            self.told(&next);
            *self.board.write().unwrap() = Some(Arc::new(next));
            Ok(threads)
        })
    }

    /// The thread a session serves, taken to the record `f` makes of it and
    /// the records its frame carries (`thread.note`, whose call writes the
    /// frame: the result node rides in it). The board holds the new record
    /// at once, under the writers' lock, so a reader never sees the card
    /// older than the frame that is being written.
    pub fn noted(
        &self,
        store: &Store,
        session: &str,
        f: impl FnOnce(&ThreadInfo) -> Result<(ThreadInfo, Vec<NewRecord>)>,
    ) -> Result<(ThreadInfo, Vec<NewRecord>)> {
        theseus_store::blocking(|| {
            let _w = self.writes.lock().unwrap();
            let now = self.load(store)?;
            let Some(t) = now.of_session(session) else {
                anyhow::bail!(
                    "this session serves no thread: thread.note is for a thread's own session"
                );
            };
            let (next, records) = f(t)?;
            let mut board = (*now).clone();
            board.threads.insert(next.slug.clone(), next.clone());
            *self.board.write().unwrap() = Some(Arc::new(board));
            Ok((next, records))
        })
    }

    /// Run `f` under the writers' lock with the board: the one place a
    /// thread's session is opened and bound, so two first messages to a
    /// thread open one session between them. `f` returns the session it
    /// bound `slug` to, when it bound one.
    pub fn bind<T>(
        &self,
        store: &Store,
        slug: &str,
        f: impl FnOnce(&Board) -> Result<(T, Option<String>)>,
    ) -> Result<T> {
        theseus_store::blocking(|| {
            let _w = self.writes.lock().unwrap();
            let now = self.load(store)?;
            let (out, bound) = f(&now)?;
            if let Some(session) = bound {
                let mut next = (*now).clone();
                if let Some(old) = now.get(slug).and_then(|t| t.session_id.clone()) {
                    next.by_session.remove(&old);
                }
                if let Some(t) = next.threads.get_mut(slug) {
                    t.session_id = Some(session.clone());
                }
                next.by_session.insert(session, slug.to_string());
                self.told(&next);
                *self.board.write().unwrap() = Some(Arc::new(next));
            }
            Ok(out)
        })
    }
}

/// The seal a merge of two threads has (theseus-f2g0): sealed if either was
/// (the first's when both are). The merge itself is the router's row's
/// (T4); the rule is here, for it to call.
pub fn merged_seal(a: &ThreadInfo, b: &ThreadInfo) -> Option<theseus_protocol::threads::Seal> {
    a.sealed.clone().or_else(|| b.sealed.clone())
}

/// The seal a split from `parent` starts with.
pub fn inherited_seal(
    parent: &ThreadInfo,
    who: &str,
    now_ms: u64,
) -> Option<theseus_protocol::threads::Seal> {
    parent
        .sealed
        .as_ref()
        .map(|_| theseus_protocol::threads::Seal {
            by: who.to_string(),
            at_ms: now_ms,
            reason: format!("inherited from the sealed thread {}", parent.slug),
        })
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
            persona: None,
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
        assert_eq!(b.open(), 3);
    }
}
