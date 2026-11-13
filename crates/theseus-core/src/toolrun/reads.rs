//! The read record: what each session has been handed, so `fs.edit` and
//! `fs.patch` can tell a write of their own reading apart from a write over
//! a change that landed underneath it (theseus-w2cs sub-item 2) -- and
//! **whose bytes the session's own record holds**, which is `undo.v1`'s
//! second missing fact (`p5kln-location-facts`, row B6 `theseus-2yowd`; the
//! owner, 2026-11-13 11:28).
//!
//! The rule is `docs/design/stale-edit.md`. The type a toollet sees is
//! `theseus_tools::reads` ([`Mark`], [`Reads`]); this is the board behind it.
//!
//! **TWO READERS, TWO WORDS, ONE NOTE.** This board was built for the stale
//! guard, and `p5kln-location-facts` found it was already most of what the
//! reversibility classifier's "do we hold a copy" needed: per session, in
//! memory, keyed so a change by anyone else is detected, capped, and dropped
//! with the session. What it was missing was two distinctions, and both are
//! on the note rather than in a second map, so the two readers cannot drift
//! apart:
//!
//!   * `guarded` -- the stale guard's question. Set by a READ only. A write
//!     never brings a path under the guard ([`ReadRecords::mark_of`] returns
//!     nothing for an unguarded note), because "no read, no refusal" is the
//!     rule the owner's decision on that guard rests on.
//!   * `whole` -- the resolver's question. Set by a read of every byte, and
//!     by a write, because a session that wrote the file holds what it wrote.
//!     Not set by a window of a file over the read cap.
//!
//! **AFTER A RESTART THIS BOARD IS EMPTY, AND THAT ERRS TOWARD HOLDING.** It
//! is in memory and nothing persists it, by design: the daemon that comes
//! back has no record of anyone's bytes, so [`ReadRecords::holds`] answers
//! false for every path, `read_this_session` is false, and a write over an
//! untracked file reads as "nothing holds a copy" -- which is the direction
//! that HOLDS the call rather than running it. A persisted board would be the
//! dangerous one: it would claim a copy the process can no longer produce.
//! `a_restarted_daemon_holds_nothing_and_errs_toward_holding` is that test.
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

use theseus_tools::reads::{Mark, Reads, Whole};

/// Paths one session may have noted at once. Past it the oldest note goes.
pub const MAX_PATHS: usize = 256;

/// Sessions with a board at once. Past it the least recently used session's
/// whole board goes: a parked daemon with thousands of sessions holds this
/// many, not one per session that ever read a file.
pub const MAX_SESSIONS: usize = 512;

/// One path's note: when it was made (to evict the oldest), the state the
/// file was in, and the two readers' words (the module header).
#[derive(Clone, Copy)]
struct Note {
    at: u64,
    mark: Mark,
    /// The stale guard checks this path: a READ put it here.
    guarded: bool,
    /// The session's own record holds the file's whole bytes as of `mark`.
    whole: bool,
}

#[derive(Default)]
struct Board {
    /// When this session's board was last written, for the session eviction.
    touched: u64,
    notes: HashMap<PathBuf, Note>,
}

