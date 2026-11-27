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
//! reviewer ran and is not the pipeline's own. **A doubt keeps the directory**: a build's
//! tree holds a builder's uncommitted work, and a tree left standing costs a slot that a
//! person can clear, where a tree wrongly removed costs work nobody can bring back. The
//! pipeline's own are a join's trees (by their prefixes), a build's (it has a build tree
//! beside it, which only `tree::open` makes, or it is a worktree of the self repo on a
//! `self/` branch), and anything git could not answer for.
//!
//! A review that never closed (the daemon stopped while it ran) is closed by the next one to
//! open: what was open before it is kept in a file beside its scratch until it closes.
//!
//! It removes directories under the self state and nothing else: a worktree a reviewer
//! registered in some other checkout is left for that checkout's own `git worktree prune`,
//! because the pipeline writes in no checkout but its own.

use std::path::{Path, PathBuf};

use crate::rsi::capacity::trees_named;
use crate::rsi::tree::{self, Place};

/// The trees a join, a dry run, a revert or a probe opens for itself (`git::detached`'s
/// callers), by the prefix each names its tree with.
const JOIN_TREES: &[&str] = &[
    "keel-", "planted-", "install-", "probe-", "revert-", "join-", "dry-",
];

/// Where the reviews keep what they make: `<state>/self/review`, outside the self trees, so
/// nothing in it holds a slot.
fn root(p: &Place) -> PathBuf {
    p.root().join("review")
}

/// `branch`'s scratch: `<state>/self/review/<slug>`.
pub fn dir(p: &Place, branch: &str) -> PathBuf {
    root(p).join(tree::slug(branch))
}

/// The file that says a scratch is open, and what was under the self trees before it: one
/// name a line.
fn mark(scratch: &Path) -> PathBuf {
    scratch.with_extension("before")
}

/// A review's scratch, open: its directory, and the self trees that were open before it.
#[derive(Debug, Clone)]
pub struct Scratch {
    pub dir: PathBuf,
    before: Vec<String>,
}

impl Scratch {
    /// The scratch at `dir`, as [`checkout`] reads it.
    pub fn at(dir: &Path) -> Self {
        Self {
            dir: dir.to_path_buf(),
            before: Vec::new(),
        }
    }
}

/// Make `branch`'s scratch, empty, after closing any review that never closed. A scratch
/// that cannot be made is said and the review runs all the same: its close still clears what
/// it leaves.
pub fn open(p: &Place, branch: &str) -> Scratch {
    for left in unclosed(p) {
        let swept = close(p, &left);
        tracing::warn!(
            dir = %left.dir.display(),
            trees = ?swept,
            "self-review: a review that never closed was closed now"
        );
    }
    let s = Scratch {
        dir: dir(p, branch),
        before: trees_named(&p.trees()),
    };
    // One an earlier review of this branch could not remove: emptied, so this review starts
    // with nothing in its way.
    if s.dir.exists() {
        let _ = std::fs::remove_dir_all(&s.dir);
    }
    let made = std::fs::create_dir_all(&s.dir)
        .and_then(|()| std::fs::write(mark(&s.dir), s.before.join("\n")));
    if let Err(e) = made {
        tracing::warn!(dir = %s.dir.display(), "self-review: its scratch could not be made: {e}");
    }
    s
}

/// The reviewed head, checked out by the daemon at `<scratch>/tree` (issue 53): a detached worktree of the self
/// repository, which the reviewer's L1 view binds read-only (`toolrun::job`) and which [`close`] removes with the
/// scratch (its directory, then `git worktree prune`). The reviewer neither makes nor removes a checkout. Its path.
pub fn checkout(p: &Place, s: &Scratch, head: &str) -> anyhow::Result<PathBuf> {
    let at = s.dir.join("tree");
    let _one = tree::repo_lock();
    p.git_ok(
        &p.repo,
        &[
            "worktree",
            "add",
            "-q",
            "--detach",
            &at.to_string_lossy(),
            head,
        ],
    )?;
    Ok(at)
}

/// The tree [`checkout`] makes in the scratch `dir`.
pub fn tree_of(dir: &Path) -> PathBuf {
    dir.join("tree")
}

