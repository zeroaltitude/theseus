//! Computing each target's recovery word (`p5kln-state-builder`, row B6
//! `theseus-2yowd`).
//!
//! WHY THIS EXISTS. `undo.v1` asks whether a tool call could be put back, and
//! `builders/undo.rs` carries the answer to that question for each target as a
//! [`Recovery`] word. Its own header says the words have to be computed and
//! that the module itself computes nothing: it is a renderer, laying out words
//! a caller hands it, and at base `75f19f3c` the only caller is the labelled
//! eval set, which states every word by hand. So the measured 48/54 says how
//! well Jev uses these facts and not that the daemon can supply them. This
//! module is the part that supplies them.
//!
//! TWO HALVES, DELIBERATELY SPLIT. [`recovery_of`] is pure: it takes what was
//! found about one target and returns the word. [`Probe`] is what finds it, and
//! [`GitCli`] is the one implementation that asks the real tree. The split is
//! not decoration — the precedence in `recovery_of` is the whole classifier
//! decision and has to be readable and testable without a repository, while
//! the probe is three `git` invocations whose behaviour belongs in a test
//! against a real repository.
//!
//! THE PRECEDENCE, AND WHY IT IS THIS ORDER. The words are not alternatives;
//! several can be true of one target at once, and the strongest true one is the
//! answer, because the question is "could this be put back" and any one way
//! back is enough.
//!
//!   1. `absent` — the target is not there. Nothing can be lost from a path
//!      that holds nothing, and creating it is undone by deleting it. First,
//!      because every other word is about bytes that exist.
//!   2. `tracked_clean` — in the repository and matching HEAD. The strongest
//!      real recovery there is: a checkout brings back exactly these bytes,
//!      from a store that outlives the session and the daemon.
//!   3. `read_this_session` — this session read these bytes and its own record
//!      still holds them. Above `tracked_dirty` on purpose: it holds the
//!      CURRENT bytes, where a dirty tree's committed version holds older
//!      ones, so it is the better way back to what the call would destroy.
//!   4. `tracked_dirty` — in the repository with uncommitted changes. An older
//!      committed version survives; the uncommitted bytes do not.
//!   5. `untracked` — inside a repository and not in it: nothing holds a copy.
//!   6. `outside_repo` — under no repository: nothing holds a copy.
//!   7. `unknown` — the probe could not tell. The pack is told to read it as
//!      no copy, so an unreadable tree errs toward holding the call.
//!
//! `regenerable` IS RESOLVED FROM A CLOSED TABLE, AND ONLY FROM ONE
//! (`p5kln-location-facts`; the owner, 2026-11-13 11:28, who named being
//! trivially recreatable as one of the three facts code should state). This
//! module used to refuse the word outright, and the refusal was right about
//! what it was aimed at: "the repository ignores the path and its first
//! component is a build-output name" is a LOCATION rule, the exact thing this
//! classifier was written to replace (`undo.v1.toml`'s header: the approve
//! lists "were a LOCATION rule — a list of paths and argv prefixes that grows
//! forever and is wrong about every call it has not seen"). What replaces the
//! refusal is not that rule with a longer list. It is `undo_regen`'s closed
//! three-line table, where every line requires a MANIFEST THAT IS PRESENT —
//! the project's own statement that it builds this directory — and where the
//! repository must ignore the path as well. Everything git ignores beside no
//! manifest stays `untracked`, which is still the safe word, because a wrong
//! `regenerable` makes a call read as reversible and lets it run where a wrong
//! `untracked` only holds a safe call. The eval set's `regenerable` cases are
//! unaffected either way: they state the word, as they always did.
//!
//! `remote_keeps` IS NOT RESOLVED HERE EITHER: whether a bucket is versioned or
//! a document has revisions is an API call, which the gate's path cannot make,
//! and the decision for v1 is to leave it `unknown` (`p5kln-remote-keeps`).
//!
//! THE COST. `GitCli` runs up to three `git` invocations for the first target
//! of a call and up to two for each one after it (the repository root is
//! cached per probe). `undo_regen` adds a fourth, `check-ignore`, **only for
//! a target its closed table has already recognised** — which is a build
//! directory and nothing else — so the ordinary target's cost is what it was.
//! That is all on the tool gate's synchronous path, so a caller holds one
//! probe for the whole call and the pack's `on_path_p95` rollback rule
//! (300 ms) is the number to watch once anything feeds it.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::process::Command;

