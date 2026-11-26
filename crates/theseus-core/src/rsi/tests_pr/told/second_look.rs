//! A failure that is not the change's fault (issue 20), through `self.propose_change`: an aborted
//! check runs again and is no try; a red suite whose failures are tests the change cannot reach
//! has those tests run once more; a change that breaks a test it reaches is red the first time.
//!
//! The gate is a stand-in whose behaviour a test sets in a scratch directory (`mode`, and the
//! suite's log), and which counts its runs; `cargo` is a stand-in too, first on the path, that
//! records what it was asked and answers from the scratch directory.

use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};

use super::*;
use crate::rsi::backlog::attempts;
use crate::rsi::tests_tree::{fixture_with, git, Fixture};

/// main's gate: the scratch directory `S` holds `mode` and, for a red suite, `suite.log`.
fn gate_in(s: &Path) -> String {
    let s = s.display();
    format!(
        r#"#!/bin/bash
cd "${{THESEUS_GATE_ROOT:-$(dirname "$0")/..}}"
echo x >>"{s}/gate.runs"
case "$(cat "{s}/mode")" in
  abort-once)
    if [ "$(wc -l <"{s}/gate.runs")" -lt 2 ]; then
      echo "gate: ABORTED, THE TREE MOVED (before the suite): this is NOT a test failure and no verdict on the code"
      exit 75
    fi ;;
  abort-always)
    echo "gate: ABORTED, THE TREE MOVED (before the suite): this is NOT a test failure and no verdict on the code"
    exit 75 ;;
  suite-red)
    sleep 1
    cat "{s}/suite.log"
    echo "gate: FAILED in suite"
    exit 1 ;;
esac
echo "gate: ok"
"#
    )
}

/// A `cargo` that answers `nextest run` from the scratch directory and says what it was asked.
fn cargo_in(s: &Path) -> String {
    let s = s.display();
    format!(
        r#"#!/bin/bash
echo "$@" >>"{s}/cargo.args"
cat "{s}/rerun.log"
exit "$(cat "{s}/rerun.status")"
"#
    )
}

/// What nextest prints for a suite of three that failed `test` of `binary`.
fn red_suite(binary: &str, test: &str) -> String {
    format!(
        "    Starting 3 tests across 3 binaries\n        PASS [   0.010s] a tests::ok\n        FAIL [   0.020s] {binary} {test}\n        PASS [   0.010s] c tests::fine\n------------\n     Summary [   0.050s] 3 tests run: 2 passed, 1 failed, 0 skipped\n        FAIL [   0.020s] {binary} {test}\nerror: test run failed\n"
    )
}

fn passed_again(binary: &str, test: &str) -> String {
    format!(
        "    Starting 1 test across 1 binary\n        PASS [   0.010s] {binary} {test}\n     Summary [   0.010s] 1 test run: 1 passed, 0 skipped\n"
    )
}

/// The workspace main holds: `a` at the bottom, `c` over it, `b` apart.
fn workspace(f: &Fixture) {
    let root = f.place.state.parent().unwrap().to_path_buf();
    let other = root.join("seeder");
    git(
        &root,
        &[
            "clone",
            "-q",
            f.upstream.to_str().unwrap(),
            other.to_str().unwrap(),
        ],
    );
    let put = |path: &str, text: &str| {
        let p = other.join(path);
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        std::fs::write(p, text).unwrap();
    };
    put("Cargo.toml", "[workspace]\nmembers = [\"crates/*\"]\n");
    for (name, deps) in [
        ("a", ""),
        ("b", ""),
        ("c", "[dependencies]\na = { path = \"../a\" }\n"),
    ] {
        put(
            &format!("crates/{name}/Cargo.toml"),
            &format!("[package]\nname = \"{name}\"\nversion = \"1.0.0\"\n{deps}"),
        );
        put(&format!("crates/{name}/src/lib.rs"), "\n");
    }
    git(&other, &["add", "-A"]);
    git(&other, &["commit", "-q", "-m", "a workspace"]);
    git(&other, &["push", "-q", "origin", "HEAD:refs/heads/main"]);
    git(&f.place.repo, &["fetch", "-q", "origin"]);
}

/// The scratch directory, removed with the test.
struct Scratch {
    path: PathBuf,
    _dir: tempfile::TempDir,
}

