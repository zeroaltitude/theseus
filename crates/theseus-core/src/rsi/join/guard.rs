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
//! - **the loop's own, completely created**: `join::git::detached` reserves the name before the tree
//!   exists (`<state>/self/trees.opened/<name>`, stage `reserved`) and, once `git worktree add` has made
//!   it, rewrites the mark as `created` with what ties it to that one worktree ([`Made`]: its git
//!   directory, a nonce written into that git directory, and that it is detached). `tree::close` removes
//!   the mark with the tree. The sweep closes only a `created` mark whose worktree, as git lists it now,
//!   is that same one. A reservation with nothing at its path is cleared; a reservation with something
//!   there, or a `created` mark whose worktree does not match, is left and reported ("marked, but not
//!   the tree the loop made"). A directory with no mark (an owner's worktree called `probe-notes`, a tree
//!   an older build opened, anything else) is never touched. `tree::open` / `reopen` (the items' trees)
//!   write no mark, clear a stale reservation at their name, and refuse a `created` one ([`item_meets`]);
//! - **abandoned**: this process does not have it open ([`claim`], taken before anything is done at the path; the
//!   check and the close hold the same lock the claim takes), and this process owns the trees
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

/// Claim `dir` for an open by this process, before anything is done at the path: refused when this process has it
/// open already (its lifetime not ended). Ended by [`closed_here`] when the tree is closed or its open fails: a
/// record is an open this process has under way, not a count of live passes. Taken under the lock the sweep holds,
/// so the two never cross.
pub fn claim(dir: &Path) -> Result<(), String> {
    let mut here = opened();
    if here.contains(dir) {
        return Err(format!(
            "{} is open in this daemon already: it is not opened again over itself",
            dir.display()
        ));
    }
    here.insert(dir.to_path_buf());
    Ok(())
}

/// The tree at `dir` is closed, or its open failed: this process no longer has it open.
pub fn closed_here(dir: &Path) {
    opened().remove(dir);
}

/// Whether this process has the tree at `dir` open (`join::git::detached`).
pub fn is_opened_here(dir: &Path) -> bool {
    opened().contains(dir)
}

/// A test's restart: what a new process knows of the trees, which is nothing.
#[cfg(test)]
pub(crate) fn forget_opened_here() {
    opened().clear();
}

/// Where a mark stands (issue 41, round 3): written before the tree exists it is a reservation; rewritten once the
/// tree exists it is proof of that one tree.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Stage {
    /// Before `git worktree add`: nothing is proven to exist. A mark written by round 2 (no stage) reads as this.
    #[default]
    Reserved,
    /// After it: [`Made`] names the worktree.
    Created,
}

/// What ties a `created` mark to the one worktree the loop made, and cannot be true of another made later at the
/// same path: the absolute git directory git gave it (`<repo>/.git/worktrees/<name>`), a nonce the loop wrote into
/// that git directory (`theseus-mark`; it goes with the worktree when git removes it, and a later worktree's git
/// directory has none), and that it was created DETACHED at `at`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Made {
    pub git_dir: String,
    pub nonce: String,
    pub at: String,
}

/// The file in a worktree's git directory that holds the loop's nonce.
pub const NONCE_FILE: &str = "theseus-mark";

/// The loop's mark of a tree it opened: its kind (`install`, `planted`, …), when, its stage and, once created, what
/// it made.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Mark {
    pub kind: String,
    pub at_ms: u64,
    pub pid: u32,
    #[serde(default)]
    pub stage: Stage,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub made: Option<Made>,
}

/// Write `m` as the mark of `name`: written to a side file, synced, renamed over the mark, and the directory synced,
/// so a crash leaves the old mark or the new one, never half of either.
fn write_mark(p: &Place, name: &str, m: &Mark) -> Result<()> {
    use std::io::Write;
    let dir = p.opened();
    std::fs::create_dir_all(&dir)?;
    let side = dir.join(format!(".{name}.new"));
    let mut f = std::fs::File::create(&side)?;
    f.write_all(serde_json::to_string(m)?.as_bytes())?;
    f.sync_all()?;
    std::fs::rename(&side, dir.join(name))?;
    std::fs::File::open(&dir)?.sync_all()?;
    Ok(())
}

/// Reserve `name` for a tree of kind `kind`, before it exists.
pub fn mark(p: &Place, name: &str, kind: &str) -> Result<()> {
    write_mark(
        p,
        name,
        &Mark {
            kind: kind.into(),
            at_ms: theseus_protocol::now_unix_ms(),
            pid: std::process::id(),
            stage: Stage::Reserved,
            made: None,
        },
    )
}

