//! The planted revert (theseus-pw1q.9), done by code and never by the
//! reviewer: in a tree at the branch's head, the branch's new and changed
//! tests run and must pass; then every non-test change is undone (each
//! changed path put back as main has it, an added one removed) and the same
//! tests run again, and at least one must fail. A branch whose tests pass
//! without its fix is unproved, and rejected.
//!
//! A test path is one under a `tests/` directory, a `tests.rs` or a
//! `tests_*.rs`, a `*_test.rs` or `*_tests.rs`, a TypeScript `*.test.ts(x)`
//! or a file under `test/`, or a Python `test_*.py`. A Rust test file's tests
//! are the `#[test]` functions whose lines the branch added or changed (all
//! of them in a new file); any other test file is one test, its path.
//! Inline tests in a non-test file go with the revert, so they cannot prove
//! it: a branch whose only tests are inline is `no-tests`.
//!
//! What only makes a test file part of the build stays. A new tests file needs
//! one declaring line in a non-test file (`#[cfg(test)]` / `mod tests_x;`), and
//! undoing that file would take the line with the fix and leave the tests not
//! built at all. So when a changed non-test Rust file is put back, the lines
//! the branch added to it that are `mod <name>;` at the start of a line (not
//! `pub mod`, not a `use`, not inside a `mod { }` block, whose indent says so)
//! for a module whose file (`<dir>/<name>.rs` or `<dir>/<name>/mod.rs`,
//! relative to the declaring file as Rust resolves it) is a test path the
//! branch touched stay, with the added attribute lines directly above (a
//! `#[path]` one is not kept: that file cannot be resolved here). They are put
//! back at the end of the file, where a declaration's place does not matter.
//!
//! "No tests ran" is never an answer either way ([`Ran`]): the runs use
//! nextest's `--no-tests=fail`, whose exit code 4 says none of the named
//! tests is in the build. With the fix in place that is [`Outcome::NotFound`];
//! with it undone, [`Outcome::Unbuilt`]: neither passed nor failed.
//!
//! With the fix undone, how the run failed is told by nextest's exit code
//! ([`Ran::of`]). A test that failed is the proof ([`Outcome::Failed`]). A build
//! that failed (a test that names what the fix added no longer compiles) is a
//! proof too, but a weaker one, so it is its own outcome, [`Outcome::BuildBroke`]:
//! it shows that a test mentions the fix, not that it checks what the fix does.
//! It is marked in the verdict's record, and the model review is told
//! ([`Outcome::proof_note`]) to check the test by reading, until a reviewer can run an
//! experiment inside the sandbox ([`REVIEWER_CAN_RUN_THE_EXPERIMENT`]).
//! Any other exit (a setup error, a signal, a run cut short), on either run, answers
//! nothing: the step stops ([`Undecided`]), and neither proves nor rejects the change;
//! the same stop on the same head, repeated, is held for the owner (`stopped_planted`).
//! On the branch's own head a failing test (100) or a failing build (101) is its own
//! fault, [`Outcome::BaseRed`], and says which.

use std::path::Path;
use std::process::{Command, Stdio};

use anyhow::Result;
use theseus_kernel::children::{self, Kind};

use super::git;
use crate::rsi::jail::{Jail, Job};
use crate::rsi::tree::{self, Place, Tree};

/// Whether a reviewer can run the experiment that checks a build-broke proof inside the sandbox.
/// It cannot yet: a reviewer's commands run outside the sandbox by default, where the experiment
/// would build and run the change's own code as the owner before any review accepted it, and a
/// reviewer cannot build inside the sandbox. While this is `false` a broken-build proof is
/// checked by reading, and recorded so. Not a setting: it is turned on in code when it is true.
pub const REVIEWER_CAN_RUN_THE_EXPERIMENT: bool = false;

/// What the planted revert found.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Outcome {
    /// A test failed with the fix undone: the branch is proved.
    Failed { tests: Vec<String> },
    /// Only the build failed with the fix undone: proved, but by a test that no longer compiles.
    BuildBroke { tests: Vec<String> },
    /// Every test passed with the fix undone: unproved.
    Passed { tests: Vec<String> },
    /// The branch has no new or changed test to run.
    NoTests,
    /// Its tests do not pass on its own head.
    BaseRed { tail: String, build_failed: bool },
    /// None of its tests is in the build on its own head.
    NotFound { tests: Vec<String> },
    /// With the fix undone none of its tests is in the build.
    Unbuilt { tests: Vec<String> },
}