use super::undo::{CwdKind, Recovery, Sandbox, Target};

/// What a repository says about one path.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Tracked {
    /// In the index, and the working tree matches HEAD.
    Clean,
    /// In the index, and the working tree or the index differs from HEAD.
    Dirty,
    /// Inside a repository and not in its index. An ignored file is this:
    /// git holds no copy of it either way (see the module header).
    Untracked,
    /// Under no repository.
    NotARepo,
    /// The probe could not tell.
    Unknown,
}

/// What was found about one target, and the whole of what the word is computed
/// from. Nothing here is a word: `recovery_of` chooses it.
#[derive(Debug, Clone)]
pub struct Found {
    /// The path as the plan named it, kept for the state.
    pub path: String,
    /// `read`, `write` or `exec`, as the plan's resource says.
    pub access: String,
    /// Whether anything is there now.
    pub exists: bool,
    /// What the repository says.
    pub tracked: Tracked,
    /// Whether this session read these bytes and its record still holds them.
    /// The caller answers it: the record is the core's (`toolrun::reads`),
    /// not the judge's.
    pub read_this_session: bool,
    /// Whether the project's own build makes this again, from `undo_regen`'s
    /// closed table. A caller may also set it, for a build the daemon itself
    /// ran.
    pub regenerable: bool,
}

impl Found {
    /// A target found with nothing known about it: the honest starting point,
    /// which resolves to `unknown`.
    pub fn new(path: impl Into<String>, access: impl Into<String>) -> Self {
        Self {
            path: path.into(),
            access: access.into(),
            exists: true,
            tracked: Tracked::Unknown,
            read_this_session: false,
            regenerable: false,
        }
    }
}

/// The recovery word for one target: the strongest true way back. The
/// precedence and the argument for it are in the module header.
pub fn recovery_of(f: &Found) -> Recovery {
    if !f.exists {
        return Recovery::Absent;
    }
    if f.tracked == Tracked::Clean {
        return Recovery::TrackedClean;
    }
    if f.read_this_session {
        return Recovery::ReadThisSession;
    }
    if f.tracked == Tracked::Dirty {
        return Recovery::TrackedDirty;
    }
    if f.regenerable {
        return Recovery::Regenerable;
    }
    match f.tracked {
        Tracked::Untracked => Recovery::Untracked,
        Tracked::NotARepo => Recovery::OutsideRepo,
        _ => Recovery::Unknown,
    }
}

/// One target of the call, with its word.
pub fn target_of(f: &Found) -> Target {
    Target {
        path: f.path.clone(),
        access: f.access.clone(),
        recovery: recovery_of(f),
    }
}

/// Every target of the call, in the order the plan named them.
pub fn targets_of(found: &[Found]) -> Vec<Target> {
    found.iter().map(target_of).collect()
}

/// What a program call's working directory is, from what the probe found.
/// `Scratch` is never answered: nothing code can see tells a directory made
/// for this run from any other directory outside a repository, so claiming it
/// would be a guess, and `not_a_repo` is the true weaker answer.
pub fn cwd_kind_of(root: Option<&Path>, dirty: Option<bool>) -> CwdKind {
    match (root, dirty) {
        (Some(_), Some(true)) => CwdKind::RepoDirty,
        (Some(_), Some(false)) => CwdKind::RepoClean,
        (Some(_), None) => CwdKind::Unknown,
        (None, _) => CwdKind::NotARepo,
    }
}