/// The reviews whose mark is still there: each opened and never closed.
fn unclosed(p: &Place) -> Vec<Scratch> {
    let Ok(d) = std::fs::read_dir(root(p)) else {
        return Vec::new();
    };
    d.filter_map(Result::ok)
        .map(|e| e.path())
        .filter(|m| m.extension().is_some_and(|x| x == "before"))
        .filter_map(|m| {
            let before = std::fs::read_to_string(&m).ok()?;
            Some(Scratch {
                dir: m.with_extension(""),
                before: before.lines().map(str::to_string).collect(),
            })
        })
        .collect()
}

/// Remove the scratch, and every directory the review left under the self trees: the names
/// of those. A tree open before the review is not this review's to remove, and the
/// pipeline's own are never touched.
pub fn close(p: &Place, s: &Scratch) -> Vec<String> {
    let mut whole = true;
    if let Err(e) = std::fs::remove_dir_all(&s.dir) {
        if s.dir.exists() {
            whole = false;
            tracing::warn!(dir = %s.dir.display(), "self-review: its scratch was not removed: {e}");
        }
    }
    // Under the lock an open holds, so a build's tree is never seen half made.
    let _one = tree::repo_lock();
    let trees = p.trees();
    let mut left = Vec::new();
    for name in trees_named(&trees) {
        if s.before.contains(&name) || is_the_pipelines(p, &trees, &name) {
            continue;
        }
        let d = trees.join(&name);
        match std::fs::remove_dir_all(&d) {
            Ok(()) => left.push(name),
            Err(e) => {
                tracing::warn!(dir = %d.display(), "self-review: a tree it left was not removed: {e}");
            }
        }
    }
    let pruned = p
        .git(&p.repo, &["worktree", "prune"])
        .is_ok_and(|o| o.status.success());
    // The mark is what a later open recovers this review by: it stays until the scratch is gone and its worktree is
    // unregistered.
    if whole && pruned {
        let _ = std::fs::remove_file(mark(&s.dir));
    } else {
        tracing::warn!(dir = %s.dir.display(), "self-review: its scratch stays marked for the next review to close");
    }
    left
}

/// A review's scratch while it is open: closed when this is dropped, a cancelled review's included, unless
/// [`OpenScratch::close`] closed it already. A daemon that stops while it is open leaves its mark, and the next
/// review's [`open`] closes it before anything else.
pub struct OpenScratch {
    p: Place,
    s: Option<Scratch>,
}

impl OpenScratch {
    pub fn new(p: Place, s: Scratch) -> Self {
        Self { p, s: Some(s) }
    }

    pub fn dir(&self) -> &Path {
        self.s.as_ref().map_or(Path::new(""), |s| &s.dir)
    }

    /// Close it now: what the review left under the self trees.
    pub fn close(mut self) -> Vec<String> {
        self.s.take().map_or_else(Vec::new, |s| close(&self.p, &s))
    }
}

impl Drop for OpenScratch {
    fn drop(&mut self) {
        if let Some(s) = self.s.take() {
            let _ = close(&self.p, &s);
        }
    }
}

/// Whether the tree `name` is one the pipeline opened, or might be: a join's, a build's, or
/// one git could not answer for.
fn is_the_pipelines(p: &Place, trees: &Path, name: &str) -> bool {
    if JOIN_TREES.iter().any(|j| name.starts_with(j)) {
        return true;
    }
    // A build's tree has its build tree beside it from the moment it is open, whatever its
    // builder has since checked out in it.
    if p.targets().join(name).exists() {
        return true;
    }
    let dir = trees.join(name);
    if !dir.join(".git").exists() {
        // No checkout at all: nothing a build or a join makes.
        return false;
    }
    // Whose it is, read from the files and never by running git in it: the daemon's git runs
    // only in the repo's own worktrees (`tree_git`), and this one may be a reviewer's clone. One
    // the repo lists is its own whatever its `.git` now says.
    use crate::rsi::tree_git::{admin_of, common_dir};
    if admin_of(&p.repo, &dir).is_none() {
        let (Some(its), Some(ours)) = (common_dir(&dir), common_dir(&p.repo)) else {
            tracing::warn!(dir = %dir.display(), "self-review: git did not say whose this tree is, so it stays");
            return true;
        };
        if its != ours {
            return false;
        }
        tracing::warn!(dir = %dir.display(), "self-review: a tree of this repo it does not list, so it stays");
        return true;
    }
    match p.git_ok(&dir, &["rev-parse", "--abbrev-ref", "HEAD"]) {
        Ok(branch) => branch.starts_with("self/"),
        Err(_) => true,
    }
}