impl Outcome {
    /// Its name, as `self.review.verdict`'s `planted_revert` says it.
    pub fn name(&self) -> &'static str {
        match self {
            Outcome::Failed { .. } => "failed",
            Outcome::BuildBroke { .. } => "build-broke",
            Outcome::Passed { .. } => "passed",
            Outcome::NoTests => "no-tests",
            Outcome::BaseRed { .. } => "base-red",
            Outcome::NotFound { .. } => "not-found",
            Outcome::Unbuilt { .. } => "unbuilt",
        }
    }

    pub fn proved(&self) -> bool {
        matches!(self, Outcome::Failed { .. } | Outcome::BuildBroke { .. })
    }

    /// [`Self::proof_note`] as the reviewer's ask carries it, for what a reviewer can do today
    /// ([`REVIEWER_CAN_RUN_THE_EXPERIMENT`]): a blank line after, empty when none.
    pub fn ask_text(&self, scratch: &str) -> String {
        self.proof_note(scratch, REVIEWER_CAN_RUN_THE_EXPERIMENT)
            .map(|n| n + "\n\n")
            .unwrap_or_default()
    }

    /// How the change was proved, for the model review that follows: what it must check.
    /// `scratch` is the directory the reviewer may work in (`scratch::dir`); `experiment` says
    /// whether the reviewer can run an experiment inside the sandbox.
    pub fn proof_note(&self, scratch: &str, experiment: bool) -> Option<String> {
        match self {
            Outcome::Failed { tests } => Some(format!(
                "How it was proved: with the fix undone, at least one of its new or changed tests \
                 failed ({}).",
                tests.join(", ")
            )),
            Outcome::BuildBroke { tests } if !experiment => Some(format!(
                "How it was proved: the mechanical step could show only that, with the fix undone, \
                 the tests no longer compile ({}). That shows that a test mentions the fix and not \
                 that it checks what the fix does. An experiment (breaking the fix's behaviour in a \
                 copy and seeing a test fail) would be the stronger check, but it is not available \
                 yet, because a reviewer cannot build inside the sandbox. So you must NOT build or \
                 run this change's code in order to check this proof. Find, in the diff, at least \
                 one new or changed test whose assertion would fail if the fix's behaviour were \
                 wrong, and name that test and that assertion in a line that begins `CHECKED BY \
                 READING:`, as one of the reason lines after your `VERDICT:` line. If there is \
                 none, reject, and say so. An accept without that line does not count.",
                tests.join(", ")
            )),
            Outcome::BuildBroke { tests } => Some(format!(
                "How it was proved: the mechanical step could show only that, with the fix undone, \
                 the tests no longer compile ({}). That shows that a test mentions the fix and not \
                 that it checks what the fix does. So check it by an experiment, not only by \
                 reading. In your scratch checkout, `{scratch}/tree` (made as below if it is not \
                 there), keep the fix's new names so that everything still compiles, and make the \
                 fix's behaviour wrong by the smallest change that does it: a wrong return value, \
                 the old logic back inside the new function, the changed branch inverted. Make the \
                 change with a command (for example `git apply` or `sed -i`, run in that checkout). \
                 Run every command of the experiment with the sandbox on (`sandbox: true`), and \
                 never outside it. Then run the named tests there with the build under \
                 `{scratch}/target`, and see at \
                 least one of them FAIL on its assertion. The experiment is not the branch: it \
                 changes only files under `{scratch}`, is thrown away with it, and nothing is \
                 committed, pushed or written anywhere else. After it, your reasons must say, as \
                 reason lines after your `VERDICT:` line, one line each, `CHANGED:` what you changed, and `FAILED:` which test failed and on \
                 which assertion. If the named tests still pass with the behaviour broken, reject, \
                 and say: its tests do not check what the fix does. If you could not run the \
                 experiment (no way to break the behaviour without breaking the build, the build \
                 could not be made, no time), say so, as one of the reason lines after your \
                 `VERDICT:` line, in a line that begins `CHECKED BY READING ONLY:` with the reason, and then find, in the diff, at least one new or changed \
                 test whose assertion would fail if the fix's behaviour were wrong, and name that \
                 test and that assertion in your reasons. If there is none, reject, and say so. An \
                 accept that gives neither the `CHANGED:` and `FAILED:` lines nor a `CHECKED BY \
                 READING ONLY:` line does not count.",
                tests.join(", ")
            )),
            _ => None,
        }
    }

    /// Why it does not prove the branch, in a sentence.
    pub fn reason(&self) -> Option<String> {
        match self {
            Outcome::Failed { .. } | Outcome::BuildBroke { .. } => None,
            Outcome::Passed { tests } => Some(format!(
                "unproved: with every non-test change undone, its tests still pass ({})",
                tests.join(", ")
            )),
            Outcome::NoTests => Some(
                "unproved: the branch adds or changes no test that the planted revert can run \
                 (inline tests go with the revert; put them in a tests file)"
                    .into(),
            ),
            Outcome::BaseRed { tail, build_failed } => Some(format!(
                "its tests do not {} on its own head, so nothing can be proved ({}): {}",
                if *build_failed { "build" } else { "pass" },
                if *build_failed {
                    "the build failed"
                } else {
                    "a test failed"
                },
                tail.lines().last().unwrap_or("").trim()
            )),
            Outcome::NotFound { tests } => Some(format!(
                "unproved: its tests could not be found in the build of its own head, so nothing ran \
                 ({}); declare each tests file with a `mod` line",
                tests.join(", ")
            )),
            Outcome::Unbuilt { tests } => Some(format!(
                "unproved: with the fix undone its tests are not built, so nothing shows they fail \
                 without it ({}); declare the test module where the revert does not reach it, with a \
                 `mod` line of its own",
                tests.join(", ")
            )),
        }
    }
}

