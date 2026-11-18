//! A failure that is not the change's fault (issue 20).
//!
//! Two things used to cost an item a try that the change had not earned, twice each in sixteen
//! builds. The owner's decision (2026-11-15, message 1571651038392357007, "A"): an aborted check
//! is not a try and runs again, and when the only failures are tests the change did not touch,
//! those tests are run once more before the build is called red. Both live here, in the
//! pipeline's reading of a verdict, and neither changes `scripts/gate.sh` or the nextest config.
//!
//! **An aborted check** (exit [`tree::GATE_ABORTED`]: the tree moved while it ran, which the
//! gate itself calls "NOT a test failure and no verdict on the code") is run again at once, up
//! to [`ABORT_RERUNS`] more times: two aborts in a row are a pattern, a third would be a loop
//! with nothing learnt. Past that the build ends with the check's own words, "the check kept
//! aborting", which is a **red of that item**: it costs the item one try, as a red does, and
//! `infra::judge` reads it as red, so it is no part of the streak that halts self builds (an
//! abort is about one working copy, something wrote in it while its gate ran, not about the
//! machine). The re-runs inside the bound are free.
//!
//! **A red suite's failures, once more**, when and only when all of this holds:
//!
//! - the gate failed in its suite phase and nowhere else (a build, clippy or keel failure is the
//!   code's, and the phases after a failed suite did not run, so the join's own gate on the merge
//!   is what finally judges them);
//! - the suite's log reads whole: one summary, every failed test named by a `FAIL` line, the count
//!   of those lines the summary's, and no timeout, signal or leak line;
//! - **every failed test is outside what the change reaches** (`reach`: the workspace's own
//!   dependency graph, read from the manifests, and inside a package the change edits, the test's
//!   own module and what it is named for, conservatively: when in doubt it is inside);
//! - those tests, alone, pass when run again (`tree::rerun_tests`), all of them.
//!
//! Then the build goes on as green, and the row says which tests were run again, so a flaky test
//! is shown and not hidden. Anything else is red as today, on the first time: a failed test the
//! change can reach is never run again.
//!
//! The log is the branch's own output, so it is read as untrusted: a test can print any line. A
//! forged `FAIL` line can only add a name, and a surplus name fails the count against the real
//! summary; a test the change reaches that really failed always has its own real line. Names are
//! checked against a plain alphabet before they go into a filter. A change written to game this
//! reading is still met by the join's gate on the merge, which runs the whole suite again.
//!
//! **The join's gate gets the same second look** (issue 20: a test of main's that fails by chance
//! held a good change for the owner), by the same function, with one difference in what it reads:
//! the join's tree is a merge, so the change is what the merge brings to the main it was made on
//! (`base`), not a diff against a ref. Same tree, same view, same rules above, and a gate that
//! passes this way is recorded as having passed on a second look. A keel stop, a build or clippy
//! failure and an aborted check are not given one.

use std::path::Path;

use super::reach;
use super::tree::{self, GateRun, Place, Tree, Verdict};

/// How many more times an aborted check is run: three runs in all.
pub const ABORT_RERUNS: u32 = 2;

/// One failed test, as nextest names it: its binary id and its path.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct Failed {
    pub binary: String,
    pub test: String,
}

impl Failed {
    /// The package: a binary id is the package's name, then `::` and the test target's.
    pub fn package(&self) -> &str {
        self.binary.split("::").next().unwrap_or_default()
    }

    /// How a row names it.
    pub fn name(&self) -> String {
        format!("{} {}", self.binary, self.test)
    }
}

/// A name nextest prints, in the alphabet of Rust paths and target names: nothing else goes into
/// a filter.
fn plain(s: &str) -> bool {
    !s.is_empty()
        && s.chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | ':' | '-' | '.' | '/'))
}

/// The counts of a nextest summary line (`Summary [ 4.1s] 213 tests run: 212 passed (1 slow), 1
/// failed, 0 skipped`): how many ran, passed and failed. None when the line says anything else
/// (a timeout, a leak): this reads only the plainest.
fn summary(line: &str) -> Option<(u64, u64, u64)> {
    let rest = line.trim().strip_prefix("Summary [")?;
    let (_, rest) = rest.split_once(']')?;
    let (ran, counts) = rest.trim().split_once(':')?;
    let ran: u64 = ran.split_whitespace().next()?.parse().ok()?;
    let (mut passed, mut failed) = (0, 0);
    for part in counts.split(',') {
        let part = part.split('(').next()?.trim();
        let (n, what) = part.split_once(' ')?;
        let n: u64 = n.parse().ok()?;
        match what.trim() {
            "passed" => passed = n,
            "failed" => failed = n,
            "skipped" => {}
            _ => return None,
        }
    }
    Some((ran, passed, failed))
}

