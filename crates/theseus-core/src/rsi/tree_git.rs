//! What the daemon's own git reads when it runs in a self tree, and what it runs (S1 of the
//! fixer's second review). A tree is a git worktree a model writes with its file tools, and a
//! branch's gate writes in L1; the daemon's git in it is a child of the daemon, outside L1. Git
//! finds its config and hooks through the tree's `.git` file, so a tree whose `.git` named a
//! directory of the writer's own would have had the daemon run whatever that directory's config
//! said. So every daemon git goes through [`super::tree::Place::git_command`], and there:
//!
//! - **the git directory is named, never found** ([`pin`]): a tree's git runs with
//!   `--git-dir=<the repo's admin directory for that tree>` and `--work-tree=<tree>`. The admin
//!   directory is found from the repo's side, by the back-pointer `git worktree add` wrote into
//!   it ([`admin_of`]), so nothing in the tree decides which config and hooks are read;
//! - **a tree whose `.git` was changed is not used** ([`intact`]): before any git in it, its
//!   `.git` must be the regular file `git worktree add` wrote, naming that admin directory, or
//!   the call stops with [`TAMPERED`]'s words. Nothing is repaired;
//! - **a tree that holds a repository of its own is not committed** ([`nested_repo`]): a nested
//!   `.git` makes `git add` run `git status` inside it, with that repository's own config, filters
//!   included, which no `-c` reaches;
//! - **the ways config makes git run a program are off on every call** ([`HARDENED`], [`ENV`]):
//!   the file-system monitor, hooks, the pager and the editors.
//!
//! The rest (`core.sshCommand`, `credential.helper`, `gpg.program`, `url.*.insteadOf`, the
//! upload and receive packs, filter and diff drivers, `include.path`) is defined only in config,
//! which a session cannot write: the repo's git directory and git's user-level config are refused
//! to a self session's writes (`own_tree::git_refusal`), and the gate's view overlays the repo.
//! They are left as the owner set them, because the push and the signature use them.

use std::ffi::OsString;
use std::path::{Component, Path, PathBuf};
use std::process::Command;

use anyhow::{bail, Result};

/// The words a call in a tree whose `.git` was changed stops with.
pub const TAMPERED: &str = "the tree's .git file is not the one git made for it";

/// The words a commit of a tree that holds a repository of its own stops with.
pub const NESTED: &str = "the tree holds a git repository of its own";

/// On every daemon git call: no pager, no file-system monitor, no hooks (the commit and the
/// push pass `--no-verify` too, which leaves `post-commit` and `post-checkout`).
pub const HARDENED: &[&str] = &[
    "--no-pager",
    "-c",
    "core.fsmonitor=false",
    "-c",
    "core.hooksPath=/dev/null",
];

/// On every daemon git call: an editor that changes nothing, in place of `core.editor` and
/// `sequence.editor`. No daemon call asks for one; a call that did would take the message as it
/// is.
pub const ENV: &[(&str, &str)] = &[("GIT_EDITOR", "true"), ("GIT_SEQUENCE_EDITOR", "true")];

/// The repository's own git directory, where its config, hooks and worktrees are.
pub(crate) fn common_dir(repo: &Path) -> Option<PathBuf> {
    let dot = repo.join(".git");
    if std::fs::symlink_metadata(&dot).ok()?.is_dir() {
        return std::fs::canonicalize(dot).ok();
    }
    // A repo whose git directory is elsewhere: its `.git` file names it, and a linked one names
    // the common directory in its `commondir`.
    let text = std::fs::read_to_string(&dot).ok()?;
    let at = repo.join(named_by(&text)?);
    match std::fs::read_to_string(at.join("commondir")) {
        Ok(c) => std::fs::canonicalize(at.join(c.trim_end_matches('\n'))).ok(),
        Err(_) => std::fs::canonicalize(at).ok(),
    }
}

/// The directory a `.git` file's one line names.
fn named_by(text: &str) -> Option<&str> {
    text.strip_prefix("gitdir: ")?
        .strip_suffix('\n')
        .filter(|t| !t.is_empty() && !t.contains('\n'))
}

/// The admin directory the repo keeps for the worktree at `dir`: the one whose `gitdir`
/// back-pointer names `<dir>/.git`. None when `dir` is no worktree of this repo. Read from the
/// repo's side only, so nothing in `dir` decides it.
pub(crate) fn admin_of(repo: &Path, dir: &Path) -> Option<PathBuf> {
    let dir = std::fs::canonicalize(dir).ok()?;
    let common = common_dir(repo)?;
    std::fs::read_dir(common.join("worktrees"))
        .ok()?
        .flatten()
        .map(|e| e.path())
        .find(|admin| {
            let Ok(text) = std::fs::read_to_string(admin.join("gitdir")) else {
                return false;
            };
            let link = admin.join(text.trim_end_matches('\n'));
            link.file_name() == Some(".git".as_ref())
                && link
                    .parent()
                    .and_then(|p| std::fs::canonicalize(p).ok())
                    .is_some_and(|p| p == dir)
        })
}

