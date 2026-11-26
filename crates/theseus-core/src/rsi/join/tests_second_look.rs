//! A failure that is not the change's fault, at the join (issue 20): a join whose gate on the
//! merge fails only tests outside what the change brings runs those tests once more, in the same
//! tree; they pass and it joins, with the row saying so, and anything else is held.
//!
//! The gate is a stand-in whose behaviour a test sets in a scratch directory (`mode`, and the
//! suite's log), counting its runs; `cargo` is a stand-in too, first on the path, that records
//! what it was asked and answers from the scratch directory.

use crate::rsi::join::tests_publish::PassYes as _;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};

use serde_json::json;
use theseus_protocol::rsi::SelfMode;

use super::tests::{git, rig_on, Rig, PROVED, SONNET};
use crate::rsi::tests_tree::{fixture_with, Fixture};

/// main's gate: the scratch directory holds `mode` and the suite's `suite.log`.
fn gate_in(s: &Path) -> String {
    let s = s.display();
    format!(
        r#"#!/bin/bash
cd "${{THESEUS_GATE_ROOT:-$(dirname "$0")/..}}"
echo x >>"{s}/gate.runs"
runs=$(wc -l <"{s}/gate.runs")
case "$(cat "{s}/mode")" in
  red-once)
    if [ "$runs" -lt 2 ]; then cat "{s}/suite.log"; echo "gate: FAILED in suite"; exit 1; fi ;;
  red-always)
    cat "{s}/suite.log"; echo "gate: FAILED in suite"; exit 1 ;;
  deny-after)
    if [ "$runs" -lt 2 ]; then cat "{s}/suite.log"; echo "gate: FAILED in suite"; exit 1; fi
    echo "gate: FAILED in deny"; exit 1 ;;
  clippy-red)
    cat "{s}/suite.log"; echo "gate: FAILED in clippy"; exit 1 ;;
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

