//! A review's scratch, and what a review leaves behind under the self trees.
//!
//! The reviewer is a model with tools. To check a branch it may make itself a checkout and a
//! build, and on 2026-11-14 one did: a worktree under the self trees, which the daemon's own
//! guard on removals then would not let it delete. The capacity guard counts every directory
//! there as a slot in use (`capacity::trees_open`), so that one tree held the pipeline's only
//! slot, and for thirteen hours every start was held on "slots" with nothing to clear it.
//!
//! So a review is given a directory of its own ([`dir`]), named in its ask, and it is removed
//! when the reviewer has answered, whatever it answered. An instruction is not a guarantee,
//! so [`close`] also removes any directory that appeared under the self trees while the
//! reviewer ran and is not the pipeline's own: not a join's tree, and not a build's (a
//! worktree of the self repo on a `self/` branch). It removes directories under the self
//! state and nothing else: a worktree a reviewer registered in some other checkout is left
//! for that checkout's own `git worktree prune`, because the pipeline writes in no checkout
//! but its own.

use std::path::{Path, PathBuf};

use anyhow::{Context, Result};

use crate::rsi::capacity::trees_named;
use crate::rsi::tree::{self, Place};

/// The trees a join opens for itself (`guard`'s phase trees and the merge's).
const JOIN_TREES: &[&str] = &[
    "keel-", "planted-", "install-", "probe-", "revert-", "join-",
];

/// Where `branch`'s review keeps what it makes: `<state>/self/review/<slug>`, outside the
/// self trees, so nothing in it holds a slot.
pub fn dir(p: &Place, branch: &str) -> PathBuf {
    p.root().join("review").join(tree::slug(branch))
}

/// A review's scratch, open: its directory, and the self trees that were open before it.
#[derive(Debug)]
pub struct Scratch {
    pub dir: PathBuf,
    before: Vec<String>,
}

/// Make `branch`'s scratch, empty (one an earlier review left is removed first).
pub fn open(p: &Place, branch: &str) -> Result<Scratch> {
    let dir = dir(p, branch);
    if dir.exists() {
        std::fs::remove_dir_all(&dir)
            .with_context(|| format!("an earlier review's scratch {} stays", dir.display()))?;
    }
    std::fs::create_dir_all(&dir)
        .with_context(|| format!("the review's scratch {} could not be made", dir.display()))?;
    Ok(Scratch {
        dir,
        before: trees_named(&p.trees()),
    })
}

/// Remove the scratch, and every directory the review left under the self trees: the names
/// of those. A build's tree and a join's are never touched, and a tree open before the
/// review is not this review's to remove.
pub fn close(p: &Place, s: &Scratch) -> Vec<String> {
    if let Err(e) = std::fs::remove_dir_all(&s.dir) {
        if s.dir.exists() {
            tracing::warn!(dir = %s.dir.display(), "self-review: its scratch was not removed: {e}");
        }
    }
    // Under the lock an open holds, so a build's tree is never seen half made.
    let _one = tree::repo_lock();
    let trees = p.trees();
    let mut left = Vec::new();
    for name in trees_named(&trees) {
        if s.before.contains(&name) || JOIN_TREES.iter().any(|j| name.starts_with(j)) {
            continue;
        }
        let d = trees.join(&name);
        if is_build_tree(p, &d) {
            continue;
        }
        match std::fs::remove_dir_all(&d) {
            Ok(()) => left.push(name),
            Err(e) => {
                tracing::warn!(dir = %d.display(), "self-review: a tree it left was not removed: {e}");
            }
        }
    }
    let _ = p.git(&p.repo, &["worktree", "prune"]);
    left
}

/// Whether `dir` is a tree a build opened: a worktree of the self repo, on a `self/` branch.
fn is_build_tree(p: &Place, dir: &Path) -> bool {
    if !dir.join(".git").exists() {
        return false;
    }
    let common = |at: &Path| {
        p.git_ok(
            at,
            &["rev-parse", "--path-format=absolute", "--git-common-dir"],
        )
        .ok()
        .and_then(|d| std::fs::canonicalize(d).ok())
    };
    let (Some(its), Some(ours)) = (common(dir), common(&p.repo)) else {
        return false;
    };
    its == ours
        && p.git_ok(dir, &["rev-parse", "--abbrev-ref", "HEAD"])
            .is_ok_and(|b| b.starts_with("self/"))
}