/// A run of the tests that answered nothing: it ended in none of the ways nextest ends when the
/// tests passed, a test failed, the build failed or no test was found. The step stops on it
/// (the error `run` returns), and the stop says which run and what the exit was.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Undecided {
    /// The run: with the branch's own head, or with the fix undone.
    pub second: bool,
    /// Its exit code; none for a signal.
    pub code: Option<i32>,
    /// The last line of its output.
    pub last: String,
}

impl Undecided {
    fn new(second: bool, code: Option<i32>, tail: &str) -> Self {
        let last = tail.lines().last().unwrap_or("").trim().to_string();
        Undecided { second, code, last }
    }

    /// The stop's class, for counting identical stops: which run, and the exit.
    pub fn class(&self) -> String {
        let run = if self.second {
            "second-run"
        } else {
            "first-run"
        };
        match self.code {
            Some(c) => format!("{run}-exit-{c}"),
            None => format!("{run}-signal"),
        }
    }

    /// What stopped, in words: which run and what the exit was.
    pub fn what(&self) -> String {
        let run = if self.second {
            "the run of its tests with the fix undone"
        } else {
            "the run of its tests on its own head"
        };
        match self.code {
            Some(c) => format!(
                "{run} exited with code {c}, which is neither a failing test nor a failing build"
            ),
            None => format!("{run} was cut short by a signal"),
        }
    }

    /// The `Undecided` an error from [`run`] is, if it is one.
    pub fn of(e: &anyhow::Error) -> Option<&Undecided> {
        e.downcast_ref::<Undecided>()
    }
}

impl std::fmt::Display for Undecided {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "{}, so it shows nothing either way: {}",
            self.what(),
            self.last
        )
    }
}

impl std::error::Error for Undecided {}

/// What a run of the named tests came to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Ran {
    /// They ran, and every one passed.
    Passed,
    /// A test ran and failed.
    Failed,
    /// The build failed, so no test ran.
    BuildFailed,
    /// None of them is in the build, so none ran.
    NoneRan,
    /// Any other end (a setup error, a signal, a run cut short): the exit code, none for a signal.
    Other(Option<i32>),
}

// nextest's exit codes (`nextest_metadata::NextestExitCode`).
const NONE_RAN: i32 = 4;
const TEST_RUN_FAILED: i32 = 100;
const BUILD_FAILED: i32 = 101;

