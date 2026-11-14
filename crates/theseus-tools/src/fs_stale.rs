//! The stale edit's two moves: a read notes a file's state, and a write
//! checks it (theseus-w2cs sub-item 2). The rule, and what it deliberately
//! does not cover, is `docs/design/stale-edit.md`.
//!
//! Both moves do nothing at all when the call carries no read record
//! ([`crate::ToolCtx::reads`] is `None`, which is every `for_tests` context)
//! or when the path has no note. **Absence is never a refusal**: a model that
//! learned a file's contents from `proc.run cat`, `git.diff`, an earlier turn
//! or an earlier session writes exactly as it did before this guard existed.

use std::path::Path;

use crate::reads::{Mark, Whole};
use crate::{ToolCtx, ToolFailure};

/// What a refused write says. It names the path, the re-read and the write,
/// in that order, because the recovery it describes **is** the contract: the
/// owner's decision rests on a model answering this message with a fresh read
/// and then its write. If that stops being true, this sentence is where it
/// broke.
pub fn stale_words(path: &Path) -> String {
    format!(
        "{} changed since you read it, so this change was made against contents that are no longer there. Nothing was written. Read the file again, then write.",
        path.display()
    )
}

/// A read of `path` that returned handed its bytes over: note the state the
/// file is in now, and whether the read was of the WHOLE file. The stale
/// guard reads the note either way; `undo.v1`'s resolver reads only a whole
/// one (`p5kln-location-facts`), because a window of a large file is not a
/// copy anything could be put back from.
pub fn noted(ctx: &ToolCtx, path: &Path, whole: Whole) {
    let (Some(r), Some(m)) = (ctx.reads.as_ref(), Mark::at(path)) else {
        return;
    };
    r.noted(path, m, whole);
}

/// Refuse a write to `path` when this session was handed it and it has
/// changed since. Ok in every other case, the unreadable file included: a
/// path whose state cannot be read is a path this guard says nothing about.
pub fn fresh(ctx: &ToolCtx, path: &Path) -> Result<(), ToolFailure> {
    let Some(was) = ctx.reads.as_ref().and_then(|r| r.mark_of(path)) else {
        return Ok(());
    };
    match Mark::at(path) {
        Some(now) if now != was => Err(ToolFailure::new(stale_words(path))),
        _ => Ok(()),
    }
}

/// This call wrote `path`: move its note on to what the file is now, so the
/// next write of the same file is not refused by this one, and record that
/// this session's own record holds those bytes. **It still does not bring the
/// path under the stale guard**: the note a write alone makes is unguarded,
/// so `fresh` says nothing about it and "no read, no refusal" holds. What the
/// note gains is the second reader's word (`p5kln-location-facts`): a session
/// rewriting a file it wrote itself destroys nothing nothing holds.
///
/// **A window, left open on purpose.** `Mark::at(path)` runs here, after the
/// caller's write has landed, so a third party's write between that write and
/// this stat is recorded as this session's own state, and the next write is
/// not refused when it should be. This guard notes and checks; it does not
/// hold under a concurrent writer. No lock, no mark taken before the write.
pub fn wrote(ctx: &ToolCtx, path: &Path) {
    let (Some(r), Some(m)) = (ctx.reads.as_ref(), Mark::at(path)) else {
        return;
    };
    r.wrote(path, m);
}
