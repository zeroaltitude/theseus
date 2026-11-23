//! The bounds on a fix's commit (issue 36), judged by code on that commit alone, before the gate
//! runs again. Each refusal has its bound's name (the `self.fix.refused` row's `bound`) and its own
//! words:
//!
//! - `size`: more lines (added and removed) or files than `[self_fix] lines` and `files`;
//! - `second`: a fix at this point of this branch was tried before ([`super::make`]);
//! - `guarded-path`: a path the join's keel table holds (`keel::classify`, the table itself, not a
//!   copy), or the loop's own judge ([`JUDGE`]): the loop does not repair its own judge;
//! - `tests`: a test that existed before the branch changed. Two readings, and either refuses:
//!   main's own keel guard run on the one commit (`keel::keel_guard`, `--base <before> --head
//!   <fix>`, as the join runs it on the branch), and the narrow rule, which needs no guard: the
//!   fix may not change a test file that is on the merge-base at all, nor remove or change an
//!   assertion or a test attribute, nor add `#[ignore]`, in any file that is on the merge-base;
//! - `history`: the tree's git state moved under the fixer ([`State`]): only the code commits.

use std::path::Path;

use anyhow::Result;

use crate::rsi::join::keel;
use crate::rsi::tree::{Place, Tree};

/// The loop's own judge, which no fix may touch (beside every path the keel table holds).
pub const JUDGE: &[&str] = &[
    // Every script, the gate and the keel guard among them, and the lints the gate holds.
    "scripts/",
    "clippy.toml",
    // The fixer's own confinement and roles, the jail its commands run in, how a red is read
    // (the second look, the machine's fault), and what a builder is told.
    "crates/theseus-core/src/rsi/own_tree.rs",
    "crates/theseus-core/src/rsi/tree_git.rs",
    "crates/theseus-core/src/rsi/roles.rs",
    "crates/theseus-core/src/rsi/jail.rs",
    "crates/theseus-core/src/rsi/rerun.rs",
    "crates/theseus-core/src/rsi/infra.rs",
    "crates/theseus-core/src/rsi/backlog/brief.rs",
    "crates/theseus-core/src/rsi/join.rs",
    "crates/theseus-core/src/rsi/join/",
    "crates/theseus-core/src/rsi/pipeline.rs",
    "crates/theseus-core/src/rsi/pipeline/",
    "crates/theseus-core/src/rsi/pr.rs",
    "crates/theseus-core/src/rsi/pr_fix.rs",
    "crates/theseus-core/src/config/self_fix.rs",
];

/// A bound that refused a fix: its name and its words.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Refusal {
    pub bound: &'static str,
    pub why: String,
}

impl Refusal {
    fn new(bound: &'static str, why: impl Into<String>) -> Self {
        Self {
            bound,
            why: why.into(),
        }
    }
}

/// One commit's change: each file with its lines added and removed, and its diff with no context.
#[derive(Debug, Clone, Default)]
pub struct Change {
    pub files: Vec<(String, u64, u64)>,
    pub diff: String,
}

impl Change {
    /// What `head` changed against `before`, read in `dir`.
    pub fn of(p: &Place, dir: &Path, before: &str, head: &str) -> Result<Self> {
        let range = format!("{before}..{head}");
        let stat = p.git_ok(dir, &["diff", "--numstat", "--no-renames", &range])?;
        let files = stat
            .lines()
            .filter_map(|l| {
                let mut f = l.splitn(3, '\t');
                let add = f.next()?.parse::<u64>().unwrap_or(0);
                let del = f.next()?.parse::<u64>().unwrap_or(0);
                Some((f.next()?.to_string(), add, del))
            })
            .collect();
        let diff = p.git_ok(dir, &["diff", "-U0", "--no-renames", &range])?;
        Ok(Self { files, diff })
    }

    /// Lines added and removed, together.
    pub fn lines(&self) -> u64 {
        self.files.iter().map(|(_, a, d)| a + d).sum()
    }

    pub fn paths(&self) -> Vec<String> {
        self.files.iter().map(|(p, _, _)| p.clone()).collect()
    }
}

