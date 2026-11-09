//! Whether a repository's instruction text may enter the prompt
//! (`docs/design/project-context.md` §7, §8). **Phase 1 builds no grant**, so
//! [`granted`] answers no on every live path: §8's "the default is closed",
//! and the feature therefore ships with no injection surface at all.
//!
//! **What the real grant must replace.** §7 is explicit that the list may not
//! live in the configuration file: `toolrun/guarded.rs`'s floor does not floor
//! the live config, an `fs.write` reaches it, and at notify posture the gate
//! cannot refuse, so a grant list in `theseus.toml` would be writable by a
//! tool call and "a repository can never grant itself" would be false of it.
//! The list is a META record in the store, written with its ledger row, in the
//! shape `flow::Scope.repos` already has: a `Vec<String>` of `r:<hex>` ids
//! from [`crate::flow::spec::repo_id`], keyed on the canonical root so a
//! re-pointed remote inherits nothing. The store **is** on the floor.
//!
//! So phase 2 replaces [`granted`]'s body with a read of that record, keyed by
//! `repo_id(root)` rather than by the root path, and the test seam below goes
//! with it. Nothing else in this module changes.
//!
//! The seam is a process-wide set under `#[cfg(test)]`, in the shape
//! `toolrun::session_dir::read_window` already uses for per-process test and
//! turn state: a thread-local would not do, because a compile runs on a
//! runtime worker and not on the test's own thread.

use std::path::Path;

/// Has the owner granted this repository's instruction text the authority of
/// the system block? Always no in this build: there is no grant to read.
#[cfg(not(test))]
pub fn granted(root: &Path) -> bool {
    let _ = root;
    false
}

/// Under test, the seam below answers it.
#[cfg(test)]
pub fn granted(root: &Path) -> bool {
    seam::granted(root)
}

/// A test's grant of `root`, held for as long as the guard lives. **This is
/// the only way a repository's instruction text reaches a prompt in this
/// build**: a test that wants the assembly exercised takes one.
#[cfg(test)]
#[must_use = "the grant is revoked when the guard is dropped"]
pub struct Granted(std::path::PathBuf);

#[cfg(test)]
pub fn grant_for_test(root: &Path) -> Granted {
    seam::grant(root);
    Granted(root.to_path_buf())
}

#[cfg(test)]
impl Drop for Granted {
    fn drop(&mut self) {
        seam::revoke(&self.0);
    }
}

#[cfg(test)]
mod seam {
    use std::collections::HashSet;
    use std::path::{Path, PathBuf};
    use std::sync::{Mutex, PoisonError};

    static GRANTED: Mutex<Option<HashSet<PathBuf>>> = Mutex::new(None);

    pub fn granted(root: &Path) -> bool {
        let g = GRANTED.lock().unwrap_or_else(PoisonError::into_inner);
        g.as_ref().is_some_and(|s| s.contains(root))
    }

    pub fn grant(root: &Path) {
        let mut g = GRANTED.lock().unwrap_or_else(PoisonError::into_inner);
        g.get_or_insert_with(HashSet::new)
            .insert(root.to_path_buf());
    }

    pub fn revoke(root: &Path) {
        let mut g = GRANTED.lock().unwrap_or_else(PoisonError::into_inner);
        if let Some(s) = g.as_mut() {
            s.remove(root);
        }
    }
}