impl Board {
    /// A read's note: guarded, and whole where the read took every byte. A
    /// path already noted keeps `whole` only if THIS read was whole -- a
    /// window read after a whole one is the newer truth about what the record
    /// holds, because the file may have changed between them.
    fn note(&mut self, at: u64, path: &Path, mark: Mark, whole: bool) {
        self.touched = at;
        self.notes.insert(
            path.to_path_buf(),
            Note {
                at,
                mark,
                guarded: true,
                whole,
            },
        );
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
    /// `session` was handed `path` in the state `mark` describes, `whole` of
    /// it.
    pub fn note(&self, session: &str, path: &Path, mark: Mark, whole: Whole) {
        let mut i = self.inner.lock().unwrap_or_else(PoisonError::into_inner);
        i.seq += 1;
        let at = i.seq;
        i.boards
            .entry(session.to_string())
            .or_default()
            .note(at, path, mark, whole.is_yes());
        Self::hold_sessions(&mut i);
    }

    /// The state `session` was last handed `path` in, if it was: **the stale
    /// guard's question.** A note only a write made is not an answer -- a
    /// write never brings a path under the guard, which is what keeps "no
    /// read, no refusal" true.
    pub fn mark_of(&self, session: &str, path: &Path) -> Option<Mark> {
        let i = self.inner.lock().unwrap_or_else(PoisonError::into_inner);
        let n = i.boards.get(session)?.notes.get(path)?;
        n.guarded.then_some(n.mark)
    }

    /// `session` wrote `path` itself: move its note on to the file as it is
    /// now, or make one where there was none. **The note a write makes is
    /// unguarded**, so the stale guard still says nothing about a path this
    /// session only wrote; what it carries is `whole`, because a session that
    /// wrote the bytes holds them (`p5kln-location-facts`).
    pub fn wrote(&self, session: &str, path: &Path, mark: Mark) {
        let mut i = self.inner.lock().unwrap_or_else(PoisonError::into_inner);
        i.seq += 1;
        let at = i.seq;
        let b = i.boards.entry(session.to_string()).or_default();
        let guarded = b.notes.get(path).is_some_and(|n| n.guarded);
        b.note(at, path, mark, true);
        if let Some(n) = b.notes.get_mut(path) {
            n.guarded = guarded;
        }
        Self::hold_sessions(&mut i);
    }

    /// **`undo.v1`'s question** (`p5kln-location-facts`): does `session`'s own
    /// record hold `path`'s whole CURRENT bytes? Three things must hold, and
    /// the third is the one that makes the answer worth anything:
    ///
    ///   1. there is a note -- this session read or wrote the file;
    ///   2. it is `whole` -- not a window of a file over the read cap;
    ///   3. **the file is still in the state the note describes.** Anyone
    ///      else's write since -- another session, an editor, a program --
    ///      leaves a different `Mark`, and then the record holds bytes that
    ///      are not the ones this call would destroy, so the answer is no.
    ///
    /// One `stat`, which is what the 300 ms on-path budget can buy; the
    /// argument against a content hash is on `Mark` itself.
    pub fn holds(&self, session: &str, path: &Path) -> bool {
        let i = self.inner.lock().unwrap_or_else(PoisonError::into_inner);
        let Some(n) = i.boards.get(session).and_then(|b| b.notes.get(path)) else {
            return false;
        };
        if !n.whole {
            return false;
        }
        let was = n.mark;
        drop(i);
        Mark::at(path) == Some(was)
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
    fn noted(&self, path: &Path, mark: Mark, whole: Whole) {
        self.records.note(&self.session, path, mark, whole);
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

    /// A read of every byte, which is the common case in these tests.
    fn note_whole(r: &ReadRecords, session: &str, path: &Path, mark: Mark) {
        r.note(session, path, mark, Whole::Yes);
    }

    #[test]
    fn past_the_cap_the_oldest_note_is_the_one_gone() {
        let r = ReadRecords::default();
        let over = 5;
        for n in 0..MAX_PATHS + over {
            note_whole(&r, "ses_cap", &path(n), mark(n as u64));
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
            note_whole(&r, "ses_lru", &path(n), mark(n as u64));
        }
        // Touch the oldest, then overflow by one: the SECOND oldest goes.
        note_whole(&r, "ses_lru", &path(0), mark(9_000));
        note_whole(&r, "ses_lru", &path(MAX_PATHS), mark(MAX_PATHS as u64));
        assert_eq!(r.len_of("ses_lru"), MAX_PATHS);
        assert_eq!(r.mark_of("ses_lru", &path(0)), Some(mark(9_000)));
        assert_eq!(r.mark_of("ses_lru", &path(1)), None);
    }

    #[test]
    fn a_write_moves_a_note_on_and_brings_no_path_under_the_guard() {
        let r = ReadRecords::default();
        note_whole(&r, "ses_w", &path(1), mark(1));
        r.wrote("ses_w", &path(1), mark(2));
        assert_eq!(r.mark_of("ses_w", &path(1)), Some(mark(2)));
        // A write to a path with no note brings it under no guard: this is
        // the rule "no read, no refusal" rests on, and it is unchanged by
        // `p5kln-location-facts`. What changed is that the write now LEAVES a
        // note, unguarded, so the resolver can read it; `mark_of` -- the
        // guard's question -- still answers nothing for it.
        r.wrote("ses_w", &path(2), mark(3));
        assert_eq!(
            r.mark_of("ses_w", &path(2)),
            None,
            "a write must never bring a path under the stale guard"
        );
        assert_eq!(
            r.len_of("ses_w"),
            2,
            "it does leave a note for the resolver"
        );
    }

    #[test]
    fn a_session_that_ended_holds_nothing_and_another_keeps_its_own() {
        let r = ReadRecords::default();
        note_whole(&r, "ses_a", &path(1), mark(1));
        note_whole(&r, "ses_b", &path(1), mark(2));
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
            note_whole(&r, &format!("ses_{n}"), &path(0), mark(n as u64));
        }
        assert_eq!(r.sessions(), MAX_SESSIONS);
        note_whole(&r, "ses_new", &path(0), mark(1));
        assert_eq!(r.sessions(), MAX_SESSIONS);
        assert_eq!(r.len_of("ses_0"), 0);
        assert_eq!(r.len_of("ses_1"), 1);
        assert_eq!(r.len_of("ses_new"), 1);
    }

    // ---- `undo.v1`'s question (`p5kln-location-facts`) -------------------

    /// A real file in a throwaway directory, since `holds` stats the path.
    struct Scratch(PathBuf);

    impl Scratch {
        fn new(tag: &str) -> Self {
            use std::sync::atomic::{AtomicU32, Ordering};
            static N: AtomicU32 = AtomicU32::new(0);
            let n = N.fetch_add(1, Ordering::Relaxed);
            let d = std::env::temp_dir().join(format!(
                "theseus-reads-holds-{tag}-{}-{n}",
                std::process::id()
            ));
            let _ = std::fs::remove_dir_all(&d);
            std::fs::create_dir_all(&d).expect("the scratch directory");
            Self(d)
        }

        /// A file with `text` in it, and the mark it is now in.
        fn file(&self, name: &str, text: &str) -> (PathBuf, Mark) {
            let p = self.0.join(name);
            std::fs::write(&p, text).expect("the fixture file");
            let m = Mark::at(&p).expect("the fixture's mark");
            (p, m)
        }
    }

    impl Drop for Scratch {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    /// Rewrite `p` so its `Mark` differs, whatever the filesystem's mtime
    /// resolution: a different LENGTH is enough, and `Mark` compares both.
    fn changed_underneath(p: &Path, text: &str) {
        std::fs::write(p, text).expect("the third party's write");
    }

    #[test]
    fn a_file_this_session_wrote_is_one_its_own_record_holds() {
        // The card's first bar: a session rewriting a file it wrote is not
        // held. The write's own note is what answers it -- before
        // `p5kln-location-facts` a write left no note at all, so a file the
        // session had written two minutes ago read as one nothing holds.
        let s = Scratch::new("wrote");
        let r = ReadRecords::default();
        let (p, m) = s.file("made.txt", "the bytes this session wrote\n");
        assert!(!r.holds("ses_h", &p), "nothing is held before the write");
        r.wrote("ses_h", &p, m);
        assert!(r.holds("ses_h", &p));
        // And it is still not under the stale guard.
        assert_eq!(r.mark_of("ses_h", &p), None);
    }

    #[test]
    fn a_file_changed_underneath_the_session_is_not_one_it_holds() {
        // The card's second bar, and the reason the note is keyed at all: the
        // record holds bytes that are no longer the ones the call would
        // destroy, so the honest answer is no and the call is held.
        let s = Scratch::new("changed");
        let r = ReadRecords::default();
        let (p, m) = s.file("shared.txt", "what this session read\n");
        r.note("ses_c", &p, m, Whole::Yes);
        assert!(r.holds("ses_c", &p));
        changed_underneath(&p, "somebody else wrote this, and it is longer\n");
        assert!(
            !r.holds("ses_c", &p),
            "a change by anyone else since must be detected"
        );
    }

    #[test]
    fn a_window_of_a_large_file_is_not_a_copy_the_record_holds() {
        // `fs_window::read` hands over part of a file over the read cap. The
        // stale guard arms on it; the resolver must not read it as a copy.
        let s = Scratch::new("window");
        let r = ReadRecords::default();
        let (p, m) = s.file("big.txt", "only a window of this was handed over\n");
        r.note("ses_win", &p, m, Whole::No);
        assert!(!r.holds("ses_win", &p), "a window is not a copy");
        // The guard still sees it: the two words are independent.
        assert_eq!(r.mark_of("ses_win", &p), Some(m));
        // A later whole read of the same path makes it one.
        r.note("ses_win", &p, m, Whole::Yes);
        assert!(r.holds("ses_win", &p));
        // And a window read AFTER a whole one is the newer truth.
        r.note("ses_win", &p, m, Whole::No);
        assert!(!r.holds("ses_win", &p));
    }

    #[test]
    fn a_partial_write_over_a_window_does_not_make_it_a_copy() {
        // The gap the test above leaves: it only moves note -> note, and
        // never calls `wrote` after a `Whole::No` note. That is the path a
        // real session takes -- `fs.read` of a file over `max_read_bytes`
        // takes the window (`fs.rs:323`, `Whole::No`), then `fs.edit`
        // (`fs.rs:684`) or `fs.apply_patch` (`fs.rs:885`) routes to
        // `fs_stale::wrote`. Neither of those two writers holds the file's
        // bytes: they splice a region of bytes they never handed the model.
        // If the write upgrades the note to `whole`, `holds` answers true,
        // `read_this_session` is true, and the pack tells the model its own
        // record holds bytes it only ever saw a window of -- so a later
        // destructive call on that file reads as `reversible` and RUNS
        // UNHELD. That is the unsafe side of the resolver's own asymmetry.
        let s = Scratch::new("windowthenwrote");
        let r = ReadRecords::default();
        let (p, m) = s.file("big.txt", "only a window of this was handed over\n");
        r.note("ses_ww", &p, m, Whole::No);
        assert!(!r.holds("ses_ww", &p), "a window is not a copy");

        // The partial write lands, and notes the file as it now is. The mark
        // is the CURRENT one on purpose: `holds`'s staleness check passes, so
        // `whole` is the only thing left that can decide the answer.
        changed_underneath(
            &p,
            "only a window of this was handed over, and one line was spliced\n",
        );
        let after = Mark::at(&p).expect("the mark after the partial write");
        r.wrote("ses_ww", &p, after);
        assert!(
            !r.holds("ses_ww", &p),
            "a partial write over a window must not upgrade the note to whole: \
             the session still holds only a window of a file over the read cap"
        );
    }

    #[test]
    fn a_restarted_daemon_holds_nothing_and_errs_toward_holding() {
        // The board is in memory and nothing persists it. A fresh
        // `ReadRecords` is what the daemon has after a restart, and it
        // answers false for a path an earlier board held -- so
        // `read_this_session` is false, an untracked target reads as one
        // nothing holds a copy of, and the call is HELD rather than run.
        // That is the safe direction, and it is why this board is not
        // persisted: a persisted one would claim a copy the process can no
        // longer produce.
        let s = Scratch::new("restart");
        let (p, m) = s.file("kept.txt", "read before the restart\n");
        let before = ReadRecords::default();
        before.note("ses_r", &p, m, Whole::Yes);
        assert!(before.holds("ses_r", &p));

        let after_restart = ReadRecords::default();
        assert_eq!(after_restart.sessions(), 0);
        assert!(
            !after_restart.holds("ses_r", &p),
            "a restarted daemon must claim no copy"
        );
    }

    #[test]
    fn a_path_that_is_gone_is_not_one_the_record_holds() {
        // A note of a file since deleted: `Mark::at` is None, so the compare
        // fails and the answer is no. The target then resolves by `exists`,
        // which is `absent`, and the pack reads that as losing nothing.
        let s = Scratch::new("gone");
        let r = ReadRecords::default();
        let (p, m) = s.file("vanishes.txt", "here for now\n");
        r.note("ses_g", &p, m, Whole::Yes);
        std::fs::remove_file(&p).expect("the fixture goes");
        assert!(!r.holds("ses_g", &p));
    }

    #[test]
    fn one_sessions_record_is_never_anothers() {
        let s = Scratch::new("apart");
        let r = ReadRecords::default();
        let (p, m) = s.file("shared.txt", "one session read this\n");
        r.note("ses_one", &p, m, Whole::Yes);
        assert!(r.holds("ses_one", &p));
        assert!(!r.holds("ses_two", &p));
        r.forget("ses_one");
        assert!(
            !r.holds("ses_one", &p),
            "a session that ended holds nothing"
        );
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