/// A rig over that workspace, the stand-ins in place, and the scratch directory.
fn scenario(
    mode: &str,
    suite: Option<String>,
    rerun: Option<(String, i32)>,
) -> (Rig, String, Scratch) {
    let dir = crate::scratch::tempdir();
    let s = dir.path().canonicalize().unwrap();
    std::fs::write(s.join("mode"), mode).unwrap();
    std::fs::write(s.join("suite.log"), suite.unwrap_or_default()).unwrap();
    let (log, status) = rerun.unwrap_or_default();
    std::fs::write(s.join("rerun.log"), log).unwrap();
    std::fs::write(s.join("rerun.status"), status.to_string()).unwrap();
    std::fs::create_dir_all(s.join("bin")).unwrap();
    let cargo = s.join("bin/cargo");
    std::fs::write(&cargo, cargo_in(&s)).unwrap();
    std::fs::set_permissions(&cargo, std::fs::Permissions::from_mode(0o755)).unwrap();
    let f = fixture_with(&gate_in(&s));
    workspace(&f);
    let (r, sid) = rig_with_task_on(rig_on(f, SelfMode::Act, Some("origin"), |_| {}));
    let path = format!(
        "{}:{}",
        s.join("bin").display(),
        std::env::var("PATH").unwrap_or_default()
    );
    let mut env = hermetic();
    env.push(("PATH".into(), path.into()));
    r.core.tools.self_pr.set_env(env);
    (r, sid, Scratch { path: s, _dir: dir })
}

fn runs(s: &Scratch, file: &str) -> usize {
    std::fs::read_to_string(s.path.join(file)).map_or(0, |t| t.lines().count())
}

/// KEY: the suite failed on a test of a package the change cannot reach; that test alone is run
/// once more and passes, so the build goes on as green, and the row names the test.
#[tokio::test]
async fn a_failure_outside_what_the_change_reaches_is_run_once_more_and_clears() {
    let (r, sid, s) = scenario(
        "suite-red",
        Some(red_suite("b", "job::tests::flaky")),
        Some((passed_again("b", "job::tests::flaky"), 0)),
    );
    r.open_and_edit(&sid, "crates/a/src/lib.rs").await;
    let said = r.turn(&sid, WIRE, propose()).await;
    assert!(said.contains("the gate is green"), "{said}");
    assert_eq!(runs(&s, "gate.runs"), 1);
    let asked = std::fs::read_to_string(s.path.join("cargo.args")).unwrap();
    assert!(
        asked.contains("nextest run")
            && asked.contains("(binary_id(=b) & test(=job::tests::flaky))"),
        "{asked}"
    );
    let row = &r.rows()[0];
    assert_eq!(row["gate"], "green", "{row}");
    assert_eq!(row["numbers"]["reran"], json!(["b job::tests::flaky"]));
    let report = std::fs::read_to_string(row["report"].as_str().unwrap()).unwrap();
    assert!(report.contains("passed when run again alone"), "{report}");
    assert!(report.contains("b job::tests::flaky"), "{report}");
    assert!(
        !upstream_refs(&r.f).contains("refs/heads/self/"),
        "kept here (issue 53)"
    );
    assert!(attempts::tries(&task_of(&r), 0).is_empty());
}

/// KEY: a test the change reaches that fails is red the first time, and is never run again.
#[tokio::test]
async fn a_failure_inside_what_the_change_reaches_is_red_the_first_time() {
    let (r, sid, s) = scenario(
        "suite-red",
        Some(red_suite("c", "tests::broken_by_a")),
        Some((passed_again("c", "tests::broken_by_a"), 0)),
    );
    r.open_and_edit(&sid, "crates/a/src/lib.rs").await;
    let said = r.turn(&sid, WIRE, propose()).await;
    assert!(said.contains("the gate is red"), "{said}");
    assert_eq!(runs(&s, "cargo.args"), 0, "nothing was run again");
    assert!(r.rows()[0]["numbers"]["reran"].is_null());
    assert_eq!(attempts::tries(&task_of(&r), 0).len(), 1, "it is a try");
}