/// The worktree at `dir` exists now: write the nonce into its git directory and the `created` mark that names it.
pub fn mark_created(p: &Place, name: &str, dir: &Path, at: &str) -> Result<()> {
    let git_dir = p.git_ok(dir, &["rev-parse", "--absolute-git-dir"])?;
    let nonce = crate::new_id("mark");
    std::fs::write(Path::new(&git_dir).join(NONCE_FILE), &nonce)?;
    let mut m = mark_of(p, name).unwrap_or(Mark {
        kind: String::new(),
        at_ms: theseus_protocol::now_unix_ms(),
        pid: std::process::id(),
        stage: Stage::Reserved,
        made: None,
    });
    m.stage = Stage::Created;
    m.made = Some(Made {
        git_dir,
        nonce,
        at: at.into(),
    });
    write_mark(p, name, &m)
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

/// Whether a mark stands at `name`.
pub fn marked(p: &Place, name: &str) -> bool {
    mark_of(p, name).is_some()
}

/// Whether the worktree at `dir` is, as git lists it now, the one a `created` mark names: registered at that path,
/// detached, with the same git directory, and that git directory holds the mark's nonce. Why not, when it is not.
pub fn is_the_made_tree(p: &Place, m: &Mark, dir: &Path) -> Result<(), String> {
    let Some(made) = (m.stage == Stage::Created)
        .then_some(m.made.as_ref())
        .flatten()
    else {
        return Err(
            "its mark is a reservation: the loop never finished making a tree there".into(),
        );
    };
    let list = p
        .git_ok(&p.repo, &["worktree", "list", "--porcelain"])
        .map_err(|e| format!("git cannot list the worktrees: {e:#}"))?;
    let path = format!("worktree {}", dir.display());
    let entry = list
        .split("\n\n")
        .find(|e| e.lines().next() == Some(path.as_str()))
        .ok_or("git does not list a worktree at that path")?;
    if !entry.lines().any(|l| l == "detached") {
        return Err(
            "the worktree at that path is on a branch, not the detached one the loop made".into(),
        );
    }
    let git_dir = p
        .git_ok(dir, &["rev-parse", "--absolute-git-dir"])
        .map_err(|e| format!("git cannot read its git directory: {e:#}"))?;
    if git_dir != made.git_dir {
        return Err(format!(
            "its git directory is {git_dir}, not {}",
            made.git_dir
        ));
    }
    let nonce = std::fs::read_to_string(Path::new(&git_dir).join(NONCE_FILE)).unwrap_or_default();
    if nonce.trim() != made.nonce {
        return Err(
            "its git directory does not hold the loop's mark: another worktree stands there".into(),
        );
    }
    Ok(())
}

/// An item's opener (`tree::open` / `reopen`) meets the name `name`: a reservation with nothing made is cleared
/// (nothing was created), a `created` mark or a name this process has open refuses the open.
pub fn item_meets(p: &Place, name: &str) -> Result<()> {
    if let Ok(trees) = std::fs::canonicalize(p.trees()) {
        if is_opened_here(&trees.join(name)) {
            anyhow::bail!("the loop has {name} open as one of its own trees: an item's tree is not opened over it");
        }
    }
    match mark_of(p, name) {
        None => Ok(()),
        Some(m) if m.stage == Stage::Reserved => {
            unmark(p, name);
            tracing::info!(tree = %name, "self: an item's tree clears the loop's stale reservation at its name");
            Ok(())
        }
        Some(m) => anyhow::bail!(
            "the loop's {} tree is marked at {name}: an item's tree is not opened over it",
            m.kind
        ),
    }
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
    /// A mark stands, but what is at its path is not the tree the loop made (a reservation with something there, or
    /// a worktree that does not match the `created` mark): why. Left as it is.
    NotTheTreeTheLoopMade(String),
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
    if p.trees().exists() && !p.trees_inside() {
        return Err(NoSweep::Outside);
    }
    // With no trees directory there is nothing to close, and every mark is stale: its path cannot exist.
    let trees = std::fs::canonicalize(p.trees()).unwrap_or_else(|_| p.trees());
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
        let swept = if !is_tree && std::fs::symlink_metadata(&dir).is_err() {
            unmark(p, name);
            Swept::MarkCleared
        } else if !is_tree {
            Swept::NotTheTreeTheLoopMade(format!("{} is not a directory", dir.display()))
        } else if let Err(why) = is_the_made_tree(p, &m, &dir) {
            Swept::NotTheTreeTheLoopMade(why)
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
    let unmarked = match p.trees().exists() {
        true => left_open(&trees, false),
        false => Vec::new(),
    };
    for name in unmarked {
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
