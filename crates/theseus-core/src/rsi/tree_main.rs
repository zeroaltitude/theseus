//! Main brought into a finished change's working copy (issue 20: "a finished change that no
//! longer merges is given up").
//!
//! A green branch has no tree: `self.propose_change` closes it after the push, and the branch
//! stays. When main has moved so that the branch no longer merges, the loop used to close what
//! there was and start the item again from the new main, throwing the finished work away. The
//! owner's decision (2026-11-15, message 1571650848512151645, "A seems exactly right") is the
//! other way: [`bring_main_in`] opens a tree on the branch's own commits, on a branch of its own
//! (`tree::next_name`, as a second attempt on a reported branch gets), and merges main into it
//! **without committing**. What clashes is left in the files, as git leaves it, for the builder
//! to resolve in its sandbox as for any build: its `self.propose_change` stages everything and
//! the commit it makes is the merge's.
//!
//! The tree is the item's own, so the pipeline's start finds it (`Core::self_tree_open`) and a
//! ready item's tree holds no slot (`pipeline_hold`). Nothing here runs the branch's code: a
//! merge that stops before its commit runs no hook.

use std::path::Path;

use anyhow::{bail, Result};

use super::tree::{self, Place, Tree};

/// A working copy with main merged in, and the paths that clash.
#[derive(Debug, Clone)]
pub struct Brought {
    pub tree: Tree,
    /// The paths the merge left in conflict; none when it merged cleanly.
    pub conflicts: Vec<String>,
}

/// Open a tree for `item` on `branch`'s commits, on a new branch, and merge `main` into it
/// without committing. None when the item has a tree open already: that one is somebody's, so
/// it is left alone and the caller does what it did before. An error leaves nothing behind.
/// Blocking: git and the disk.
pub fn bring_main_in(p: &Place, item: &str, branch: &str, main: &str) -> Result<Option<Brought>> {
    if tree::find(p, item).is_some() {
        return Ok(None);
    }
    let t = {
        let _one = tree::repo_lock();
        let slug = tree::slug(item);
        let repo = std::fs::canonicalize(&p.repo)?;
        std::fs::create_dir_all(p.trees())?;
        let dir = std::fs::canonicalize(p.trees())?.join(&slug);
        if dir.exists() {
            bail!("a tree for {item} is open already at {}", dir.display());
        }
        let new = tree::next_name(p, &repo, branch)?;
        p.git_ok(
            &repo,
            &[
                "worktree",
                "add",
                "-q",
                "--no-track",
                "-b",
                &new,
                &dir.to_string_lossy(),
                &format!("refs/heads/{branch}"),
            ],
        )?;
        let target = p.targets().join(&slug);
        std::fs::create_dir_all(&target)?;
        Tree {
            slug,
            branch: new,
            dir,
            target,
        }
    };
    match merge_main(p, &t.dir, main) {
        Ok(conflicts) => Ok(Some(Brought { tree: t, conflicts })),
        Err(e) => {
            // The tree and the branch were made for this and hold nothing of anyone's.
            let _ = tree::close(p, &t);
            let _ = tree::delete_branch(p, &t.branch);
            Err(e)
        }
    }
}

/// `main` merged into the tree at `dir`, uncommitted: the paths left in conflict.
fn merge_main(p: &Place, dir: &Path, main: &str) -> Result<Vec<String>> {
    let o = p.git(dir, &["merge", "--no-commit", "--no-ff", "-q", main])?;
    match o.status.code() {
        Some(0) => Ok(vec![]),
        Some(1) => {
            let paths = p.git_ok(dir, &["diff", "--name-only", "--diff-filter=U"])?;
            let paths: Vec<String> = paths
                .lines()
                .filter(|l| !l.is_empty())
                .map(str::to_string)
                .collect();
            if paths.is_empty() {
                bail!(
                    "git could not merge main and named no path in conflict: {}",
                    String::from_utf8_lossy(&o.stderr).trim()
                );
            }
            Ok(paths)
        }
        _ => bail!(
            "git could not merge main into the tree: {}",
            String::from_utf8_lossy(&o.stderr).trim()
        ),
    }
}