impl Ran {
    /// A nextest run's exit code, as `--no-tests=fail` gives it.
    pub fn of(code: Option<i32>) -> Ran {
        match code {
            Some(0) => Ran::Passed,
            Some(NONE_RAN) => Ran::NoneRan,
            Some(TEST_RUN_FAILED) => Ran::Failed,
            Some(BUILD_FAILED) => Ran::BuildFailed,
            other => Ran::Other(other),
        }
    }
}

/// Whether an inherited variable stays out of the child's environment: Theseus's own, and nextest's
/// (`NEXTEST_PROFILE` and the rest, which a daemon or a test run under nextest holds, would make the
/// child's `cargo nextest` look for a profile the branch's own tree has not got).
fn not_for_the_child(name: &str) -> bool {
    name.starts_with("THESEUS_") || name == "NEXTEST" || name.starts_with("NEXTEST_")
}

/// Runs a branch's tests in a tree, and says how they came out. Blocking.
pub trait Tests: Send + Sync {
    fn run(&self, p: &Place, t: &Tree, tests: &[String]) -> Result<(Ran, String)>;
}

/// The workspace's tests through nextest, in the tree's own build tree, at
/// the background priority. Only Rust tests: another kind is not run here.
pub struct Cargo;

impl Tests for Cargo {
    fn run(&self, p: &Place, t: &Tree, tests: &[String]) -> Result<(Ran, String)> {
        let names: Vec<&str> = tests
            .iter()
            .filter_map(|s| s.strip_prefix("rust:"))
            .filter_map(|s| s.rsplit_once("::").map(|(_, n)| n))
            .collect();
        if names.is_empty() {
            return Ok((Ran::Passed, "no Rust test to run".into()));
        }
        let expr = names
            .iter()
            .map(|n| format!("test(/(^|::){n}$/)"))
            .collect::<Vec<_>>()
            .join(" | ");
        if let Some(jail) = p.jail()? {
            return in_l1(jail, p, t, &expr);
        }
        let mut c = Command::new("nice");
        c.args([
            "-n",
            "19",
            "cargo",
            "nextest",
            "run",
            "--workspace",
            "--no-fail-fast",
        ])
        .args(["--no-tests=fail", "-E", &expr])
        .current_dir(&t.dir)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
        for (k, _) in std::env::vars_os() {
            if not_for_the_child(&k.to_string_lossy()) {
                c.env_remove(&k);
            }
        }
        c.env("CARGO_TARGET_DIR", &t.target)
            .env("RUSTC_WRAPPER", "")
            .env("CARGO_BUILD_RUSTC_WRAPPER", "");
        for (k, v) in &p.env {
            c.env(k, v);
        }
        let o = children::spawn(Kind::Owned, || c.spawn(), |c| Some(c.id()))
            .and_then(std::process::Child::wait_with_output)?;
        let text = String::from_utf8_lossy(&o.stderr);
        let lines: Vec<&str> = text.lines().collect();
        let tail = lines[lines.len().saturating_sub(20)..].join("\n");
        Ok((Ran::of(o.status.code()), tail))
    }
}

/// The same run in L1 (theseus-pw1q.21.1): the branch's tests are the
/// branch's code, and run below the floor like its gate. Its output is a
/// log under the gates, whose tail is the verdict's words.
pub(super) fn in_l1(jail: &Jail, p: &Place, t: &Tree, expr: &str) -> Result<(Ran, String)> {
    // A branch that changed its lock files has its packages in the cache the gate made for the
    // same lock files, and made again here when that one is gone (`deps`).
    let _packages = crate::rsi::deps::provide_blocking(p, t)?;
    std::fs::create_dir_all(p.gates())?;
    let log = p.gates().join(format!("planted-{}.log", t.slug));
    let argv = [
        "nice",
        "-n",
        "19",
        "cargo",
        "nextest",
        "run",
        "--workspace",
        "--no-fail-fast",
        "--no-tests=fail",
        "-E",
        expr,
    ]
    .map(Into::into)
    .to_vec();
    let code = jail.run_blocking(
        p,
        &Job {
            tree: t,
            ro: Vec::new(),
            argv,
            env: Vec::new(),
            log: &log,
            limit: TESTS_SILENCE,
        },
    )?;
    let text = std::fs::read_to_string(&log).unwrap_or_default();
    let lines: Vec<&str> = text.lines().collect();
    Ok((
        Ran::of(code),
        lines[lines.len().saturating_sub(20)..].join("\n"),
    ))
}

