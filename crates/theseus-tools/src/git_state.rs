//! The repository's state in one line for the model's tail (theseus-wadu,
//! `docs/design/project-context.md` §4): the branch (or a detached head) and
//! whether tracked files differ from the index. Through gitoxide, as
//! `git.diff` and `git.log` read, never the git binary, and bounded: a path
//! that is no repository, a head that cannot be read, or a scan that runs out
//! of entries or time degrades to a word of its own (`unknown`, `changes
//! unknown`), never an error and never a held turn.
//!
//! Deliberately left out: file names, a remote's URL, untracked files, and
//! changes that are staged but not in the working tree. A branch name is
//! text a repository's author chose, so it is cut to `NAME_MAX` characters
//! and to a plain alphabet before it can reach a prompt.

use std::path::Path;
use std::time::{Duration, Instant};

/// Index entries looked at before the scan gives up.
const ENTRY_MAX: usize = 20_000;
/// The longest branch name carried.
const NAME_MAX: usize = 48;
/// The largest file whose bytes are hashed to settle a stat mismatch.
const HASH_MAX: u64 = 4 * 1024 * 1024;

/// Whether tracked files differ from the index.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Tree {
    Clean,
    Dirty,
    Unknown,
}

/// The head and the tree.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GitState {
    /// `Some(name)` on a branch; `None` detached (then `short` names the commit).
    pub branch: Option<String>,
    pub short: Option<String>,
    pub tree: Tree,
}

impl GitState {
    /// The tail's line.
    pub fn line(&self) -> String {
        let head = match (&self.branch, &self.short) {
            (Some(b), _) => format!("branch {b}"),
            (None, Some(s)) => format!("detached at {s}"),
            (None, None) => "detached".to_string(),
        };
        let tree = match self.tree {
            Tree::Clean => "tracked files clean",
            Tree::Dirty => "tracked files changed",
            Tree::Unknown => "changes unknown",
        };
        format!("Git: {head}, {tree}")
    }
}

/// The line for `dir`, or `Git: unknown` when it holds no readable repository.
pub fn line(dir: &Path, budget: Duration) -> String {
    state(dir, budget).map_or_else(|| "Git: unknown".to_string(), |s| s.line())
}

fn plain(name: &str) -> String {
    name.chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || "._/-+".contains(c) {
                c
            } else {
                '?'
            }
        })
        .take(NAME_MAX)
        .collect()
}

/// The state of the repository holding `dir`, read within `budget`.
pub fn state(dir: &Path, budget: Duration) -> Option<GitState> {
    let deadline = Instant::now() + budget;
    let repo = gix::discover(dir).ok()?;
    let wd = repo.workdir()?.to_path_buf();
    let head = repo.head().ok()?;
    let branch = head
        .referent_name()
        .map(|n| plain(&n.shorten().to_string()));
    let short = repo
        .head_id()
        .ok()
        .map(|id| id.to_string().chars().take(7).collect::<String>());
    let tree = scan(&repo, &wd, deadline);
    Some(GitState {
        branch,
        short,
        tree,
    })
}

