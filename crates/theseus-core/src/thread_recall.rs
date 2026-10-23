//! Threads in recall (theseus-f2g0): which thread a candidate's session
//! belongs to, what a message names, and the [`ThreadLens`] the pure pipeline
//! (`theseus_memory::threads`) reads. One thread's facts reach another only
//! when relevant or asked, always named, and a sealed thread's never ambient.
//!
//! - **A session's thread** is read from memory first (`Board::of_session`:
//!   the session a thread's place runs on now), then from its record: the
//!   place a supersession link names (`thread:<slug>`), then its task parent.
//!   A session's thread never changes once known, so the answer, `None`
//!   included, is kept (`Memo`); an unreadable record is not kept.
//! - **Nothing is read** while no thread exists: [`TurnRunner::thread_scene`]
//!   gives none, and recall is as it was before threads.
//! - **Streams do not mix** (theseus-wncdv): a node's own thread (`moved_to`, what a
//!   split moved) outranks its session's; `[threads] base_distance` makes a node of
//!   the place's own conversation another stream's to a threaded asker;
//!   `cross_recall_beats_own` and `cross_recall_needs_entity` make another stream's
//!   hit outrank what the thread holds and share an entity with the message.
//!   What another live thread's hit must outrank is the thread's own best hit,
//!   in context or not (`own_top`, theseus-stwkb); a hit no thread claims (the
//!   place's own conversation) must outrank only its best not already in
//!   context (`own_top_admissible`, theseus-98bz.8), so a home note holding the
//!   one fact the thread lacks still gets in.
//! - **Naming** (`waived`): the message names a thread by its slug, its
//!   title, or one of its working set's entities (a repo, branch, ref, file,
//!   root or host), as whole words, in any case. The threads the owner linked
//!   with the asking one are waived too (`Board::linked_with`, theseus-98bz.11);
//!   a seal still holds them out, since the lens checks it first.

use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::sync::{Arc, Mutex, PoisonError};

use theseus_memory::{SessionThread, ThreadLens};
use theseus_protocol::threads::{slug_of_place, ThreadInfo, ThreadState};

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