fn failed_again(binary: &str, test: &str) -> String {
    format!(
        "    Starting 1 test across 1 binary\n        FAIL [   0.010s] {binary} {test}\n     Summary [   0.010s] 1 test run: 0 passed, 1 failed, 0 skipped\n"
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
    // `a` is the big package: modules in directories, as a core has them.
    for module in [
        "term/mod.rs",
        "term/pty.rs",
        "term/tests.rs",
        "config/mod.rs",
        "config/words.rs",
        "config/tests_answer.rs",
        "rpc/mcp_card.rs",
    ] {
        put(&format!("crates/a/src/{module}"), "// before\n");
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

impl Scratch {
    fn runs(&self, file: &str) -> usize {
        std::fs::read_to_string(self.path.join(file)).map_or(0, |t| t.lines().count())
    }
}

/// A join rig over that workspace with the stand-ins in place: the gate's `mode`, the suite's
/// log, and what a second run of the failing tests prints and exits with.
fn scenario(mode: &str, suite: String, rerun: (String, i32)) -> (Rig, Scratch) {
    let dir = crate::scratch::tempdir();
    let s = dir.path().canonicalize().unwrap();
    std::fs::write(s.join("mode"), mode).unwrap();
    std::fs::write(s.join("suite.log"), suite).unwrap();
    std::fs::write(s.join("rerun.log"), rerun.0).unwrap();
    std::fs::write(s.join("rerun.status"), rerun.1.to_string()).unwrap();
    std::fs::create_dir_all(s.join("bin")).unwrap();
    let cargo = s.join("bin/cargo");
    std::fs::write(&cargo, cargo_in(&s)).unwrap();
    std::fs::set_permissions(&cargo, std::fs::Permissions::from_mode(0o755)).unwrap();
    let f = fixture_with(&gate_in(&s));
    workspace(&f);
    let r = rig_on(f, SelfMode::Act, |_| {}, |_| {});
    let path = format!(
        "{}:{}",
        s.join("bin").display(),
        std::env::var("PATH").unwrap_or_default()
    );
    let mut env = crate::rsi::tests_tree::hermetic();
    env.push(("PATH".into(), path.into()));
    r.core.tools.self_pr.set_env(env);
    (r, Scratch { path: s, _dir: dir })
}

/// The files of a branch that edits package `a` and nothing outside it (a path in no package
/// reaches every package), with a test the planted revert can fail.
const EDITS_A: &[(&str, &str)] = &[
    ("crates/a/fix.txt", "fixed\n"),
    ("crates/a/tests/fix.sh", "test -f crates/a/fix.txt\n"),
];

/// The change of the second self join: three files of the one package, in modules that are not
/// the terminal's. The planted revert's stand-in runs a test file only as a script, which in `a`
/// would be a file that is not Rust source and would reach the whole package, so the proof sits in
/// the package beside it, which `a` does not depend on.
const EDITS_WORDS: &[(&str, &str)] = &[
    (
        "crates/b/tests/words.sh",
        "grep -q after crates/a/src/config/words.rs\n",
    ),
    ("crates/a/src/config/words.rs", "// after\n"),
    ("crates/a/src/rpc/mcp_card.rs", "// after\n"),
    ("crates/a/src/config/tests_answer.rs", "// after\n"),
];

const TERM_TEST: &str = "term::tests::cat_echoes_and_ctrl_c_ends_it";

/// KEY: the gate's only failure is a test of a package the merge does not touch. That test alone
/// is run once more and passes, and the join goes on: main's whole gate runs once more (what
/// follows the suite never ran), is green, and the join happens, with its row naming the test
/// and saying it passed on a second look. No stop is recorded.
#[tokio::test]
async fn an_unrelated_test_that_fails_once_is_run_again_and_the_change_joins() {
    let (r, s) = scenario(
        "red-once",
        red_suite("b", "job::tests::flaky"),
        (passed_again("b", "job::tests::flaky"), 0),
    );
    let (branch, _) = r.seed("trim-the-sails", EDITS_A, SONNET);
    let main = r.upstream_main();
    let pass = r.core.self_join_pass_yes(&r.seams()).await.unwrap();
    assert_eq!(pass.joined.len(), 1, "{pass:?}");
    assert_eq!(pass.joined[0].0, branch);
    assert_ne!(r.upstream_main(), main, "it joined");
    assert!(r.rows("self.join.stopped").is_empty());
    let asked = std::fs::read_to_string(s.path.join("cargo.args")).unwrap();
    assert!(
        asked.contains("nextest run")
            && asked.contains("(binary_id(=b) & test(=job::tests::flaky))"),
        "{asked}"
    );
    assert_eq!(s.runs("cargo.args"), 1, "one second look");
    assert_eq!(s.runs("gate.runs"), 2, "the gate, and the gate once more");
    let j = &r.rows("self.joined")[0];
    assert_eq!(j["numbers"]["reran"], json!(["b job::tests::flaky"]), "{j}");
    let said = j["gate"].as_str().unwrap();
    assert!(
        said.contains("passed on a second look") && said.contains("b job::tests::flaky"),
        "{said}"
    );
    // The first run's log, which the second wrote over, is kept.
    let kept: Vec<_> = std::fs::read_dir(r.f.place.gates())
        .unwrap()
        .flatten()
        .filter(|e| e.file_name().to_string_lossy().ends_with(".first.log"))
        .collect();
    assert_eq!(kept.len(), 1, "{kept:?}");
}

/// KEY: an unrelated test that fails again is red. The change is held for the owner, and the card
/// names the test; main is untouched and the gate ran once.
#[tokio::test]
async fn an_unrelated_test_that_fails_twice_holds_the_change_and_is_named() {
    let (r, s) = scenario(
        "red-always",
        red_suite("b", "job::tests::flaky"),
        (failed_again("b", "job::tests::flaky"), 100),
    );
    let (branch, _) = r.seed("trim-the-sails", EDITS_A, SONNET);
    let main = r.upstream_main();
    let pass = r.core.self_join_pass_yes(&r.seams()).await.unwrap();
    assert_eq!(pass.stopped, [(branch, "gate".to_string())], "{pass:?}");
    assert_eq!(r.upstream_main(), main);
    assert_eq!(s.runs("cargo.args"), 1, "once, and no more");
    assert_eq!(
        s.runs("gate.runs"),
        1,
        "no second gate for a test that fails twice"
    );
    let st = &r.rows("self.join.stopped")[0];
    let why = st["why"].as_str().unwrap();
    assert!(
        why.contains("1 test failed: b job::tests::flaky") && why.contains("main is untouched"),
        "{why}"
    );
    assert_eq!(st["numbers"]["held"], true, "{st}");
    assert!(r.rows("self.joined").is_empty());
}

/// KEY: a test the change touches that fails is red at once: no second look, no second gate.
#[tokio::test]
async fn a_test_the_change_touches_that_fails_is_held_at_once() {
    let (r, s) = scenario(
        "red-always",
        red_suite("c", "tests::broken_by_a"),
        (passed_again("c", "tests::broken_by_a"), 0),
    );
    let (branch, _) = r.seed("trim-the-sails", EDITS_A, SONNET);
    let main = r.upstream_main();
    let pass = r.core.self_join_pass_yes(&r.seams()).await.unwrap();
    assert_eq!(pass.stopped, [(branch, "gate".to_string())], "{pass:?}");
    assert_eq!(r.upstream_main(), main);
    assert_eq!(s.runs("cargo.args"), 0, "nothing was run again");
    assert_eq!(s.runs("gate.runs"), 1);
    let why = r.rows("self.join.stopped")[0]["why"].clone();
    assert!(
        why.as_str().unwrap().contains("c tests::broken_by_a"),
        "{why}"
    );
}

/// KEY: what follows the suite is judged. The tests pass on the second look, but the whole gate
/// that follows is red in `deny`: the change is held, and the card says what had been let go.
#[tokio::test]
async fn a_pass_on_the_second_look_does_not_skip_what_follows_the_suite() {
    let (r, s) = scenario(
        "deny-after",
        red_suite("b", "job::tests::flaky"),
        (passed_again("b", "job::tests::flaky"), 0),
    );
    let (branch, _) = r.seed("trim-the-sails", EDITS_A, SONNET);
    let main = r.upstream_main();
    let pass = r.core.self_join_pass_yes(&r.seams()).await.unwrap();
    assert_eq!(pass.stopped, [(branch, "gate".to_string())], "{pass:?}");
    assert_eq!(r.upstream_main(), main, "main is untouched");
    assert_eq!(s.runs("gate.runs"), 2);
    let why = r.rows("self.join.stopped")[0]["why"].clone();
    let why = why.as_str().unwrap();
    assert!(
        why.contains("b job::tests::flaky had failed before and passed alone"),
        "{why}"
    );
}

/// A gate that is red in a phase other than the suite is the code's: no second look.
#[tokio::test]
async fn a_red_in_another_phase_is_given_no_second_look() {
    let (r, s) = scenario(
        "clippy-red",
        red_suite("b", "job::tests::flaky"),
        (passed_again("b", "job::tests::flaky"), 0),
    );
    let (branch, _) = r.seed("trim-the-sails", EDITS_A, SONNET);
    let main = r.upstream_main();
    let pass = r.core.self_join_pass_yes(&r.seams()).await.unwrap();
    assert_eq!(pass.stopped, [(branch, "gate".to_string())], "{pass:?}");
    assert_eq!(r.upstream_main(), main);
    assert_eq!(s.runs("cargo.args"), 0);
    assert_eq!(s.runs("gate.runs"), 1);
}

/// A change with a file in no package (here at the root) reaches everything: nothing is outside,
/// so an unrelated test that fails is red at once.
#[tokio::test]
async fn a_change_in_no_package_reaches_every_test() {
    let (r, s) = scenario(
        "red-always",
        red_suite("b", "job::tests::flaky"),
        (passed_again("b", "job::tests::flaky"), 0),
    );
    let (branch, _) = r.seed("trim-the-sails", PROVED, SONNET);
    let pass = r.core.self_join_pass_yes(&r.seams()).await.unwrap();
    assert_eq!(pass.stopped, [(branch, "gate".to_string())], "{pass:?}");
    assert_eq!(s.runs("cargo.args"), 0);
    assert_eq!(s.runs("gate.runs"), 1);
}

/// The card names at most five failed tests, and counts the rest.
#[test]
fn a_hold_names_five_failed_tests_and_counts_them() {
    let dir = crate::scratch::tempdir();
    let log = dir.path().join("gate.log");
    let fails: String = (1..=7)
        .map(|n| format!("        FAIL [   0.020s] b job::tests::t{n}\n"))
        .collect();
    let text = format!(
        "    Starting 9 tests across 1 binary\n{fails}------------\n     Summary [   0.050s] 9 tests run: 2 passed, 7 failed, 0 skipped\n{fails}gate: FAILED in suite\n"
    );
    std::fs::write(&log, &text).unwrap();
    let run = crate::rsi::tree::GateRun {
        verdict: crate::rsi::tree::Verdict::Red,
        exit: Some(100),
        blob: "x".into(),
        log,
        secs: 1,
        tail: text,
        unrunnable: false,
    };
    let why = super::joining::red_said(&run, &[]);
    assert!(
        why.contains("7 tests failed, the first 5: b job::tests::t1, b job::tests::t2")
            && why.contains("b job::tests::t5")
            && !why.contains("b job::tests::t6"),
        "{why}"
    );
}

/// KEY, the case that was held on 2026-11-17: the change edits three files of the one package and
/// the only failure is a terminal test of that same package, in a module the change does not edit.
/// It is run once more alone, passes, main's whole gate runs once more, the join goes on and the
/// row names the test.
#[tokio::test]
async fn an_unrelated_test_of_the_same_package_that_fails_once_is_run_again_and_the_change_joins() {
    let (r, s) = scenario(
        "red-once",
        red_suite("a", TERM_TEST),
        (passed_again("a", TERM_TEST), 0),
    );
    let (branch, _) = r.seed("trim-the-sails", EDITS_WORDS, SONNET);
    let main = r.upstream_main();
    let pass = r.core.self_join_pass_yes(&r.seams()).await.unwrap();
    assert_eq!(pass.joined.len(), 1, "{pass:?}");
    assert_eq!(pass.joined[0].0, branch);
    assert_ne!(r.upstream_main(), main, "it joined");
    assert!(r.rows("self.join.stopped").is_empty());
    let asked = std::fs::read_to_string(s.path.join("cargo.args")).unwrap();
    assert!(
        asked.contains(&format!("(binary_id(=a) & test(={TERM_TEST}))")),
        "{asked}"
    );
    assert_eq!(s.runs("cargo.args"), 1, "one second look");
    assert_eq!(s.runs("gate.runs"), 2, "the gate, and the gate once more");
    let j = &r.rows("self.joined")[0];
    assert_eq!(
        j["numbers"]["reran"],
        json!([format!("a {TERM_TEST}")]),
        "{j}"
    );
    let said = j["gate"].as_str().unwrap();
    assert!(
        said.contains("passed on a second look") && said.contains(TERM_TEST),
        "{said}"
    );
}

/// KEY: a failed test in the module the change edits is held at once, with no second look and no
/// second gate: whether the change edits a file of its subtree or the test's own file.
#[tokio::test]
async fn a_failed_test_in_a_module_the_change_edits_is_held_at_once() {
    for edited in ["crates/a/src/term/pty.rs", "crates/a/src/term/tests.rs"] {
        let (r, s) = scenario(
            "red-always",
            red_suite("a", TERM_TEST),
            (passed_again("a", TERM_TEST), 0),
        );
        let mut files = EDITS_WORDS.to_vec();
        files.push((edited, "// after\n"));
        let (branch, _) = r.seed("trim-the-sails", &files, SONNET);
        let main = r.upstream_main();
        let pass = r.core.self_join_pass_yes(&r.seams()).await.unwrap();
        assert_eq!(
            pass.stopped,
            [(branch, "gate".to_string())],
            "{edited}: {pass:?}"
        );
        assert_eq!(r.upstream_main(), main);
        assert_eq!(s.runs("cargo.args"), 0, "{edited}: nothing was run again");
        assert_eq!(s.runs("gate.runs"), 1, "{edited}");
        let why = r.rows("self.join.stopped")[0]["why"].clone();
        assert!(why.as_str().unwrap().contains(TERM_TEST), "{why}");
    }
}

/// KEY: a change to a manifest of the package, or to a file in no package, with any failure: held
/// at once, whatever module the test is in.
#[tokio::test]
async fn a_cargo_file_or_a_script_in_the_change_gives_no_second_look() {
    for edited in ["crates/a/Cargo.toml", "scripts/helper.sh"] {
        let (r, s) = scenario(
            "red-always",
            red_suite("a", TERM_TEST),
            (passed_again("a", TERM_TEST), 0),
        );
        let mut files = EDITS_WORDS.to_vec();
        let text = match edited.ends_with("Cargo.toml") {
            true => "[package]\nname = \"a\"\nversion = \"1.0.1\"\n",
            false => "changed\n",
        };
        files.push((edited, text));
        let (branch, _) = r.seed("trim-the-sails", &files, SONNET);
        let main = r.upstream_main();
        let pass = r.core.self_join_pass_yes(&r.seams()).await.unwrap();
        assert_eq!(
            pass.stopped,
            [(branch, "gate".to_string())],
            "{edited}: {pass:?}"
        );
        assert_eq!(r.upstream_main(), main);
        assert_eq!(s.runs("cargo.args"), 0, "{edited}");
        assert_eq!(s.runs("gate.runs"), 1, "{edited}");
    }
}

/// KEY: a test of the same package that fails again on its second look is held, and the gate is
/// not run once more.
#[tokio::test]
async fn an_unrelated_test_of_the_same_package_that_fails_twice_holds_the_change() {
    let (r, s) = scenario(
        "red-always",
        red_suite("a", TERM_TEST),
        (failed_again("a", TERM_TEST), 100),
    );
    let (branch, _) = r.seed("trim-the-sails", EDITS_WORDS, SONNET);
    let main = r.upstream_main();
    let pass = r.core.self_join_pass_yes(&r.seams()).await.unwrap();
    assert_eq!(pass.stopped, [(branch, "gate".to_string())], "{pass:?}");
    assert_eq!(r.upstream_main(), main);
    assert_eq!(s.runs("cargo.args"), 1, "once, and no more");
    assert_eq!(s.runs("gate.runs"), 1);
    let why = r.rows("self.join.stopped")[0]["why"].clone();
    assert!(why.as_str().unwrap().contains(TERM_TEST), "{why}");
}

/// KEY: a test binary of its own, or of a program, is judged by package: its tests can use any
/// module of the library from outside. With the package edited, a failure there is held at once
/// (the failed test's id carries its binary, and only a unit test of the library is placed by
/// module).
#[tokio::test]
async fn a_failed_test_of_another_binary_of_an_edited_package_is_held_at_once() {
    let (r, s) = scenario(
        "red-always",
        red_suite("a::integration", TERM_TEST),
        (passed_again("a::integration", TERM_TEST), 0),
    );
    let (branch, _) = r.seed("trim-the-sails", EDITS_WORDS, SONNET);
    let main = r.upstream_main();
    let pass = r.core.self_join_pass_yes(&r.seams()).await.unwrap();
    assert_eq!(pass.stopped, [(branch, "gate".to_string())], "{pass:?}");
    assert_eq!(r.upstream_main(), main);
    assert_eq!(s.runs("cargo.args"), 0, "nothing was run again");
    assert_eq!(s.runs("gate.runs"), 1);
}