fn scan(repo: &gix::Repository, wd: &Path, deadline: Instant) -> Tree {
    let Ok(index) = repo.index_or_empty() else {
        return Tree::Unknown;
    };
    for (n, e) in index.entries().iter().enumerate() {
        if n >= ENTRY_MAX || (n % 64 == 0 && Instant::now() >= deadline) {
            return Tree::Unknown;
        }
        if e.mode.is_submodule() {
            continue;
        }
        let path = e.path(&index);
        let Ok(rel) = std::str::from_utf8(path) else {
            continue;
        };
        let full = wd.join(rel);
        let Ok(md) = std::fs::symlink_metadata(&full) else {
            return Tree::Dirty;
        };
        if md.is_dir() {
            return Tree::Dirty;
        }
        let mtime = md
            .modified()
            .ok()
            .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
            .map(|d| d.as_secs() as u32);
        if md.len() as u32 == e.stat.size && mtime == Some(e.stat.mtime.secs) {
            continue;
        }
        // The stat moved (a touch, a checkout): only the bytes can say.
        if md.len() > HASH_MAX {
            return Tree::Unknown;
        }
        let data = if md.file_type().is_symlink() {
            use std::os::unix::ffi::OsStrExt;
            std::fs::read_link(&full)
                .map(|p| p.as_os_str().as_bytes().to_vec())
                .ok()
        } else {
            std::fs::read(&full).ok()
        };
        let Some(data) = data else {
            return Tree::Dirty;
        };
        let same = gix::objs::compute_hash(gix::hash::Kind::Sha1, gix::objs::Kind::Blob, &data)
            .is_ok_and(|id| id == e.id);
        if !same {
            return Tree::Dirty;
        }
    }
    Tree::Clean
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::process::Command;

    fn git(dir: &Path, args: &[&str]) {
        let st = Command::new("git")
            .args(args)
            .current_dir(dir)
            .env("GIT_CONFIG_GLOBAL", "/dev/null")
            .env("GIT_CONFIG_NOSYSTEM", "1")
            .env("GIT_AUTHOR_NAME", "Tester")
            .env("GIT_AUTHOR_EMAIL", "t@example.com")
            .env("GIT_COMMITTER_NAME", "Tester")
            .env("GIT_COMMITTER_EMAIL", "t@example.com")
            .status()
            .unwrap();
        assert!(st.success(), "git {args:?}");
    }

    const WAIT: Duration = Duration::from_secs(5);

    #[test]
    fn a_directory_that_is_no_repository_is_unknown_not_an_error() {
        let d = tempfile::tempdir().unwrap();
        // Walls the discovery in, so a stray repository above the temp dir cannot answer.
        std::env::set_var("GIT_CEILING_DIRECTORIES", d.path().parent().unwrap());
        assert_eq!(line(d.path(), WAIT), "Git: unknown");
        assert_eq!(line(&d.path().join("absent"), WAIT), "Git: unknown");
    }

    #[test]
    fn a_branch_reads_clean_then_dirty_and_a_detached_head_names_its_commit() {
        if Command::new("git").arg("--version").output().is_err() {
            return;
        }
        let d = tempfile::tempdir().unwrap();
        let root = d.path();
        git(root, &["init", "-q", "-b", "harbour"]);
        std::fs::write(root.join("a.txt"), "one\n").unwrap();
        git(root, &["add", "."]);
        git(root, &["commit", "-q", "-m", "first"]);
        assert_eq!(line(root, WAIT), "Git: branch harbour, tracked files clean");
        // An untracked file and a touch without a change are not changes.
        std::fs::write(root.join("new.txt"), "x\n").unwrap();
        std::fs::write(root.join("a.txt"), "one\n").unwrap();
        assert_eq!(line(root, WAIT), "Git: branch harbour, tracked files clean");
        std::fs::write(root.join("a.txt"), "two\n").unwrap();
        assert_eq!(line(root, WAIT), "Git: branch harbour, tracked files changed");
        git(root, &["checkout", "-q", "-f", "--detach"]);
        let l = line(root, WAIT);
        assert!(l.starts_with("Git: detached at ") && l.ends_with(", tracked files clean"), "{l}");
    }

    #[test]
    fn a_hostile_branch_name_is_cut_and_plain() {
        assert_eq!(plain("a b\nc"), "a?b?c");
        assert_eq!(plain(&"x".repeat(200)).len(), NAME_MAX);
    }

    #[test]
    fn a_spent_budget_gives_changes_unknown() {
        if Command::new("git").arg("--version").output().is_err() {
            return;
        }
        let d = tempfile::tempdir().unwrap();
        git(d.path(), &["init", "-q", "-b", "m"]);
        std::fs::write(d.path().join("a"), "1").unwrap();
        git(d.path(), &["add", "."]);
        git(d.path(), &["commit", "-q", "-m", "c"]);
        // Zero budget: the first entry check (n = 0) is already late.
        assert_eq!(line(d.path(), Duration::ZERO), "Git: branch m, changes unknown");
    }
}
