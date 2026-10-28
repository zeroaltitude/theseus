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

use std::path::Path;
use std::process::{Command, Stdio};

use anyhow::Result;
use theseus_kernel::children::{self, Kind};

use super::git;
use crate::rsi::jail::{Jail, Job};
use crate::rsi::tree::{self, Place, Tree};

/// What the planted revert found.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Outcome {
    /// A test failed with the fix undone: the branch is proved.
    Failed { tests: Vec<String> },
    /// Every test passed with the fix undone: unproved.
    Passed { tests: Vec<String> },
    /// The branch has no new or changed test to run.
    NoTests,
    /// Its tests do not pass on its own head.
    BaseRed { tail: String },
}

impl Outcome {
    /// Its name, as `self.review.verdict`'s `planted_revert` says it.
    pub fn name(&self) -> &'static str {
        match self {
            Outcome::Failed { .. } => "failed",
            Outcome::Passed { .. } => "passed",
            Outcome::NoTests => "no-tests",
            Outcome::BaseRed { .. } => "base-red",
        }
    }

    pub fn proved(&self) -> bool {
        matches!(self, Outcome::Failed { .. })
    }

    /// Why it does not prove the branch, in a sentence.
    pub fn reason(&self) -> Option<String> {
        match self {
            Outcome::Failed { .. } => None,
            Outcome::Passed { tests } => Some(format!(
                "unproved: with every non-test change undone, its tests still pass ({})",
                tests.join(", ")
            )),
            Outcome::NoTests => Some(
                "unproved: the branch adds or changes no test that the planted revert can run \
                 (inline tests go with the revert; put them in a tests file)"
                    .into(),
            ),
            Outcome::BaseRed { tail } => Some(format!(
                "its tests do not pass on its own head, so nothing can be proved: {}",
                tail.lines().last().unwrap_or("").trim()
            )),
        }
    }
}

/// Runs a branch's tests in a tree: `true` when every one passed. Blocking.
pub trait Tests: Send + Sync {
    fn run(&self, p: &Place, t: &Tree, tests: &[String]) -> Result<(bool, String)>;
}

/// The workspace's tests through nextest, in the tree's own build tree, at
/// the background priority. Only Rust tests: another kind is not run here.
pub struct Cargo;

impl Tests for Cargo {
    fn run(&self, p: &Place, t: &Tree, tests: &[String]) -> Result<(bool, String)> {
        let names: Vec<&str> = tests
            .iter()
            .filter_map(|s| s.strip_prefix("rust:"))
            .filter_map(|s| s.rsplit_once("::").map(|(_, n)| n))
            .collect();
        if names.is_empty() {
            return Ok((true, "no Rust test to run".into()));
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
        .args(["--no-tests=pass", "-E", &expr])
        .current_dir(&t.dir)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
        for (k, _) in std::env::vars_os() {
            if k.to_string_lossy().starts_with("THESEUS_") {
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
        Ok((o.status.success(), tail))
    }
}

/// The same run in L1 (theseus-pw1q.21.1): the branch's tests are the
/// branch's code, and run below the floor like its gate. Its output is a
/// log under the gates, whose tail is the verdict's words.
fn in_l1(jail: &Jail, p: &Place, t: &Tree, expr: &str) -> Result<(bool, String)> {
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
        "--no-tests=pass",
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
        code == Some(0),
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
/// or removed when `base` has none. The paths undone.
pub fn undo_fix(p: &Place, t: &Tree, base: &str, paths: &[(char, String)]) -> Result<Vec<String>> {
    let mut undone = Vec::new();
    for (status, path) in paths {
        if is_test_path(path) {
            continue;
        }
        if *status == 'A' {
            let f = t.dir.join(path);
            if f.exists() {
                std::fs::remove_file(&f)?;
            }
        } else {
            p.git_ok(&t.dir, &["checkout", "-q", base, "--", path])?;
        }
        undone.push(path.clone());
    }
    Ok(undone)
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
        let (ok, tail) = tests.run(p, &t, &names)?;
        if !ok {
            return Ok(Outcome::BaseRed { tail });
        }
        if undo_fix(p, &t, base, &paths)?.is_empty() {
            return Ok(Outcome::Passed { tests: names });
        }
        let (ok, _) = tests.run(p, &t, &names)?;
        Ok(if ok {
            Outcome::Passed { tests: names }
        } else {
            Outcome::Failed { tests: names }
        })
    })();
    let _ = tree::close(p, &t);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

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