/// The package the change edits is inside, whatever the failing test is called.
#[tokio::test]
async fn a_failure_in_the_package_the_change_edits_is_red_the_first_time() {
    let (r, sid, s) = scenario(
        "suite-red",
        Some(red_suite("a", "tests::mine")),
        Some((passed_again("a", "tests::mine"), 0)),
    );
    r.open_and_edit(&sid, "crates/a/src/lib.rs").await;
    let said = r.turn(&sid, WIRE, propose()).await;
    assert!(said.contains("the gate is red"), "{said}");
    assert_eq!(runs(&s, "cargo.args"), 0);
}

/// A change in no package (here a document) reaches everything: nothing is outside.
#[tokio::test]
async fn a_change_in_no_package_reaches_everything() {
    let (r, sid, s) = scenario(
        "suite-red",
        Some(red_suite("b", "job::tests::flaky")),
        Some((passed_again("b", "job::tests::flaky"), 0)),
    );
    r.open_and_edit(&sid, "docs/note.md").await;
    let said = r.turn(&sid, WIRE, propose()).await;
    assert!(said.contains("the gate is red"), "{said}");
    assert_eq!(runs(&s, "cargo.args"), 0);
}

/// A test that fails again is red, after the one second run.
#[tokio::test]
async fn an_outside_test_that_fails_again_is_red() {
    let failing = "    Starting 1 test across 1 binary\n        FAIL [ 0.1s] b job::tests::flaky\n     Summary [ 0.1s] 1 test run: 0 passed, 1 failed, 0 skipped\n".to_string();
    let (r, sid, s) = scenario(
        "suite-red",
        Some(red_suite("b", "job::tests::flaky")),
        Some((failing, 100)),
    );
    r.open_and_edit(&sid, "crates/a/src/lib.rs").await;
    let said = r.turn(&sid, WIRE, propose()).await;
    assert!(said.contains("the gate is red"), "{said}");
    assert_eq!(runs(&s, "cargo.args"), 1, "once, and no more");
    assert!(r.rows()[0]["numbers"]["reran"].is_null());
}

/// KEY: an aborted check is run again, once it ends the build is judged as any, and nothing of
/// the abort is a try.
#[tokio::test]
async fn an_aborted_check_runs_again_and_is_no_try() {
    let (r, sid, s) = scenario("abort-once", None, None);
    r.open_and_edit(&sid, "crates/a/src/lib.rs").await;
    let said = r.turn(&sid, WIRE, propose()).await;
    assert!(said.contains("the gate is green"), "{said}");
    assert_eq!(runs(&s, "gate.runs"), 2, "run again after the abort");
    let t = task_of(&r);
    assert!(
        !t.evidence.iter().any(|e| e.identity == "gate:red"),
        "the abort left no red: {:?}",
        t.evidence
    );
    assert!(attempts::tries(&t, 0).is_empty());
}

/// KEY: a check that keeps aborting is run again twice, no more, ends the build with its own
/// words, and is a red of that item: one try, the reason in the task's evidence, the report and
/// the row. The re-runs inside the bound cost nothing.
#[tokio::test]
async fn a_check_that_keeps_aborting_is_bounded_says_so_and_is_a_red_of_its_item() {
    let (r, sid, s) = scenario("abort-always", None, None);
    r.open_and_edit(&sid, "crates/a/src/lib.rs").await;
    let said = r.turn(&sid, WIRE, propose()).await;
    assert_eq!(runs(&s, "gate.runs"), 3, "the first run and two more");
    assert!(said.contains("kept aborting"), "{said}");
    let t = task_of(&r);
    let tries = attempts::tries(&t, 0);
    assert_eq!(tries.len(), 1, "one try, not three: {:?}", t.evidence);
    assert!(
        tries[0].why.contains("its check kept aborting"),
        "{tries:?}"
    );
    assert!(
        !t.evidence.iter().any(|e| e.identity == "gate:unjudged"),
        "it is judged, a red of its item"
    );
    let note = t
        .evidence
        .iter()
        .find(|e| e.identity == "gate:red")
        .and_then(|e| e.note.clone())
        .unwrap();
    assert!(note.contains("kept aborting"), "{note}");
    let row = &r.rows()[0];
    assert_eq!(
        (&row["numbers"]["exit"], &row["gate"]),
        (&json!(75), &json!("red"))
    );
    assert_eq!(row["numbers"]["kept_aborting"], true, "{row}");
    let report = std::fs::read_to_string(row["report"].as_str().unwrap()).unwrap();
    assert!(report.contains("the check kept aborting"), "{report}");
}