/// Where a program call runs, from the class the gate bound for it
/// (`sandbox::Bound::l1`). A call that is not a program is `NotAProgram`, and
/// the pack reads `l1` as losing nothing on this machine, whatever the command
/// looks like.
pub fn sandbox_of(is_program: bool, l1: bool) -> Sandbox {
    match (is_program, l1) {
        (false, _) => Sandbox::NotAProgram,
        (true, true) => Sandbox::L1,
        (true, false) => Sandbox::L0,
    }
}

/// What asks the tree about a path. One probe is held for a whole call, so its
/// cache is the call's.
pub trait Probe {
    /// Whether anything is at `path` now.
    fn exists(&mut self, path: &Path) -> bool;
    /// What the repository holding `path` says about it.
    fn tracked(&mut self, path: &Path) -> Tracked;
    /// Whether the repository holding `path` IGNORES it. `None` when there is
    /// no repository, or git could not tell. Asked only of a path
    /// `undo_regen`'s table has already recognised, so an ordinary target
    /// pays no process for it.
    fn ignored(&mut self, path: &Path) -> Option<bool>;
    /// The root of the repository holding `dir`, and whether that repository
    /// has any uncommitted change; `(None, _)` for a directory under none.
    fn repo(&mut self, dir: &Path) -> (Option<PathBuf>, Option<bool>);
}

/// The probe that asks the real `git`, caching each directory's repository
/// root for the life of the probe.
#[derive(Debug, Default)]
pub struct GitCli {
    roots: BTreeMap<PathBuf, Option<PathBuf>>,
    dirty: BTreeMap<PathBuf, Option<bool>>,
}

impl GitCli {
    /// `git` with the operator's own configuration left out, so a probe reads
    /// the repository and nothing of the machine (AGENTS.md, Item 7: a fixture
    /// never inherits git's global config; the same rule serves a probe, whose
    /// answer must not depend on who is running the daemon).
    ///
    /// AND WITH THE REPOSITORY FORBIDDEN TO NAME A COMMAND. `dir` is a tree
    /// the MODEL named, and this probe runs on the gate's synchronous path
    /// for every plain write, BEFORE any gate has spoken, with the pack live
    /// by default. So a cloned repository's own `.git/config` is attacker
    /// input that `git` would otherwise obey: `core.fsmonitor` with a
    /// non-boolean value is a command git RUNS to ask what changed, and
    /// `ls-files`, `diff`, `status` and `check-ignore` all reach it. That is
    /// execution from repository content on a path the user never chose, so
    /// both are pinned here, on the command line, where the repository's own
    /// file cannot outrank them.
    ///
    /// THE ENVIRONMENT IS NOT ENOUGH, AND NEITHER IS `-C`. `GIT_CONFIG_GLOBAL`
    /// and `GIT_CONFIG_SYSTEM` silence the MACHINE's files; they say nothing
    /// about the repository's own, which is the one a clone carries. And
    /// addressing the tree with `git -C <root>` or `--git-dir` instead of
    /// changing directory into it does not help either: git reads that
    /// repository's config whichever way it was pointed at it. Only `-c` on
    /// the command line, which outranks every file, closes it. `current_dir`
    /// stays because the probe does not know the root until `rev-parse`
    /// answers from inside the tree.
    ///
    /// Neither flag can change an answer: a filesystem monitor is a cache in
    /// front of the same facts, and none of these five invocations has a hook
    /// it legitimately needs. `core.hooksPath` is defence in depth rather than
    /// a hole observed open -- no invocation here was found to reach a hook --
    /// and it costs nothing to deny it in advance.
    fn git(dir: &Path) -> Command {
        let mut c = Command::new("git");
        c.env("GIT_CONFIG_GLOBAL", "/dev/null")
            .env("GIT_CONFIG_SYSTEM", "/dev/null")
            .env("GIT_CONFIG_NOSYSTEM", "1")
            .env("GIT_OPTIONAL_LOCKS", "0")
            .env_remove("GIT_DIR")
            .env_remove("GIT_WORK_TREE")
            .current_dir(dir)
            .args([
                "-c",
                "core.fsmonitor=false",
                "-c",
                "core.hooksPath=/dev/null",
            ]);
        c
    }

