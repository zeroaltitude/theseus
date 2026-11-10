//! What a session has been handed, so a write can tell it apart from a write
//! over someone else's change (theseus-w2cs sub-item 2, the stale edit).
//!
//! The rule is `docs/design/stale-edit.md`, and it is short: a tool that
//! hands a model a file's bytes **notes** the file's state; `fs.edit` and
//! `fs.patch` **check** that note, and only when one exists. No note is never
//! a refusal, so a model that learned a file from `proc.run cat`, `git.diff`
//! or an earlier turn writes exactly as it does today. This is a guard
//! against one race -- I read it, something else changed it, I write over
//! that change -- and not a permission system.
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

/// One session's read record, as a call sees it: the runtime binds the board
/// to the calling session and hands it over in [`crate::ToolCtx::reads`].
pub trait Reads: Send + Sync + std::fmt::Debug {
    /// This call handed `path`'s bytes to the model in the state `mark`
    /// describes. Past the board's per-session cap the oldest note goes.
    fn noted(&self, path: &Path, mark: Mark);

    /// The state `path` was in when this session was last handed it, or
    /// `None` when it was never handed it (or its note has been evicted).
    fn mark_of(&self, path: &Path) -> Option<Mark>;

    /// This call wrote `path`, so a note of it is now of the file as it was
    /// before this call's own write. Set it to `mark` **where a note already
    /// exists**, and create none otherwise: a write must never bring a path
    /// under the guard, or "no read, no refusal" would stop being true.
    fn wrote(&self, path: &Path, mark: Mark);
}
