//! The read record: what each session has been handed, so `fs.edit` and
//! `fs.patch` can tell a write of their own reading apart from a write over
//! a change that landed underneath it (theseus-w2cs sub-item 2).
//!
//! The rule is `docs/design/stale-edit.md`. The type a toollet sees is
//! `theseus_tools::reads` ([`Mark`], [`Reads`]); this is the board behind it.
//!
//! **Why it is shaped this way.** The first attempt kept one process-wide
//! `HashMap` with one insert and one get: no eviction, no cap, and no clear
//! at a session's end, so a long-lived daemon grew it forever. This one is
//! keyed by session and bounded twice -- [`MAX_PATHS`] notes a session, and
//! [`MAX_SESSIONS`] sessions with a board at all -- and a session's board is
//! dropped when its session ends ([`ReadRecords::forget`], called from the
//! turn runner). The two in-tree shapes it follows are `JobWaits`
//! (`toolrun/waits.rs`) and `Terms` (`term/mod.rs`): one board on the
//! runtime, reached by a call through an `Arc`.
//!
//! **At the cap the oldest note goes, and that path is then unguarded.** That
//! is the quiet choice, and it is the right default under the rule: an
//! evicted note puts the path back where every unread path already is. The
//! noisy choice -- refuse to record and say so -- is defensible too and is an
//! open question for the owner.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, PoisonError};

use theseus_tools::reads::{Mark, Reads};

/// Paths one session may have noted at once. Past it the oldest note goes.
pub const MAX_PATHS: usize = 256;

/// Sessions with a board at once. Past it the least recently used session's
/// whole board goes: a parked daemon with thousands of sessions holds this
/// many, not one per session that ever read a file.
pub const MAX_SESSIONS: usize = 512;

/// One path's note: when it was made (to evict the oldest), and the state the
/// file was in.
#[derive(Clone, Copy)]
struct Note {
    at: u64,
    mark: Mark,
}

#[derive(Default)]
struct Board {
    /// When this session's board was last written, for the session eviction.
    touched: u64,
    notes: HashMap<PathBuf, Note>,
}

impl Board {
    fn note(&mut self, at: u64, path: &Path, mark: Mark) {
        self.touched = at;
        self.notes.insert(path.to_path_buf(), Note { at, mark });
        while self.notes.len() > MAX_PATHS {
            let Some(oldest) = self
                .notes
                .iter()
                .min_by_key(|(_, n)| n.at)
                .map(|(p, _)| p.clone())
            else {
                break;
            };
            self.notes.remove(&oldest);
        }
    }
}

/// Every session's read record.
#[derive(Default)]
pub struct ReadRecords {
    inner: Mutex<Inner>,
}

#[derive(Default)]
struct Inner {
    /// A counter, not a clock: it orders the notes and nothing else, so no
    /// test of it depends on the wall clock or the hour.
    seq: u64,
    boards: HashMap<String, Board>,
}

impl ReadRecords {
    /// `session` was handed `path` in the state `mark` describes.
    pub fn note(&self, session: &str, path: &Path, mark: Mark) {
        let mut i = self.inner.lock().unwrap_or_else(PoisonError::into_inner);
        i.seq += 1;
        let at = i.seq;
        i.boards
            .entry(session.to_string())
            .or_default()
            .note(at, path, mark);
        Self::hold_sessions(&mut i);
    }

    /// The state `session` was last handed `path` in, if it was.
    pub fn mark_of(&self, session: &str, path: &Path) -> Option<Mark> {
        let i = self.inner.lock().unwrap_or_else(PoisonError::into_inner);
        Some(i.boards.get(session)?.notes.get(path)?.mark)
    }

    /// `session` wrote `path` itself, so a note of it is now of the file
    /// before that write: move it on. **Only where a note exists** -- a write
    /// never brings a path under the guard.
    pub fn wrote(&self, session: &str, path: &Path, mark: Mark) {
        let mut i = self.inner.lock().unwrap_or_else(PoisonError::into_inner);
        i.seq += 1;
        let at = i.seq;
        if let Some(b) = i.boards.get_mut(session) {
            if let Some(n) = b.notes.get_mut(path) {
                *n = Note { at, mark };
                b.touched = at;
            }
        }
    }

    /// `session` ended: its board goes with it.
    pub fn forget(&self, session: &str) {
        let mut i = self.inner.lock().unwrap_or_else(PoisonError::into_inner);
        i.boards.remove(session);
    }

    /// The notes `session` holds now. For tests and for health.
    pub fn len_of(&self, session: &str) -> usize {
        let i = self.inner.lock().unwrap_or_else(PoisonError::into_inner);
        i.boards.get(session).map_or(0, |b| b.notes.len())
    }

    /// The sessions with a board now. For tests and for health.
    pub fn sessions(&self) -> usize {
        let i = self.inner.lock().unwrap_or_else(PoisonError::into_inner);
        i.boards.len()
    }

    /// This call's view of the board: one session's, as a toollet sees it.
    pub fn bind(self: &Arc<Self>, session: &str) -> Arc<dyn Reads> {
        Arc::new(Bound {
            records: self.clone(),
            session: session.to_string(),
        })
    }