/// The longest a planted revert's test run may take before it is stopped.
const TESTS_SILENCE: std::time::Duration = std::time::Duration::from_secs(2 * 3600);

/// Whether `path` holds tests only.
pub fn is_test_path(path: &str) -> bool {
    let name = path.rsplit('/').next().unwrap_or(path);
    let dirs: Vec<&str> = path.split('/').collect();
    let dirs = &dirs[..dirs.len().saturating_sub(1)];
    dirs.contains(&"tests")
        || dirs.contains(&"test")
        || name == "tests.rs"
        || (name.starts_with("tests_") && name.ends_with(".rs"))
        || name.ends_with("_test.rs")
        || name.ends_with("_tests.rs")
        || name.contains(".test.ts")
        || (name.starts_with("test_") && name.ends_with(".py"))
}

/// The `#[test]` functions of a Rust file whose lines `added` holds (1-based
/// line numbers of the head's file), as `rust:<path>::<name>`; every one
/// when `added` is `None` (a new file).
pub fn rust_tests(path: &str, text: &str, added: Option<&[(u32, u32)]>) -> Vec<String> {
    let lines: Vec<&str> = text.lines().collect();
    let mut starts: Vec<(usize, String)> = Vec::new();
    let mut attr: Option<usize> = None;
    for (i, l) in lines.iter().enumerate() {
        let t = l.trim_start();
        if t.starts_with("#[") && (t.contains("test]") || t.contains("::test")) {
            attr.get_or_insert(i);
            continue;
        }
        if let Some(a) = attr {
            if t.starts_with("#[") || t.starts_with("//") {
                continue;
            }
            let f = t
                .trim_start_matches("pub ")
                .trim_start_matches("async ")
                .strip_prefix("fn ");
            if let Some(name) = f.and_then(|f| f.split(['(', '<', ' ']).next()) {
                starts.push((a, name.to_string()));
            }
            attr = None;
        }
    }
    let mut out = Vec::new();
    for (k, (start, name)) in starts.iter().enumerate() {
        let end = starts.get(k + 1).map_or(lines.len(), |(s, _)| *s);
        let (lo, hi) = (*start as u32 + 1, end as u32);
        let touched = added.is_none_or(|r| r.iter().any(|(a, b)| *a <= hi && *b >= lo));
        if touched {
            out.push(format!("rust:{path}::{name}"));
        }
    }
    out
}

/// The head file's added line ranges, from a `-U0` diff of one file.
fn added_ranges(diff: &str) -> Vec<(u32, u32)> {
    diff.lines()
        .filter_map(|l| l.strip_prefix("@@ "))
        .filter_map(|l| l.split(' ').find(|w| w.starts_with('+')))
        .filter_map(|w| {
            let w = &w[1..];
            let (s, n) = w.split_once(',').unwrap_or((w, "1"));
            let (s, n): (u32, u32) = (s.parse().ok()?, n.parse().ok()?);
            (n > 0).then_some((s, s + n - 1))
        })
        .collect()
}

/// The branch's new and changed tests, from its changed paths.
pub fn tests_of(
    p: &Place,
    dir: &Path,
    base: &str,
    head: &str,
    paths: &[(char, String)],
) -> Result<Vec<String>> {
    let mut out = Vec::new();
    for (status, path) in paths {
        if *status == 'D' || !is_test_path(path) {
            continue;
        }
        if path.ends_with(".rs") {
            let text = std::fs::read_to_string(dir.join(path)).unwrap_or_default();
            let ranges = (*status != 'A').then(|| {
                let d = p
                    .git_ok(
                        &p.repo,
                        &["diff", "-U0", &format!("{base}...{head}"), "--", path],
                    )
                    .unwrap_or_default();
                added_ranges(&d)
            });
            out.extend(rust_tests(path, &text, ranges.as_deref()));
        } else {
            out.push(path.clone());
        }
    }
    Ok(out)
}