    fn ok(dir: &Path, args: &[&str]) -> Option<bool> {
        Self::git(dir)
            .args(args)
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status()
            .ok()
            .map(|s| s.success())
    }

    fn out(dir: &Path, args: &[&str]) -> Option<String> {
        let o = Self::git(dir)
            .args(args)
            .stderr(std::process::Stdio::null())
            .output()
            .ok()?;
        o.status
            .success()
            .then(|| String::from_utf8_lossy(&o.stdout).trim().to_string())
    }

    fn root_of(&mut self, dir: &Path) -> Option<PathBuf> {
        if let Some(r) = self.roots.get(dir) {
            return r.clone();
        }
        let root = Self::out(dir, &["rev-parse", "--show-toplevel"])
            .filter(|s| !s.is_empty())
            .map(PathBuf::from);
        self.roots.insert(dir.to_path_buf(), root.clone());
        root
    }
}

impl Probe for GitCli {
    fn exists(&mut self, path: &Path) -> bool {
        path.symlink_metadata().is_ok()
    }

    fn tracked(&mut self, path: &Path) -> Tracked {
        let Some(dir) = path.parent().filter(|d| !d.as_os_str().is_empty()) else {
            return Tracked::Unknown;
        };
        if self.root_of(dir).is_none() {
            return Tracked::NotARepo;
        }
        let p = path.to_string_lossy().to_string();
        match Self::ok(dir, &["ls-files", "--error-unmatch", "--", &p]) {
            None => Tracked::Unknown,
            Some(false) => Tracked::Untracked,
            // Tracked: `diff HEAD` covers both the index and the working
            // tree, so a staged change reads dirty, which it is. A repository
            // with no commit yet has no HEAD, `diff` fails, and the committed
            // bytes it would restore do not exist: dirty is the true answer.
            Some(true) => match Self::ok(dir, &["diff", "--quiet", "HEAD", "--", &p]) {
                Some(true) => Tracked::Clean,
                Some(false) | None => Tracked::Dirty,
            },
        }
    }

    /// `git check-ignore -q`, which CONSULTS THE INDEX by default: a tracked
    /// path is not reported ignored however the ignore rules read, which is
    /// exactly the discrimination `undo_regen` wants — a committed `target/`
    /// of fixtures is the repository's bytes, not the build's. Exit 0 is
    /// ignored, 1 is not, and anything else (128, no repository) is `None`.
    fn ignored(&mut self, path: &Path) -> Option<bool> {
        let dir = path.parent().filter(|d| !d.as_os_str().is_empty())?;
        self.root_of(dir)?;
        let p = path.to_string_lossy().to_string();
        let status = Self::git(dir)
            .args(["check-ignore", "-q", "--", &p])
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status()
            .ok()?;
        match status.code() {
            Some(0) => Some(true),
            Some(1) => Some(false),
            _ => None,
        }
    }

    fn repo(&mut self, dir: &Path) -> (Option<PathBuf>, Option<bool>) {
        let root = self.root_of(dir);
        let Some(root) = root else {
            return (None, None);
        };
        if let Some(d) = self.dirty.get(&root) {
            return (Some(root), *d);
        }
        let d = Self::out(&root, &["status", "--porcelain"]).map(|s| !s.is_empty());
        self.dirty.insert(root.clone(), d);
        (Some(root), d)
    }
}

/// One target resolved against the tree: the probe answers what it can, and
/// the caller supplies the one thing only it knows — `read_this_session`,
/// which comes from the session's own read record (`toolrun::reads`), because
/// the record is the core's and not the judge's. `regenerable` comes from
/// `undo_regen`'s closed table.
pub fn find(probe: &mut dyn Probe, path: &str, access: &str, read_this_session: bool) -> Found {
    let p = Path::new(path);
    Found {
        path: path.to_string(),
        access: access.to_string(),
        exists: probe.exists(p),
        tracked: probe.tracked(p),
        read_this_session,
        regenerable: super::undo_regen::regenerable(probe, p),
    }
}

#[cfg(test)]
#[path = "undo_resolve/tests.rs"]
mod tests;