    fn hold_sessions(i: &mut Inner) {
        while i.boards.len() > MAX_SESSIONS {
            let Some(oldest) = i
                .boards
                .iter()
                .min_by_key(|(_, b)| b.touched)
                .map(|(s, _)| s.clone())
            else {
                break;
            };
            i.boards.remove(&oldest);
        }
    }
}

/// The board bound to one session, which is what rides in a `ToolCtx`.
struct Bound {
    records: Arc<ReadRecords>,
    session: String,
}

impl std::fmt::Debug for Bound {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Reads")
            .field("session", &self.session)
            .finish()
    }
}

impl Reads for Bound {
    fn noted(&self, path: &Path, mark: Mark) {
        self.records.note(&self.session, path, mark);
    }
    fn mark_of(&self, path: &Path) -> Option<Mark> {
        self.records.mark_of(&self.session, path)
    }
    fn wrote(&self, path: &Path, mark: Mark) {
        self.records.wrote(&self.session, path, mark);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn mark(n: u64) -> Mark {
        Mark {
            len: n,
            mtime_ns: u128::from(n) * 1_000,
        }
    }

    fn path(n: usize) -> PathBuf {
        PathBuf::from(format!("/w/f{n}.txt"))
    }

    #[test]
    fn past_the_cap_the_oldest_note_is_the_one_gone() {
        let r = ReadRecords::default();
        let over = 5;
        for n in 0..MAX_PATHS + over {
            r.note("ses_cap", &path(n), mark(n as u64));
        }
        // Exactly the cap is kept, not "fewer than N".
        assert_eq!(r.len_of("ses_cap"), MAX_PATHS);
        // The first `over` are the ones gone, and every later one is held,
        // with the mark it was noted with.
        for n in 0..over {
            assert_eq!(
                r.mark_of("ses_cap", &path(n)),
                None,
                "f{n} should be evicted"
            );
        }
        for n in over..MAX_PATHS + over {
            assert_eq!(
                r.mark_of("ses_cap", &path(n)),
                Some(mark(n as u64)),
                "f{n} should be held"
            );
        }
    }

    #[test]
    fn a_note_read_again_is_not_the_oldest() {
        let r = ReadRecords::default();
        for n in 0..MAX_PATHS {
            r.note("ses_lru", &path(n), mark(n as u64));
        }
        // Touch the oldest, then overflow by one: the SECOND oldest goes.
        r.note("ses_lru", &path(0), mark(9_000));
        r.note("ses_lru", &path(MAX_PATHS), mark(MAX_PATHS as u64));
        assert_eq!(r.len_of("ses_lru"), MAX_PATHS);
        assert_eq!(r.mark_of("ses_lru", &path(0)), Some(mark(9_000)));
        assert_eq!(r.mark_of("ses_lru", &path(1)), None);
    }

    #[test]
    fn a_write_moves_a_note_on_and_makes_none() {
        let r = ReadRecords::default();
        r.note("ses_w", &path(1), mark(1));
        r.wrote("ses_w", &path(1), mark(2));
        assert_eq!(r.mark_of("ses_w", &path(1)), Some(mark(2)));
        // A write to a path with no note brings it under no guard.
        r.wrote("ses_w", &path(2), mark(3));
        assert_eq!(r.mark_of("ses_w", &path(2)), None);
        assert_eq!(r.len_of("ses_w"), 1);
    }

    #[test]
    fn a_session_that_ended_holds_nothing_and_another_keeps_its_own() {
        let r = ReadRecords::default();
        r.note("ses_a", &path(1), mark(1));
        r.note("ses_b", &path(1), mark(2));
        assert_eq!(r.sessions(), 2);
        r.forget("ses_a");
        assert_eq!(r.sessions(), 1);
        assert_eq!(r.len_of("ses_a"), 0);
        assert_eq!(r.mark_of("ses_a", &path(1)), None);
        assert_eq!(r.mark_of("ses_b", &path(1)), Some(mark(2)));
    }

    #[test]
    fn past_the_session_cap_the_least_recently_written_board_is_the_one_gone() {
        let r = ReadRecords::default();
        for n in 0..MAX_SESSIONS {
            r.note(&format!("ses_{n}"), &path(0), mark(n as u64));
        }
        assert_eq!(r.sessions(), MAX_SESSIONS);
        r.note("ses_new", &path(0), mark(1));
        assert_eq!(r.sessions(), MAX_SESSIONS);
        assert_eq!(r.len_of("ses_0"), 0);
        assert_eq!(r.len_of("ses_1"), 1);
        assert_eq!(r.len_of("ses_new"), 1);
    }

    #[test]
    fn a_marks_two_fields_both_count() {
        let a = Mark {
            len: 10,
            mtime_ns: 5,
        };
        assert_ne!(
            a,
            Mark {
                len: 11,
                mtime_ns: 5
            }
        );
        assert_ne!(
            a,
            Mark {
                len: 10,
                mtime_ns: 6
            }
        );
        assert_eq!(
            a,
            Mark {
                len: 10,
                mtime_ns: 5
            }
        );
    }
}