/// Undo every non-test change in the tree: each path back as `base` has it,
/// or removed when `base` has none, except the lines that only declare a
/// test file ([`kept_declarations`]). The paths undone.
pub fn undo_fix(p: &Place, t: &Tree, base: &str, paths: &[(char, String)]) -> Result<Vec<String>> {
    let touched = |f: &str| paths.iter().any(|(s, q)| *s != 'D' && q == f);
    let mut undone = Vec::new();
    for (status, path) in paths {
        if is_test_path(path) {
            continue;
        }
        // The branch's tests ran in this tree with it writable: a link they left is not followed.
        if let Some(at) = crate::rsi::tree_git::link_below(&t.dir, path) {
            anyhow::bail!(
                "{path} lies through a link in the tree ({}), which the branch's tests left \
                 there, so nothing was removed or restored through it",
                at.display()
            );
        }
        if *status == 'A' {
            let f = t.dir.join(path);
            if f.symlink_metadata().is_ok() {
                std::fs::remove_file(&f)?;
            }
        } else {
            let mut keep = Vec::new();
            if *status == 'M' && path.ends_with(".rs") {
                let text = std::fs::read_to_string(t.dir.join(path)).unwrap_or_default();
                let d = p.git_ok(&t.dir, &["diff", "-U0", base, "--", path])?;
                keep = kept_declarations(path, &text, &added_ranges(&d), &touched);
            }
            p.git_ok(&t.dir, &["checkout", "-q", base, "--", path])?;
            if !keep.is_empty() {
                let f = t.dir.join(path);
                let mut text = std::fs::read_to_string(&f)?;
                keep.retain(|g| {
                    !text
                        .lines()
                        .any(|x| Some(x) == g.last().map(String::as_str))
                });
                if !keep.is_empty() {
                    if !text.ends_with('\n') {
                        text.push('\n');
                    }
                    for l in keep.concat() {
                        text.push_str(&l);
                        text.push('\n');
                    }
                    std::fs::write(&f, text)?;
                }
            }
        }
        undone.push(path.clone());
    }
    Ok(undone)
}

/// The groups of lines `added` (1-based, of the head's `text` of the non-test file
/// `path`) that only make a test file part of the build: `mod <name>;` from
/// the first column, whose file is one `touched` says the branch changed and
/// `is_test_path` says holds tests, with the added `#[..]` lines directly
/// above it (none of them a `#[path]`).
pub fn kept_declarations(
    path: &str,
    text: &str,
    added: &[(u32, u32)],
    touched: &dyn Fn(&str) -> bool,
) -> Vec<Vec<String>> {
    let lines: Vec<&str> = text.lines().collect();
    let is_added = |i: usize| {
        added
            .iter()
            .any(|(a, b)| (*a..=*b).contains(&(i as u32 + 1)))
    };
    let (dir, file) = path.rsplit_once('/').unwrap_or(("", path));
    let stem = file.strip_suffix(".rs").unwrap_or(file);
    let home = if matches!(file, "mod.rs" | "lib.rs" | "main.rs") {
        dir.to_string()
    } else if dir.is_empty() {
        stem.to_string()
    } else {
        format!("{dir}/{stem}")
    };
    let at = |f: &str| {
        if home.is_empty() {
            f.to_string()
        } else {
            format!("{home}/{f}")
        }
    };
    let mut out = Vec::new();
    for (i, l) in lines.iter().enumerate() {
        let name = l
            .strip_prefix("mod ")
            .and_then(|r| r.trim_end().strip_suffix(';'))
            .filter(|n| !n.is_empty() && n.chars().all(|c| c.is_alphanumeric() || c == '_'));
        let Some(name) = name.filter(|_| is_added(i)) else {
            continue;
        };
        let file_ok = [at(&format!("{name}.rs")), at(&format!("{name}/mod.rs"))]
            .iter()
            .any(|f| touched(f) && is_test_path(f));
        let mut attrs: Vec<&str> = Vec::new();
        while i > attrs.len()
            && is_added(i - attrs.len() - 1)
            && lines[i - attrs.len() - 1].trim_start().starts_with("#[")
        {
            attrs.insert(0, lines[i - attrs.len() - 1]);
        }
        if file_ok && !attrs.iter().any(|a| a.contains("path")) {
            let mut group: Vec<String> = attrs.iter().map(|a| a.to_string()).collect();
            group.push(l.to_string());
            out.push(group);
        }
    }
    out
}