/// The branch the repo's admin directory for the worktree at `dir` names (`HEAD` when it is
/// detached), read from the repo's side: no git runs in `dir` to say it. None when `dir` is no
/// worktree of the repo, or the repo keeps its refs in no files (reftable's `HEAD` names none).
pub(crate) fn branch_of(repo: &Path, dir: &Path) -> Option<String> {
    let head = std::fs::read_to_string(admin_of(repo, dir)?.join("HEAD")).ok()?;
    match head.trim_end().strip_prefix("ref: refs/heads/") {
        Some(".invalid") => None,
        Some(b) => Some(b.to_string()),
        None => Some("HEAD".into()),
    }
}

/// Whether `<dir>/.git` is what `git worktree add` wrote for it: a regular file (not a link,
/// not a directory) of the one line `gitdir: <admin>`, naming `admin`.
pub(crate) fn intact(dir: &Path, admin: &Path) -> bool {
    let dot = dir.join(".git");
    let regular = std::fs::symlink_metadata(&dot).is_ok_and(|m| m.file_type().is_file());
    let text = std::fs::read_to_string(&dot).unwrap_or_default();
    let (Some(named), Ok(admin)) = (named_by(&text), std::fs::canonicalize(admin)) else {
        return false;
    };
    let named = Path::new(named);
    // The absolute form is the default; the relative one is `worktree.useRelativePaths`'s.
    let names_it = if named.is_absolute() {
        named == admin
    } else {
        std::fs::canonicalize(dir.join(named)).is_ok_and(|n| n == admin)
    };
    regular && names_it
}

/// The arguments that put a daemon git call in `dir`: `-C <dir>` for the repo itself, and for
/// a worktree of it its admin directory and its work tree, named. A worktree whose `.git` was
/// changed, and a directory that is no worktree of the repo, get no git at all.
pub(crate) fn place(repo: &Path, dir: &Path) -> Result<Vec<OsString>> {
    let (args, admin) = located(repo, dir)?;
    if let Some(admin) = admin.filter(|a| !intact(dir, a)) {
        bail!(
            "{TAMPERED} ({}, which should name {}): something rewrote it, so Theseus runs no \
             git in this tree. Nothing was repaired.",
            dir.join(".git").display(),
            admin.display()
        );
    }
    Ok(args)
}

/// [`place`]'s arguments before the `.git` check, and the admin directory for a worktree.
pub(crate) fn located(repo: &Path, dir: &Path) -> Result<(Vec<OsString>, Option<PathBuf>)> {
    let mut args: Vec<OsString> = vec!["-C".into(), dir.into()];
    let canon = |p: &Path| std::fs::canonicalize(p).ok();
    if repo == dir || canon(repo).is_some_and(|r| canon(dir) == Some(r)) {
        return Ok((args, None));
    }
    let Some(admin) = admin_of(repo, dir) else {
        bail!(
            "{} is not a worktree of {}, so Theseus runs no git there",
            dir.display(),
            repo.display()
        );
    };
    args.extend(pin(dir, &admin));
    Ok((args, Some(admin)))
}

/// `--git-dir` and `--work-tree`, both absolute.
pub(crate) fn pin(dir: &Path, admin: &Path) -> [OsString; 2] {
    let canon = |p: &Path| std::fs::canonicalize(p).unwrap_or_else(|_| p.to_path_buf());
    let mut g = OsString::from("--git-dir=");
    g.push(canon(admin));
    let mut w = OsString::from("--work-tree=");
    w.push(canon(dir));
    [g, w]
}

/// The first `.git` under `dir` other than its own, walked without following links: a
/// repository the tree holds. Its own `.git` and the build tree's link (`target`) are not
/// walked into.
pub(crate) fn nested_repo(dir: &Path) -> Option<PathBuf> {
    let mut todo = vec![dir.to_path_buf()];
    while let Some(d) = todo.pop() {
        for e in std::fs::read_dir(&d).into_iter().flatten().flatten() {
            let path = e.path();
            let is_git = e.file_name().eq_ignore_ascii_case(".git");
            if is_git && d != dir {
                return Some(path);
            }
            if !is_git && e.file_type().is_ok_and(|t| t.is_dir()) {
                todo.push(path);
            }
        }
    }
    None
}

/// Whether `path` has a component named `.git` (any case: git treats `.GIT` as its own on a
/// file system that folds case).
pub(crate) fn names_dot_git(path: &Path) -> bool {
    path.components()
        .any(|c| matches!(c, Component::Normal(n) if n.eq_ignore_ascii_case(".git")))
}

/// A daemon git command over `args` in `dir`, placed and hardened: what
/// [`super::tree::Place::git_command`] spawns.
pub(crate) fn command(repo: &Path, dir: &Path, args: &[&str]) -> Result<Command> {
    let mut c = Command::new("git");
    c.args(HARDENED).args(place(repo, dir)?).args(args);
    for (k, v) in ENV {
        c.env(k, v);
    }
    Ok(c)
}