/// (a) The size: at most `lines` changed lines and `files` files.
pub fn size(c: &Change, lines: u64, files: u64) -> Option<Refusal> {
    let (n, f) = (c.lines(), c.files.len() as u64);
    (n > lines || f > files).then(|| {
        Refusal::new(
            "size",
            format!(
                "the fix changed {n} lines in {f} files, and a fix may change at most {lines} \
                 lines in {files} files"
            ),
        )
    })
}

/// (c) A path the keel table holds, or the loop's own judge.
pub fn guarded(c: &Change) -> Option<Refusal> {
    let paths = c.paths();
    if let Some(h) = keel::classify(&paths, &c.diff).into_iter().next() {
        return Some(Refusal::new(
            "guarded-path",
            format!(
                "the fix touches what the keel holds for the owner ({}: {})",
                h.class, h.at
            ),
        ));
    }
    let judge = paths.iter().find(|p| {
        JUDGE
            .iter()
            .any(|j| p.as_str() == *j || (j.ends_with('/') && p.starts_with(j)))
    })?;
    Some(Refusal::new(
        "guarded-path",
        format!("the fix touches {judge}, which is the self loop's own judge"),
    ))
}

/// Whether `path` is a test file by its name: under a `tests` folder, or named as one.
pub fn is_test_file(path: &str) -> bool {
    let name = path.rsplit('/').next().unwrap_or(path);
    let mut dirs = path.split('/').rev().skip(1);
    dirs.any(|s| s == "tests" || s == "test" || s.starts_with("tests_"))
        || name == "tests.rs"
        || name.starts_with("tests_")
        || name.starts_with("test_")
        || name.ends_with("_test.rs")
        || name.ends_with("_tests.rs")
        || name.contains(".test.")
        || name.contains(".spec.")
}

/// The words of a line that is a test's check or its attribute: removing or changing one weakens
/// a test.
fn is_check(line: &str) -> bool {
    let l = line.trim();
    l.contains("assert")
        || l.starts_with("#[test")
        || l.starts_with("#[tokio::test")
        || l.contains("expect_err")
        || l.contains("should_panic")
}

/// (d) The narrow rule, on the fix's own diff (made from `before`): no test file that is on the
/// merge-base `base` is changed at all; in any file that is, no check is removed or changed and no
/// `#[ignore]` is added, and no line of a test that was in it on the merge-base is changed, nor a
/// line put right above one (`super::regions`). Files the branch itself added are the fixer's to
/// repair.
pub fn tests(p: &Place, dir: &Path, base: &str, before: &str, c: &Change) -> Option<Refusal> {
    let existed: Vec<&str> = c
        .files
        .iter()
        .map(|(path, _, _)| path.as_str())
        .filter(|path| {
            p.git(dir, &["cat-file", "-e", &format!("{base}:{path}")])
                .is_ok_and(|o| o.status.success())
        })
        .collect();
    for path in &existed {
        if is_test_file(path) {
            return Some(Refusal::new(
                "tests",
                format!(
                    "the fix changes {path}, a test file that existed before the branch: a fix may \
                     not change a test the branch did not write"
                ),
            ));
        }
    }
    let mut file = String::new();
    for l in c.diff.lines() {
        if let Some(f) = l.strip_prefix("+++ b/") {
            file = f.to_string();
            continue;
        }
        if l.starts_with("--- ") || l.starts_with("+++ ") || !existed.contains(&file.as_str()) {
            continue;
        }
        let weakened = match l.as_bytes().first() {
            Some(b'-') => is_check(&l[1..]),
            Some(b'+') => l.contains("#[ignore"),
            _ => false,
        };
        if weakened {
            return Some(Refusal::new(
                "tests",
                format!(
                    "the fix weakens a test in {file}, which existed before the branch: {}",
                    l.trim().chars().take(160).collect::<String>()
                ),
            ));
        }
    }
    inline(p, dir, base, before, c, &existed)
}

