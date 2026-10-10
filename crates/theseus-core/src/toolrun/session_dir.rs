//! The session's directory (theseus-aab7): a session works where its client
//! was started. `theseus ask` sends its own current directory, the session
//! keeps it, and its tools follow it: the default `cwd` of `proc_run` and
//! `term_open`, the base of `fs_*`'s relative paths, and the default `path`
//! of `git_*`. A session without one works in `[tools] cwd`, as before.
//!
//! A directory changes no policy: the gate judges a call's resolved paths
//! against the workspace roots as always, so a read or a run outside them
//! waits for approval wherever the session works.

use std::borrow::Cow;
use std::path::{Path, PathBuf};

use theseus_tools::ToolCtx;

use super::{ToolRuntime, TurnCtx};

/// A directory a client sent, checked: an absolute path, or why not.
pub fn checked(dir: Option<&str>) -> Result<Option<String>, String> {
    match dir {
        None => Ok(None),
        Some(d) if Path::new(d).is_absolute() => Ok(Some(d.to_string())),
        Some(d) => Err(format!(
            "dir must be an absolute path (got {d:?}): the client sends its own current directory"
        )),
    }
}

impl ToolRuntime {
    /// Where a session's tools work: the directory it was given, resolved
    /// as the gate resolves a path (its symlinks, so it cannot hide a way
    /// out of the roots), else `[tools] cwd`. The one resolver: every
    /// tool's default directory, and the system block's `Directory:`.
    pub fn cwd_for(&self, dir: Option<&str>) -> PathBuf {
        match dir {
            Some(d) => theseus_tools::paths::canonical_best_effort(Path::new(d)),
            None => self.ctx.cwd.clone(),
        }
    }

    /// The context a call of this turn plans and runs with: the runtime's,
    /// in its session's directory.
    pub(crate) fn ctx_in(&self, tc: &TurnCtx<'_>) -> Cow<'_, ToolCtx> {
        match tc.dir {
            None => Cow::Borrowed(&self.ctx),
            Some(d) => {
                let mut ctx = self.ctx.clone();
                ctx.cwd = self.cwd_for(Some(d));
                Cow::Owned(ctx)
            }
        }
    }

    /// The workspace roots, when `dir` is under none of them: what the CLI
    /// says once as it starts a session there. None inside a root.
    pub fn outside_roots(&self, dir: &str) -> Option<Vec<String>> {
        let at = self.cwd_for(Some(dir));
        let roots = &self.ctx.roots;
        (!roots.iter().any(|r| at.starts_with(r)))
            .then(|| roots.iter().map(|r| r.display().to_string()).collect())
    }
}