/// The last run of the log: from its last `Starting N tests across` banner, so a phase before the
/// suite that ran nextest (the reader rule's) is not read as it.
fn last_run(log: &str) -> &str {
    let (mut at, mut pos) = (0, 0);
    for line in log.split_inclusive('\n') {
        let t = line.trim_start();
        if t.starts_with("Starting ") && t.contains(" tests across ") {
            at = pos;
        }
        pos += line.len();
    }
    &log[at..]
}

/// Whether a line is a status nextest gives a test that this does not read as a plain failure.
fn odd(line: &str) -> bool {
    let word = line.split_whitespace().next().unwrap_or_default();
    word == "TIMEOUT" || word == "EXECFAIL" || word.starts_with("SIG") || word.starts_with("LEAK")
}

/// The failed tests of a suite's log, when the log can be read whole: at least one, the count of
/// distinct `FAIL` lines the summary's `failed`, nothing else in the summary and no odd status.
pub fn suite_failures(log: &str) -> Option<Vec<Failed>> {
    let run = last_run(log);
    let counts = run.lines().rev().find_map(|l| summary(l.trim_start()))?;
    // Only the last summary is the run's: a second would be another run.
    if run
        .lines()
        .filter(|l| l.trim_start().starts_with("Summary ["))
        .count()
        != 1
    {
        return None;
    }
    let mut failed: Vec<Failed> = Vec::new();
    for line in run.lines().map(str::trim) {
        if odd(line) {
            return None;
        }
        if !line.starts_with("FAIL [") {
            continue;
        }
        let mut words = line.split_whitespace().rev();
        let (test, binary) = (words.next()?, words.next()?);
        if !plain(test) || !plain(binary) {
            return None;
        }
        let f = Failed {
            binary: binary.to_string(),
            test: test.to_string(),
        };
        if !failed.contains(&f) {
            failed.push(f);
        }
    }
    (counts.2 > 0 && failed.len() as u64 == counts.2).then(|| {
        failed.sort();
        failed
    })
}

/// A second run's log, read: every one of `tests` ran and passed, and nothing else did.
pub fn all_passed(log: &str, tests: &[Failed]) -> bool {
    let run = last_run(log);
    let counts = run.lines().rev().find_map(|l| summary(l.trim_start()));
    !run.lines().any(|l| odd(l.trim()))
        && counts == Some((tests.len() as u64, tests.len() as u64, 0))
}

/// The nextest filter for exactly `tests`.
pub fn filter(tests: &[Failed]) -> String {
    tests
        .iter()
        .map(|t| format!("(binary_id(={}) & test(={}))", t.binary, t.test))
        .collect::<Vec<_>>()
        .join(" | ")
}

/// The tests a red gate failed only in, when the gate went red in its suite and in nothing else.
pub fn failed_in_suite(run: &GateRun) -> Option<Vec<Failed>> {
    let phase = super::pr::failed_phase(&run.tail)?;
    if run.verdict != Verdict::Red
        || run.aborted()
        || !matches!(phase.as_str(), "suite" | "suite filtered")
    {
        return None;
    }
    suite_failures(&std::fs::read_to_string(&run.log).ok()?)
}

/// Whether every failed test is outside what the change reaches. Blocking: git and the disk. The
/// change is read against `base` when one is given (the join's merge, made on that main), else
/// against `origin/main` from the merge base (a build's own branch).
fn all_outside(p: &Place, tree: &Tree, failed: &[Failed], base: Option<&str>) -> Option<bool> {
    let changed = match base {
        Some(base) => reach::changed_from(p, tree, base),
        None => reach::changed(p, tree),
    }
    .ok()?;
    let graph = reach::read(&tree.dir).ok()?;
    Some(
        failed
            .iter()
            .all(|f| reach::apart(&graph, &changed, f.package(), &f.test, &tree.dir)),
    )
}

/// The tests a red gate failed on that are run once more and all pass: those, for the row. None
/// when the red stands: any other cause, a failed test the change can reach or that cannot be
/// placed, a test that fails again, or a second run that could not be made. `base` is the commit
/// a merge tree was made on, for the join's gate; a build passes none.
pub async fn second_look(
    p: &Place,
    tree: &Tree,
    run: &GateRun,
    base: Option<&str>,
) -> Option<Vec<Failed>> {
    let failed = failed_in_suite(run)?;
    let (pc, tc, fc, bc) = (
        p.clone(),
        tree.clone(),
        failed.clone(),
        base.map(str::to_string),
    );
    let outside = tokio::task::spawn_blocking(move || all_outside(&pc, &tc, &fc, bc.as_deref()))
        .await
        .ok()??;
    if !outside {
        return None;
    }
    tracing::info!(branch = %tree.branch, tests = failed.len(),
        "self: the suite's failures are outside what the change reaches; running them once more");
    let (status, log) = tree::rerun_tests(p, tree, &filter(&failed)).await.ok()?;
    let text = read(&log)?;
    (status == Some(0) && all_passed(&text, &failed)).then_some(failed)
}

fn read(log: &Path) -> Option<String> {
    std::fs::read_to_string(log).ok()
}
