//! A join leaves no tree open (issue 6, the install's reap and its guard).
//!
//! A tree a join phase opens for itself is closed before the phase ends: the keel check's,
//! the planted revert's, the install's, a probe's and a revert's. One left behind holds the
//! pipeline's slot for good (a 102 MB install tree stood open for fifteen hours, and every
//! start in that time was held on "slots" with nothing to clear it). The guard asks, before
//! each phase and at the end of the pass, whether any is still open. In a test it fails; on
//! the machine it says which tree, so the hold on the slot is explained and not a mystery.
//! The merge's own tree stays open across the phases that use it, and is checked at the end of
//! the pass only.
//!
//! **A phase's tree left by a daemon that is gone is closed at start** (issue 41: a restart in the
//! middle of an install left `install-…` open, and every start waited on "slots" for nine hours).
//! The evidence that no pass of its own is running is this process's own memory:
//! - every phase tree is opened by `join::git::detached`, which records it here before the tree
//!   exists ([`opened_here`]);
//! - a pass runs in the daemon that opened its tree, and only one daemon serves a state directory,
//!   because the store's lock is exclusive (a second daemon fails to open the store, so it never
//!   reaches this sweep). A daemon with another state directory has another trees directory.
//!
//! So a phase tree with no record in this process was opened by a process that has ended: its
//! pass cannot still be running. No live pass can match that, because its tree is recorded before
//! it is made, and the check and the close hold the same lock the record takes. Closing it frees
//! the slot and does nothing else: no join, install or revert is started or stopped by it. A tree
//! a gate still holds (`tree_lease`) is left alone.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, MutexGuard, PoisonError};

use crate::rsi::tree::{self, Place, Tree};

/// The trees this process opened (`join::git::detached`), by their canonical paths.
static OPENED_HERE: Mutex<BTreeSet<PathBuf>> = Mutex::new(BTreeSet::new());

fn opened() -> MutexGuard<'static, BTreeSet<PathBuf>> {
    OPENED_HERE.lock().unwrap_or_else(PoisonError::into_inner)
}

/// Record that this process opens the tree at `dir`, before it exists.
pub fn opened_here(dir: &Path) {
    opened().insert(dir.to_path_buf());
}

/// A test's restart: what a new process knows of the trees, which is nothing.
#[cfg(test)]
pub(crate) fn forget_opened_here() {
    opened().clear();
}

/// The trees one phase opens and closes.
const PHASE_TREES: &[&str] = &["keel-", "planted-", "install-", "probe-", "revert-"];
/// The merge's tree, open from the merge to the push.
const MERGE_TREE: &str = "join-";

/// The names of the join's trees open under the trees directory; with `merge`, the merge's too.
pub fn left_open(trees: &Path, merge: bool) -> Vec<String> {
    let Ok(dir) = std::fs::read_dir(trees) else {
        return Vec::new();
    };
    let mut names: Vec<String> = dir
        .filter_map(Result::ok)
        .filter(|e| e.file_type().is_ok_and(|t| t.is_dir()))
        .map(|e| e.file_name().to_string_lossy().into_owned())
        .filter(|n| {
            PHASE_TREES.iter().any(|p| n.starts_with(p)) || (merge && n.starts_with(MERGE_TREE))
        })
        .collect();
    names.sort();
    names
}

/// Ask whether a join tree is open `when`. A test fails on one; the machine says which.
pub fn assert_closed(p: &Place, when: &str, merge: bool) {
    let open = left_open(&p.trees(), merge);
    if open.is_empty() {
        return;
    }
    let said = format!(
        "a join left a tree open {when}: {}, which holds the pipeline's slot until it is closed",
        open.join(", ")
    );
    if cfg!(test) {
        panic!("{said}");
    }
    tracing::warn!(trees = ?open, "self-join: {said}");
}

/// A phase's tree that a daemon that is gone left open, as the start found it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Dead {
    /// Its name under the trees directory (`install-72af5c3fd1ff`).
    pub tree: String,
    /// The phase that opened it: `keel`, `planted`, `install`, `probe` or `revert`.
    pub phase: String,
    /// Closed, or why it could not be.
    pub closed: Result<(), String>,
}

/// Close every phase tree under `p`'s trees directory that this process did not open (the
/// module's doc). The merge's tree (`join-…`) is not a phase's and is not touched.
pub fn close_dead(p: &Place) -> Vec<Dead> {
    let Ok(trees) = std::fs::canonicalize(p.trees()) else {
        return Vec::new();
    };
    // Held across the check and the close: a pass that opens a tree now waits for the close.
    let here = opened();
    let mut out = Vec::new();
    for name in left_open(&trees, false) {
        let dir = trees.join(&name);
        if here.contains(&dir) {
            continue;
        }
        let phase = PHASE_TREES
            .iter()
            .find(|pre| name.starts_with(**pre))
            .map_or("", |pre| pre.trim_end_matches('-'))
            .to_string();
        let t = Tree {
            slug: name.clone(),
            branch: String::new(),
            target: p.targets().join(&name),
            dir,
        };
        let closed = close_one(p, &t, phase == "install");
        out.push(Dead {
            tree: name,
            phase,
            closed,
        });
    }
    out
}

/// Close `t` as its phase does (an install's build tree stays: `install::reap_target` owns it), or
/// remove it as `join::git::detached` removes one a crash left when git no longer knows it.
fn close_one(p: &Place, t: &Tree, keep_target: bool) -> Result<(), String> {
    if let Some(busy) = crate::rsi::tree_lease::held(&[&t.dir, &t.target]) {
        return Err(format!("a gate is still running in {}", busy.display()));
    }
    let closed = match keep_target {
        true => tree::close_keeping_target(p, t),
        false => tree::close(p, t),
    };
    if closed.is_ok() && !t.dir.exists() {
        return Ok(());
    }
    tree::remove_all(&t.dir)
        .map_err(|e| format!("{} could not be removed: {e}", t.dir.display()))?;
    let _ = p.git(&p.repo, &["worktree", "prune"]);
    match t.dir.exists() {
        true => Err(format!("{} is still there", t.dir.display())),
        false => Ok(()),
    }
}