/// The planted revert of `head` against `base`, in a tree of its own,
/// closed after.
pub fn run(p: &Place, tests: &dyn Tests, branch: &str, base: &str, head: &str) -> Result<Outcome> {
    // The fix is undone to where the branch left main, not to a main that
    // moved since: only the branch's own change goes.
    let base = &p.git_ok(&p.repo, &["merge-base", base, head])?;
    let paths = git::paths(p, base, head)?;
    let t = git::detached(p, "planted", branch, head)?;
    let out = (|| -> Result<Outcome> {
        let names = tests_of(p, &t.dir, base, head, &paths)?;
        if names.is_empty() {
            return Ok(Outcome::NoTests);
        }
        let (ran, tail) = tests.run(p, &t, &names)?;
        match ran {
            Ran::Passed => {}
            Ran::Failed => {
                return Ok(Outcome::BaseRed {
                    tail,
                    build_failed: false,
                })
            }
            Ran::BuildFailed => {
                return Ok(Outcome::BaseRed {
                    tail,
                    build_failed: true,
                })
            }
            Ran::Other(code) => return Err(Undecided::new(false, code, &tail).into()),
            Ran::NoneRan => return Ok(Outcome::NotFound { tests: names }),
        }
        if undo_fix(p, &t, base, &paths)?.is_empty() {
            return Ok(Outcome::Passed { tests: names });
        }
        let (ran, tail) = tests.run(p, &t, &names)?;
        Ok(match ran {
            Ran::Passed => Outcome::Passed { tests: names },
            Ran::Failed => Outcome::Failed { tests: names },
            Ran::BuildFailed => Outcome::BuildBroke { tests: names },
            Ran::NoneRan => Outcome::Unbuilt { tests: names },
            Ran::Other(code) => return Err(Undecided::new(true, code, &tail).into()),
        })
    })();
    let _ = tree::close(p, &t);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A variable of nextest's (its profile above all) or of Theseus's is not handed to the child's
    /// `cargo nextest`: under `--profile flakehunt` the child would look for that profile in a tree
    /// that has none, and exit 96.
    #[test]
    fn the_child_does_not_inherit_nextests_or_theseus_variables() {
        for k in [
            "NEXTEST_PROFILE",
            "NEXTEST",
            "NEXTEST_RUN_ID",
            "THESEUS_STATE",
        ] {
            assert!(not_for_the_child(k), "{k}");
        }
        for k in ["PATH", "HOME", "CARGO_HOME", "RUSTUP_TOOLCHAIN", "NEXT"] {
            assert!(!not_for_the_child(k), "{k}");
        }
    }

    #[test]
    fn test_paths_are_told_apart_from_the_fix() {
        for p in [
            "crates/theseus-core/src/rsi/tests_pr.rs",
            "crates/theseusd/tests/join.rs",
            "crates/theseus-core/src/check/tests.rs",
            "cockpit/src/lib/x.test.ts",
            "bench/self/test_score.py",
            "tests/fix.sh",
        ] {
            assert!(is_test_path(p), "{p}");
        }
        for p in [
            "crates/theseus-core/src/rsi/pr.rs",
            "scripts/gate.sh",
            "tests.md",
        ] {
            assert!(!is_test_path(p), "{p}");
        }
    }

    #[test]
    fn the_changed_rust_tests_are_the_ones_whose_lines_moved() {
        let text = "use x;\n\n#[test]\nfn one() {\n    assert!(true);\n}\n\n#[tokio::test]\nasync fn two() {\n    assert!(a);\n}\n";
        assert_eq!(
            rust_tests("a/tests_x.rs", text, None),
            ["rust:a/tests_x.rs::one", "rust:a/tests_x.rs::two"]
        );
        assert_eq!(
            rust_tests("a/tests_x.rs", text, Some(&[(10, 10)])),
            ["rust:a/tests_x.rs::two"]
        );
        assert!(rust_tests("a/tests_x.rs", text, Some(&[(1, 1)])).is_empty());
        assert_eq!(
            added_ranges("@@ -3,0 +10,2 @@\n+x\n+y\n@@ -9 +20 @@\n"),
            [(10, 11), (20, 20)]
        );
    }
}
