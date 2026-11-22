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
//! **A tree the loop opened, left by a daemon that is gone, is closed at start** (issue 41: a restart in
//! the middle of an install left `install-…` open, and every start waited on "slots" for nine hours).
//! The sweep closes only what it can PROVE is the loop's own and abandoned; everything else is left as
//! it is and reported:
//! - **the loop's own**: `join::git::detached` writes a mark for each tree it opens
//!   (`<state>/self/trees.opened/<name>`, holding the kind and when) before the tree exists, and
//!   `tree::close` removes it with the tree. A directory with no mark (an owner's worktree that happens
//!   to be called `probe-notes`, a tree an older build opened, anything else) is never touched. Nothing
//!   else writes a mark: `tree::open` / `reopen` make the items' trees, which have none;
//! - **abandoned**: this process did not open it ([`opened_here`], recorded before the tree exists; the
//!   check and the close hold the same lock the record takes), and this process owns the trees
//!   (`trees_lock`: one daemon at a time, whatever store it opened, so a `--stdio` daemon beside the
//!   service never sweeps the service's live trees);
//! - and the trees directory resolves inside the state directory.
//!
//! Closing frees the slot and does nothing else: no join, install or revert is started or stopped by
//! it. A tree a gate still holds (`tree_lease`) is left alone, and so is one git will not close: no
//! plain delete stands in for `tree::close`.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, MutexGuard, PoisonError};

use anyhow::Result;
use serde::{Deserialize, Serialize};

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

/// Whether this process opened the tree at `dir` (`join::git::detached`).
pub fn is_opened_here(dir: &Path) -> bool {
    opened().contains(dir)
}

/// A test's restart: what a new process knows of the trees, which is nothing.
#[cfg(test)]
pub(crate) fn forget_opened_here() {
    opened().clear();
}

/// The loop's mark of a tree it opened: its kind (`install`, `planted`, …) and when.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Mark {
    pub kind: String,
    pub at_ms: u64,
    pub pid: u32,
}

/// Write the mark of `name`, of kind `kind`, before its tree exists: written and synced, so a crash after it leaves
/// it on disk.
pub fn mark(p: &Place, name: &str, kind: &str) -> Result<()> {
    use std::io::Write;
    let dir = p.opened();
    std::fs::create_dir_all(&dir)?;
    let m = Mark {
        kind: kind.into(),
        at_ms: theseus_protocol::now_unix_ms(),
        pid: std::process::id(),
    };
    let mut f = std::fs::File::create(dir.join(name))?;
    f.write_all(serde_json::to_string(&m)?.as_bytes())?;
    f.sync_all()?;
    Ok(())
}

/// Remove the mark of `name` (its tree was not made, or is closed).
pub fn unmark(p: &Place, name: &str) {
    let _ = std::fs::remove_file(p.opened().join(name));
}

/// The mark of `name`, when the loop opened it; none for a directory it did not (or a mark that does not read).
pub fn mark_of(p: &Place, name: &str) -> Option<Mark> {
    let text = std::fs::read_to_string(p.opened().join(name)).ok()?;
    serde_json::from_str(&text).ok()
}

/// Whether the loop opened `name`.
pub fn marked(p: &Place, name: &str) -> bool {
    mark_of(p, name).is_some()
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

/// What the start did with one tree, and why.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Swept {
    /// The loop's own, abandoned: closed.
    Closed,
    /// The loop's own, abandoned, but it could not be closed: why. It is left as it is.
    NotClosed(String),
    /// Its mark had no tree any more: the mark is cleared.
    MarkCleared,
    /// A phase-named directory the loop did not open (no mark): left as it is.
    NotTheLoops,
}

/// One tree the start looked at.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Dead {
    /// Its name under the trees directory (`install-72af5c3fd1ff`).
    pub tree: String,
    /// The kind its mark names (`install`, `planted`, …); for one the loop did not open, its name's.
    pub phase: String,
    pub swept: Swept,
}

/// Why the start did not sweep at all.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum NoSweep {
    /// Another daemon owns the trees (`trees_lock`).
    NotOwner,
    /// The trees directory leads out of the state directory.
    Outside,
}

/// The start's sweep (the module's doc): every tree with the loop's mark that this process did not open is closed;
/// a mark with no tree is cleared; a phase-named directory with no mark is reported and left.
pub fn close_dead(p: &Place) -> Result<Vec<Dead>, NoSweep> {
    if p.owner.is_none() {
        return Err(NoSweep::NotOwner);
    }
    if !p.trees().exists() {
        return Ok(Vec::new());
    }
    if !p.trees_inside() {
        return Err(NoSweep::Outside);
    }
    let Ok(trees) = std::fs::canonicalize(p.trees()) else {
        return Ok(Vec::new());
    };
    // Held across the check and the close: a pass that opens a tree now waits for the close.
    let here = opened();
    let mut out = Vec::new();
    let mut names: Vec<String> = std::fs::read_dir(p.opened())
        .map(|d| {
            d.filter_map(Result::ok)
                .filter(|e| e.file_type().is_ok_and(|t| t.is_file()))
                .map(|e| e.file_name().to_string_lossy().into_owned())
                .filter(|n| !n.contains('/') && !n.starts_with('.'))
                .collect()
        })
        .unwrap_or_default();
    names.sort();
    for name in &names {
        let dir = trees.join(name);
        if here.contains(&dir) {
            continue;
        }
        let Some(m) = mark_of(p, name) else { continue };
        let is_tree = std::fs::symlink_metadata(&dir).is_ok_and(|md| md.file_type().is_dir());
        let swept = if !is_tree && !dir.exists() {
            unmark(p, name);
            Swept::MarkCleared
        } else if !is_tree {
            Swept::NotClosed(format!("{} is not a directory", dir.display()))
        } else {
            let t = Tree {
                slug: name.clone(),
                branch: String::new(),
                target: p.targets().join(name),
                dir,
            };
            match close_one(p, &t, m.kind == "install") {
                Ok(()) => Swept::Closed,
                Err(e) => Swept::NotClosed(e),
            }
        };
        out.push(Dead {
            tree: name.clone(),
            phase: m.kind,
            swept,
        });
    }
    for name in left_open(&trees, false) {
        if names.contains(&name) {
            continue;
        }
        let phase = PHASE_TREES
            .iter()
            .find(|pre| name.starts_with(**pre))
            .map_or("", |pre| pre.trim_end_matches('-'))
            .to_string();
        out.push(Dead {
            tree: name,
            phase,
            swept: Swept::NotTheLoops,
        });
    }
    Ok(out)
}

/// Close `t` as its phase does (an install's build tree stays: `install::reap_target` owns it). A tree a gate holds,
/// or one git will not close, is left as it is: no plain delete stands in for the close.
fn close_one(p: &Place, t: &Tree, keep_target: bool) -> Result<(), String> {
    if let Some(busy) = crate::rsi::tree_lease::held(&[&t.dir, &t.target]) {
        return Err(format!("a gate is still running in {}", busy.display()));
    }
    let closed = match keep_target {
        true => tree::close_keeping_target(p, t),
        false => tree::close(p, t),
    };
    match closed {
        Ok(()) if !t.dir.exists() => Ok(()),
        Ok(()) => Err(format!("{} is still there", t.dir.display())),
        Err(e) => Err(format!("git did not close it: {e:#}")),
    }
}
