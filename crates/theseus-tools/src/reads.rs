//! What a session has been handed, so a write can tell it apart from a write
//! over someone else's change (theseus-w2cs sub-item 2, the stale edit), and
//! **whose bytes the session's own record therefore holds**
//! (`p5kln-location-facts`).
//!
//! The rule is `docs/design/stale-edit.md`, and it is short: a tool that
//! hands a model a file's bytes **notes** the file's state; `fs.edit` and
//! `fs.patch` **check** that note, and only when one exists. No note is never
//! a refusal, so a model that learned a file from `proc.run cat`, `git.diff`
//! or an earlier turn writes exactly as it does today. This is a guard
//! against one race -- I read it, something else changed it, I write over
//! that change -- and not a permission system.
//!
//! **TWO READERS, ONE BOARD, AND THEY ASK DIFFERENT QUESTIONS.** The stale
//! guard asks "was this session handed this file, and has it changed since",
//! and it must keep its own rule that a WRITE never brings a path under the
//! guard. `undo.v1`'s resolver asks something else of the same board: "does
//! this session's own record hold this file's whole current bytes", for which
//! a file the session WROTE counts and a file it read only a window of does
//! not. So a note carries both words -- `guarded` for the first reader and
//! `whole` for the second -- and the two never trade places. The board is
//! theseus-core's `toolrun::reads`; the resolver's question is answered
//! there, not here.
//!
//! The board itself lives in theseus-core, off the session, so it dies with
//! the session and is capped per session. This module holds only what a
//! toollet needs: the state a file is in, and the trait the runtime injects
//! into [`crate::ToolCtx`] per call, bound to one session, as it injects
//! `Cores` and `Secrets`. A `None` board notes nothing and checks nothing,
//! which is what every `ToolCtx::for_tests` gets.

use std::path::Path;

/// A file's state, as the tool that handed its bytes over saw it: its byte
/// length and its modification time.
///
/// **No hash.** The first attempt at this guard identified a file by
/// `std::hash::DefaultHasher`, whose algorithm is explicitly not stable
/// across Rust releases, so such a value means something only inside one
/// running process and must never be stored or compared across processes.
/// Observable state avoids the question, costs one `stat` instead of a read
/// of up to 16 MiB on every edit, and is sound here because Theseus's own
/// writes go through `fs::write_atomic`, which renames a fresh file over the
/// target: any write by another tool, another session or an editor leaves a
/// new mtime. Two states a change could leave indistinguishable would need
/// the same length *and* the same mtime to nanosecond resolution.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Mark {
    /// The file's length in bytes.
    pub len: u64,
    /// Its modification time, nanoseconds since the Unix epoch.
    pub mtime_ns: u128,
}

impl Mark {
    /// The state `meta` describes. `None` when the filesystem gives no
    /// modification time, or one before the epoch: then nothing is noted and
    /// nothing is checked, which is the unguarded path, never a refusal.
    pub fn of(meta: &std::fs::Metadata) -> Option<Self> {
        let at = meta.modified().ok()?;
        let since = at.duration_since(std::time::UNIX_EPOCH).ok()?;
        Some(Self {
            len: meta.len(),
            mtime_ns: since.as_nanos(),
        })
    }

    /// The state `path` is in now, or `None` when it cannot be read.
    pub fn at(path: &Path) -> Option<Self> {
        Self::of(&std::fs::metadata(path).ok()?)
    }
}

/// Whether a read handed over the file's WHOLE bytes or only a window of
/// them. The stale guard does not care -- a note is a note -- but `undo.v1`'s
/// resolver does: "this session's own record holds these bytes" is the claim
/// that a write could be put back from it, and a window of a 20 MiB file is
/// not that.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Whole {
    /// Every byte of the file was read.
    Yes,
    /// Only part of it was (`fs_window::read`, for a file over the cap).
    No,
}

impl Whole {
    /// For the board, which keeps a plain flag.
    pub fn is_yes(self) -> bool {
        self == Whole::Yes
    }
}

/// One session's read record, as a call sees it: the runtime binds the board
/// to the calling session and hands it over in [`crate::ToolCtx::reads`].
pub trait Reads: Send + Sync + std::fmt::Debug {
    /// This call handed `path`'s bytes to the model in the state `mark`
    /// describes, `whole` of them. Past the board's per-session cap the
    /// oldest note goes.
    fn noted(&self, path: &Path, mark: Mark, whole: Whole);

    /// The state `path` was in when this session was last handed it, or
    /// `None` when it was never handed it (or its note has been evicted).
    /// **The stale guard's question**: a note a write alone made is not one,
    /// because a write must never bring a path under the guard.
    fn mark_of(&self, path: &Path) -> Option<Mark>;

    /// This call wrote `path`, so a note of it is now of the file as it was
    /// after this call's own write. **It does not bring the path under the
    /// stale guard** -- the note it makes or moves is unguarded unless a read
    /// had already guarded it, so "no read, no refusal" stays true -- but it
    /// DOES record that this session's record holds the bytes, which is what
    /// `undo.v1`'s resolver reads (`p5kln-location-facts`): a session that
    /// rewrites a file it wrote two minutes ago is not destroying anything
    /// nothing holds.
    fn wrote(&self, path: &Path, mark: Mark);
}