/// The region half of the narrow rule (`super::regions`), for each file on the merge-base.
fn inline(
    p: &Place,
    dir: &Path,
    base: &str,
    before: &str,
    c: &Change,
    existed: &[&str],
) -> Option<Refusal> {
    let show = |rev: &str, path: &str| p.git_ok(dir, &["show", &format!("{rev}:{path}")]).ok();
    for path in existed.iter().filter(|f| f.ends_with(".rs")) {
        let hunks = hunks_of(&c.diff, path);
        if hunks.is_empty() {
            continue;
        }
        let (Some(was), Some(on_base)) = (show(before, path), show(base, path)) else {
            continue;
        };
        if let Some(r) = super::regions::touched(&was, &on_base, &hunks) {
            return Some(Refusal::new(
                "tests",
                format!(
                    "the fix changes the test {} in {path} (lines {} to {}), which was there \
                     before the branch: a fix may not change a test the branch did not write",
                    r.name, r.start, r.end
                ),
            ));
        }
    }
    None
}

/// The `@@` lines of `path`'s part of a `-U0` diff.
fn hunks_of<'a>(diff: &'a str, path: &str) -> Vec<&'a str> {
    let (mut on, mut out) = (false, Vec::new());
    for l in diff.lines() {
        if let Some(f) = l.strip_prefix("--- ") {
            on = f.strip_prefix("a/") == Some(path);
        } else if on && l.starts_with("@@ ") {
            out.push(l);
        }
    }
    out
}

/// (d) Main's own keel guard on the fix's commit alone: any finding refuses it. A main with no
/// guard has nothing to say (the narrow rule still holds).
pub fn guard(p: &Place, dir: &Path, main: &str, before: &str, head: &str) -> Option<Refusal> {
    let hits = match keel::keel_guard(p, dir, main, before, head) {
        Ok(h) => h,
        Err(e) => {
            return Some(Refusal::new(
                "tests",
                format!("main's keel guard could not judge the fix: {e:#}"),
            ))
        }
    };
    // The guard's own lines: its summary first, then each finding; the finding is what is said.
    let first = hits.first()?.at.clone();
    let finding = hits
        .iter()
        .map(|h| h.at.trim())
        .find(|a| !a.starts_with("keel:"))
        .map_or(first, str::to_string);
    Some(Refusal::new(
        "tests",
        format!("main's keel guard found, in the fix alone: {finding}"),
    ))
}

/// The tree's git state the fixer must leave as it found it: its head, its branch and that
/// branch's ref, the stash, and no merge, rebase or pick under way.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct State {
    head: String,
    branch: String,
    tip: String,
    stash: String,
    busy: Vec<String>,
}

impl State {
    pub fn of(p: &Place, t: &Tree) -> Result<Self> {
        let git = |args: &[&str]| p.git_ok(&t.dir, args).unwrap_or_default();
        let gitdir = git(&["rev-parse", "--absolute-git-dir"]);
        let busy = [
            "MERGE_HEAD",
            "CHERRY_PICK_HEAD",
            "REVERT_HEAD",
            "rebase-merge",
            "rebase-apply",
        ]
        .into_iter()
        .filter(|f| Path::new(&gitdir).join(f).exists())
        .map(str::to_string)
        .collect();
        Ok(Self {
            head: p.git_ok(&t.dir, &["rev-parse", "HEAD"])?,
            branch: git(&["symbolic-ref", "-q", "HEAD"]),
            tip: git(&[
                "rev-parse",
                "-q",
                "--verify",
                &format!("refs/heads/{}", t.branch),
            ]),
            stash: git(&["rev-parse", "-q", "--verify", "refs/stash"]),
            busy,
        })
    }

    /// (e) Why the fixer touched the history, or none.
    pub fn moved(&self, now: &Self) -> Option<Refusal> {
        let what = if self.head != now.head {
            "its head moved"
        } else if self.branch != now.branch {
            "its branch was switched"
        } else if self.tip != now.tip {
            "its branch was moved"
        } else if self.stash != now.stash {
            "the stash changed"
        } else if !now.busy.is_empty() {
            "a merge, rebase or pick was left under way"
        } else {
            return None;
        };
        Some(Refusal::new(
            "history",
            format!(
                "the fixer touched the branch's git state ({what}): only Theseus commits a fix"
            ),
        ))
    }
}