/// The threads `message` names: by slug, title, or a working-set entity.
pub fn named(board: &Board, message: &str) -> BTreeSet<String> {
    let text = message.to_lowercase();
    let ws = |t: &ThreadInfo| -> Vec<String> {
        let w = &t.working_set;
        [&w.roots, &w.repos, &w.branches, &w.refs, &w.files, &w.hosts]
            .into_iter()
            .flatten()
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
/// board as it was, who asks, what the message names. [`Self::lens`] reads it
/// over the candidates once the index has answered.
#[derive(Clone)]
pub struct ThreadScene {
    here: Option<String>,
    distance: bool,
    waived: BTreeSet<String>,
    penalty: f64,
    bar: f64,
    base: bool,
    beats_own: bool,
    own_top_admissible: bool,
    needs_entity: bool,
    /// The nodes Jev admitted across threads this recall (theseus-98bz.10).
    admitted: BTreeSet<String>,
    board: Arc<Board>,
    store: Store,
    memo: Arc<Memo>,
}

impl ThreadScene {
    /// Jev read these nodes as about the message's subject: the lens admits
    /// them with the penalty (theseus-98bz.10).
    pub fn admit(&mut self, nodes: BTreeSet<String>) {
        self.admitted.extend(nodes);
    }

    /// The pairs a link could join (theseus-98bz.11): the asking thread with each of `others` that
    /// is live, unsealed, not merged and not linked with it already. None for an unthreaded asker.
    pub fn link_pairs(&self, others: &BTreeSet<String>) -> Vec<crate::thread_link_seen::Pair> {
        let held = |t: &ThreadInfo| {
            t.sealed.is_some() || t.seal_pending.is_some() || t.merged_into.is_some()
        };
        let Some(here) = self.here.as_deref().and_then(|h| self.board.get(h)) else {
            return Vec::new();
        };
        if held(here) {
            return Vec::new();
        }
        let linked = self.board.linked_with(&here.slug);
        others
            .iter()
            .filter(|s| **s != here.slug && !linked.contains(*s))
            .filter_map(|s| self.board.get(s))
            .filter(|t| !held(t) && t.state != ThreadState::Done)
            .map(|t| crate::thread_link_seen::Pair {
                here: (here.slug.clone(), here.title.clone()),
                other: (t.slug.clone(), t.title.clone()),
            })
            .collect()
    }

    /// The thread of `session`, with whether it is held.
    pub fn of(&self, session: &str) -> Option<SessionThread> {
        let slug = self.memo.slug_of(&self.store, &self.board, session)?;
        let t = self.board.get(&slug)?;
        Some(SessionThread {
            held: t.sealed.is_some() || t.seal_pending.is_some(),
            slug,
        })
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

    /// The thread a split moved `node` to, when it did (`moved.of.<node>`):
    /// the node's own thread, which outranks its session's.
    fn moved_to(&self, node: &str) -> Option<SessionThread> {
        let m = self
            .store
            .get_meta::<Option<crate::moved::MovedTo>>(&crate::moved::moved_of_key(node))
            .ok()
            .flatten()
            .flatten()?;
        let t = self.board.get(&m.thread)?;
        Some(SessionThread {
            held: t.sealed.is_some() || t.seal_pending.is_some(),
            slug: m.thread,
        })
    }

    /// The lens over candidates given as (session, node, fused score), the
    /// nodes already in the turn's context being `in_context`.
    pub fn lens<'a>(
        &self,
        candidates: impl IntoIterator<Item = (&'a str, &'a str, f64)>,
        in_context: &BTreeSet<String>,
    ) -> ThreadLens {
        let mut of_session = BTreeMap::new();
        let mut of_node = BTreeMap::new();
        let mut top: f64 = 0.0;
        let mut seen = Vec::new();
        for (session, node, fused) in candidates {
            if fused.is_finite() {
                top = top.max(fused);
            }
            if !of_session.contains_key(session) {
                if let Some(t) = self.of(session) {
                    of_session.insert(session.to_string(), t);
                }
            }
            if let Some(t) = self.moved_to(node) {
                of_node.insert(node.to_string(), t);
            }
            seen.push((session, node, fused));
        }
        // Jev's yes, never a held thread's node.
        let admitted = seen
            .iter()
            .filter(|(_, node, _)| self.admitted.contains(*node))
            .filter(|(session, node, _)| {
                !of_node
                    .get(*node)
                    .or_else(|| of_session.get(*session))
                    .is_some_and(|t: &SessionThread| t.held)
            })
            .map(|(_, node, _)| (*node).to_string())
            .collect();
        // What the asking thread already holds: its own best hit, in context
        // or not, which another live thread's must outrank (theseus-stwkb);
        // and, with `own_top_admissible` (theseus-98bz.8), its best that can
        // still be admitted, which a note no thread claims must outrank: a
        // note in context drops `in_context`, so it holds no home note back.
        let own: Vec<(bool, f64)> = seen
            .iter()
            .filter(|(session, node, fused)| {
                fused.is_finite()
                    && of_node
                        .get(*node)
                        .or_else(|| of_session.get(*session))
                        .is_some_and(|t| self.here.as_deref() == Some(t.slug.as_str()))
            })
            .map(|(_, node, fused)| (in_context.contains(*node), *fused))
            .collect();
        let best = |open_only: bool| {
            own.iter()
                .filter(|(held, _)| !(open_only && *held))
                .fold(0.0_f64, |a, (_, f)| a.max(*f))
        };
        let own_top = best(false);
        let own_top_admissible = self.own_top_admissible.then(|| best(true));
        ThreadLens {
            here: self.here.clone(),
            distance: self.distance,
            of_session,
            of_node,
            base: self.base,
            waived: self.waived.clone(),
            top_fused: top,
            own_top,
            own_top_admissible,
            beats_own: self.beats_own,
            needs_entity: self.needs_entity,
            penalty: self.penalty,
            bar: self.bar,
            admitted,
        }
    }
}

impl TurnRunner {
    /// The threads' part of a recall for `session`, or none while threads are
    /// off or none exists (recall is then as before). `ambient`: the turn's
    /// own recall, which pays the distance in a threaded session; a lookup
    /// the model asked for, or a client's, keeps only the seal.
    pub fn thread_scene(
        &self,
        session: Option<&str>,
        message: &str,
        ambient: bool,
    ) -> Option<ThreadScene> {
        let cfg = &self.cfg.threads;
        if !cfg.enabled {
            return None;
        }
        let board = self.threads.all(&self.store).ok()?;
        if board.is_empty() {
            return None;
        }
        let memo = self.memory.thread_memo();
        let here = session.and_then(|s| memo.slug_of(&self.store, &board, s));
        let distance = ambient && here.is_some();
        let waived = match here.as_deref().filter(|_| distance) {
            // A linked thread's notes pass unpaid; the lens still checks the seal first.
            Some(h) => named(&board, message)
                .union(&board.linked_with(h))
                .cloned()
                .collect(),
            None => BTreeSet::new(),
        };
        Some(ThreadScene {
            here,
            distance,
            waived,
            penalty: cfg.cross_recall_penalty,
            bar: cfg.cross_recall_bar,
            base: cfg.base_distance,
            beats_own: cfg.cross_recall_beats_own,
            own_top_admissible: cfg.own_top_admissible,
            needs_entity: cfg.cross_recall_needs_entity,
            admitted: BTreeSet::new(),
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
