//! scripts/size-step.sh, the gate's size phase (theseus-2m7jg): it skips with a line when no release build of the tree
//! under test exists, and otherwise fails on growth over the last recorded size, never on §9's 60 MB.

use std::path::{Path, PathBuf};
use std::process::{Command, Output};

fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn stamp() -> String {
    let out = Command::new(root().join("scripts/tree-stamp.sh"))
        .output()
        .unwrap();
    assert!(out.status.success());
    String::from_utf8(out.stdout).unwrap().trim().to_string()
}

/// A release directory with a `theseusd` of `bytes` (sparse), stamped for this tree unless `stamped` says another.
fn release(target: &Path, bytes: u64, stamped: Option<&str>) {
    let dir = target.join("release");
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::File::create(dir.join("theseusd"))
        .unwrap()
        .set_len(bytes)
        .unwrap();
    let tree = stamped.map_or_else(stamp, str::to_string);
    std::fs::write(
        dir.join(".theseus-build-stamp"),
        format!("{tree} {bytes}\n"),
    )
    .unwrap();
}

fn step(target: &Path, history: &Path) -> Output {
    step_as(target, history, None)
}

/// `step`, judging the tree as `tree` instead of reading its stamp now: the gate's suite runs beside this test and
/// may touch the working tree (the protocol generator rewrites its files) between the stamp `release` wrote and the
/// one the step reads.
fn step_as(target: &Path, history: &Path, tree: Option<&str>) -> Output {
    let mut c = Command::new(root().join("scripts/size-step.sh"));
    c.arg(history)
        .arg("lane abc123")
        .env("THESEUS_SIZE_TARGET", target)
        .env("THESEUS_SIM", env!("CARGO_BIN_EXE_theseus-sim"));
    if let Some(t) = tree {
        c.env("THESEUS_SIZE_STAMP", t);
    }
    c.output().unwrap()
}

fn text(o: &Output) -> String {
    String::from_utf8_lossy(&o.stdout).into_owned()
}

#[test]
fn no_release_build_of_this_tree_is_a_skip_that_says_so() {
    let d = tempfile::tempdir().unwrap();
    let history = d.path().join("h.csv");
    let none = step(d.path(), &history);
    assert!(none.status.success());
    assert!(
        text(&none).contains("size: skipped: no release build of"),
        "{}",
        text(&none)
    );
    // A build of another tree is not a build of this one.
    release(d.path(), 10_000_000, Some("0000 deadbeef"));
    let other = step(d.path(), &history);
    assert!(other.status.success());
    assert!(text(&other).contains("size: skipped: no release build of"));
    assert!(!history.exists(), "a skip records nothing");
}

#[test]
fn growth_over_a_megabyte_fails_and_the_sixty_megabyte_miss_does_not() {
    let d = tempfile::tempdir().unwrap();
    let history = d.path().join("h.csv");
    // One stamp for the whole test: the tree as it was when the test began.
    let tree = stamp();
    let step = |target: &Path, history: &Path| step_as(target, history, Some(&tree));
    let release = |target: &Path, bytes: u64, _: Option<&str>| release(target, bytes, Some(&tree));
    // The first run has no baseline: over 60 MB, a miss with its gap, and a pass.
    release(d.path(), 77_000_000, None);
    let first = step(d.path(), &history);
    let out = text(&first);
    assert!(first.status.success(), "{out}");
    assert!(
        out.contains("17.00 MB over") && out.contains("not a failure"),
        "{out}"
    );
    // Half a megabyte more passes; one and a half over the baseline fails.
    release(d.path(), 77_500_000, None);
    let small = step(d.path(), &history);
    assert!(small.status.success(), "{}", text(&small));
    release(d.path(), 79_000_000, None);
    let big = step(d.path(), &history);
    assert!(!big.status.success(), "{}", text(&big));
    assert!(text(&big).contains("GREW: FAILED"), "{}", text(&big));
    // The failed run is not the baseline: the same binary fails again.
    let again = step(d.path(), &history);
    assert!(!again.status.success(), "{}", text(&again));
}
