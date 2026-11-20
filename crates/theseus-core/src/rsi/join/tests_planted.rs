//! The planted revert's proof step (theseus-pw1q.9) on throwaway repositories: what the undo keeps
//! (the lines that only make a test file part of the build), how a run in which no test ran
//! is told from one that passed or failed, and how a failing test, a failing build and any other
//! end of the second run are told apart.

use std::collections::VecDeque;
use std::sync::Mutex;

use super::git;
use super::planted::{self, Outcome, Ran, Tests};
use super::tests::git as run_git;
use crate::rsi::tests_tree::{fixture, Fixture};
use crate::rsi::tree::{Place, Tree};

const TEST_FILE: &str = "src/backlog/tests_judge.rs";
const BASE_BACKLOG: &str = "mod read;\npub mod other;\n";
const HEAD_BACKLOG: &str = "mod read;\n#[cfg(test)]\nmod tests_judge;\npub mod other;\n";

/// A base commit with `base` files and a head on it with `head` files, in a throwaway worktree of
/// the fixture's repo; the two commits.
pub(super) fn commits(
    f: &Fixture,
    base: &[(&str, &str)],
    head: &[(&str, &str)],
) -> (String, String) {
    let repo = &f.place.repo;
    let wt = repo.parent().unwrap().join("seed-planted");
    run_git(
        repo,
        &[
            "worktree",
            "add",
            "-q",
            "--detach",
            wt.to_str().unwrap(),
            "origin/main",
        ],
    );
    let commit = |files: &[(&str, &str)], what: &str| {
        for (path, text) in files {
            let p = wt.join(path);
            std::fs::create_dir_all(p.parent().unwrap()).unwrap();
            std::fs::write(p, text).unwrap();
        }
        run_git(&wt, &["add", "-A"]);
        run_git(&wt, &["commit", "-q", "-m", what]);
        run_git(&wt, &["rev-parse", "HEAD"])
    };
    let (b, h) = (commit(base, "base"), commit(head, "head"));
    run_git(
        repo,
        &["worktree", "remove", "--force", wt.to_str().unwrap()],
    );
    (b, h)
}

/// The morning's change: a fix, a new tests file, and the lines that declare it in a non-test file.
fn morning(f: &Fixture) -> (String, String) {
    commits(
        f,
        &[
            ("src/backlog.rs", BASE_BACKLOG),
            ("src/backlog/read.rs", "pub fn f() -> u32 { 1 }\n"),
        ],
        &[
            ("src/backlog.rs", HEAD_BACKLOG),
            ("src/backlog/read.rs", "pub fn f() -> u32 { 2 }\n"),
            (TEST_FILE, "#[test]\nfn the_fix_holds() {}\n"),
        ],
    )
}

fn undone(f: &Fixture, base: &str, head: &str) -> (Tree, Vec<String>) {
    let p = &f.place;
    let paths = git::paths(p, base, head).unwrap();
    let t = git::detached(p, "planted", "self/x", head).unwrap();
    let undone = planted::undo_fix(p, &t, base, &paths).unwrap();
    (t, undone)
}

fn read(t: &Tree, path: &str) -> String {
    std::fs::read_to_string(t.dir.join(path)).unwrap()
}

/// The morning's case: the fix goes, the declaring lines stay (at the end of the file), and so
/// does the tests file, so the same tests are built again.
#[test]
fn undoing_the_fix_keeps_the_lines_that_declare_its_tests() {
    let f = fixture();
    let (base, head) = morning(&f);
    let (t, undone) = undone(&f, &base, &head);
    assert_eq!(undone, ["src/backlog.rs", "src/backlog/read.rs"]);
    assert_eq!(read(&t, "src/backlog/read.rs"), "pub fn f() -> u32 { 1 }\n");
    assert_eq!(
        read(&t, "src/backlog.rs"),
        "mod read;\npub mod other;\n#[cfg(test)]\nmod tests_judge;\n"
    );
    assert!(t.dir.join(TEST_FILE).exists());
}

/// Only a declaration of a test file the branch touched stays.
#[test]
fn only_a_declaration_of_a_touched_test_file_is_kept() {
    let touched = |p: &str| {
        [
            "src/a/tests_x.rs",
            "src/a/tests/mod.rs",
            "src/a/helper.rs",
            "src/tests_top.rs",
        ]
        .contains(&p)
    };
    let kept = |path: &str, text: &str| {
        planted::kept_declarations(path, text, &[(1, text.lines().count() as u32)], &touched)
    };
    let one = |l: &str| vec![vec![l.to_string()]];
    // Both forms Rust allows, relative to the declaring file as Rust resolves it.
    assert_eq!(kept("src/a.rs", "mod tests_x;\n"), one("mod tests_x;"));
    assert_eq!(kept("src/a.rs", "mod tests;\n"), one("mod tests;"));
    assert_eq!(kept("src/a/mod.rs", "mod tests_x;\n"), one("mod tests_x;"));
    // `a.rs` holds `a/`'s modules, so `b.rs`'s `tests_x` is `b/tests_x.rs`, which is not there.
    assert!(kept("src/b.rs", "mod tests_x;\n").is_empty());
    assert_eq!(
        kept("src/lib.rs", "mod tests_top;\n"),
        one("mod tests_top;")
    );
    // Its attributes stay with it, but a `#[path]` cannot be resolved here.
    assert_eq!(
        kept("src/a.rs", "#[cfg(test)]\nmod tests_x;\n"),
        vec![vec!["#[cfg(test)]".to_string(), "mod tests_x;".to_string()]]
    );
    assert!(kept("src/a.rs", "#[path = \"elsewhere.rs\"]\nmod tests_x;\n").is_empty());
    // Not these: a module that is not a test file, one the branch did not touch, `pub mod`, a
    // `use`, an indented one (inside a block), a body, and a line the branch did not add.
    for text in [
        "mod helper;\n",
        "mod tests_untouched;\n",
        "pub mod tests_x;\n",
        "use self::tests_x;\n",
        "mod inner {\n    mod tests_x;\n}\n",
        "mod tests_x { }\n",
    ] {
        assert!(kept("src/a.rs", text).is_empty(), "{text}");
    }
    assert!(planted::kept_declarations("src/a.rs", "mod tests_x;\n", &[], &touched).is_empty());
}

/// A `mod` line for a module that is not a test file, and a `use`, go with the fix.
#[test]
fn a_declaration_of_a_non_test_module_and_a_use_go_with_the_fix() {
    let f = fixture();
    let (base, head) = commits(
        &f,
        &[("src/backlog.rs", BASE_BACKLOG)],
        &[
            (
                "src/backlog.rs",
                "mod read;\nmod helper;\nuse crate::x;\npub mod other;\nmod tests_judge;\n",
            ),
            ("src/backlog/helper.rs", "pub fn h() {}\n"),
            (TEST_FILE, "#[test]\nfn t() {}\n"),
        ],
    );
    let (t, _) = undone(&f, &base, &head);
    assert_eq!(
        read(&t, "src/backlog.rs"),
        "mod read;\npub mod other;\nmod tests_judge;\n"
    );
}

/// A stand-in that says what each run comes to, and what the tree held at each.
struct Says {
    runs: Mutex<VecDeque<Ran>>,
    seen: Mutex<Vec<(String, String)>>,
}

impl Says {
    fn new(runs: &[Ran]) -> Self {
        Self {
            runs: Mutex::new(runs.iter().copied().collect()),
            seen: Mutex::default(),
        }
    }
}

impl Tests for Says {
    fn run(&self, _p: &Place, t: &Tree, tests: &[String]) -> anyhow::Result<(Ran, String)> {
        assert_eq!(tests, [format!("rust:{TEST_FILE}::the_fix_holds")]);
        self.seen
            .lock()
            .unwrap()
            .push((read(t, "src/backlog.rs"), read(t, "src/backlog/read.rs")));
        Ok((
            self.runs.lock().unwrap().pop_front().unwrap(),
            "tail".into(),
        ))
    }
}

fn outcome(runs: &[Ran]) -> (Outcome, Vec<(String, String)>) {
    let (o, seen) = answer(runs);
    (o.unwrap(), seen)
}

/// [`outcome`], where the run may end in an error (a stop) instead of an outcome.
fn answer(runs: &[Ran]) -> (anyhow::Result<Outcome>, Vec<(String, String)>) {
    let f = fixture();
    let (base, head) = morning(&f);
    let says = Says::new(runs);
    let o = planted::run(&f.place, &says, "self/x", &base, &head);
    let seen = says.seen.into_inner().unwrap();
    (o, seen)
}

/// The tests are run with the fix in place, then with it undone and the declaration standing:
/// the morning's case, end to end.
#[test]
fn the_second_run_is_of_the_tree_with_the_fix_undone_and_its_tests_declared() {
    let (o, seen) = outcome(&[Ran::Passed, Ran::Failed]);
    assert!(o.proved(), "{o:?}");
    assert_eq!(
        seen,
        [
            (
                HEAD_BACKLOG.to_string(),
                "pub fn f() -> u32 { 2 }\n".to_string()
            ),
            (
                "mod read;\npub mod other;\n#[cfg(test)]\nmod tests_judge;\n".to_string(),
                "pub fn f() -> u32 { 1 }\n".to_string()
            ),
        ]
    );
}

/// With the fix in place and no test run, nothing is known: rejected, in words of its own.
#[test]
fn no_test_run_on_the_head_is_a_rejection_that_says_the_tests_were_not_found() {
    let (o, seen) = outcome(&[Ran::NoneRan]);
    assert_eq!(seen.len(), 1);
    assert!(matches!(o, Outcome::NotFound { .. }), "{o:?}");
    assert!(!o.proved());
    assert_eq!(o.name(), "not-found");
    let why = o.reason().unwrap();
    assert!(why.contains("could not be found in the build"), "{why}");
    assert!(!why.contains("do not pass"), "{why}");
}

/// With the fix undone and no test run, the tests are not built without it: not "a test failed",
/// and not "its tests still pass".
#[test]
fn no_test_run_with_the_fix_undone_is_unbuilt_not_proved_and_not_passed() {
    let (o, _) = outcome(&[Ran::Passed, Ran::NoneRan]);
    assert!(matches!(o, Outcome::Unbuilt { .. }), "{o:?}");
    assert!(!o.proved());
    assert_eq!(o.name(), "unbuilt");
    let why = o.reason().unwrap();
    assert!(
        why.starts_with("unproved: with the fix undone its tests are not built"),
        "{why}"
    );
    assert!(why.contains("declare the test module"), "{why}");
    assert!(!why.contains("still pass"), "{why}");
}

/// The ordinary cases are what they were.
#[test]
fn the_ordinary_outcomes_are_unchanged() {
    let (o, _) = outcome(&[Ran::Passed, Ran::Failed]);
    assert!(matches!(o, Outcome::Failed { .. }), "{o:?}");
    assert!(o.proved());
    let (o, _) = outcome(&[Ran::Passed, Ran::Passed]);
    assert!(matches!(o, Outcome::Passed { .. }), "{o:?}");
    assert_eq!(
        o.reason().unwrap(),
        format!("unproved: with every non-test change undone, its tests still pass (rust:{TEST_FILE}::the_fix_holds)")
    );
    let (o, _) = outcome(&[Ran::Failed]);
    assert!(matches!(o, Outcome::BaseRed { .. }), "{o:?}");
}

/// nextest's exit codes: 0 passed, 4 none ran, 100 a test failed, 101 the build failed, and any
/// other (a setup error, a bad filter, a run left incomplete, a signal) is none of those.
#[test]
fn nextests_exit_codes_say_what_ran() {
    for (code, want) in [
        (Some(0), Ran::Passed),
        (Some(4), Ran::NoneRan),
        (Some(100), Ran::Failed),
        (Some(101), Ran::BuildFailed),
        (Some(94), Ran::Other(Some(94))),
        (Some(96), Ran::Other(Some(96))),
        (Some(102), Ran::Other(Some(102))),
        (Some(106), Ran::Other(Some(106))),
        (Some(1), Ran::Other(Some(1))),
        (None, Ran::Other(None)),
    ] {
        assert_eq!(Ran::of(code), want, "{code:?}");
    }
}

/// With the fix undone, a failing test and a failing build are each proof, each under its own
/// name; the test's name in the record is the one it always had.
#[test]
fn the_second_runs_failing_test_and_failing_build_are_each_proof_under_their_own_name() {
    let (o, _) = outcome(&[Ran::Passed, Ran::Failed]);
    assert!(matches!(o, Outcome::Failed { .. }), "{o:?}");
    assert_eq!(o.name(), "failed");
    let (b, _) = outcome(&[Ran::Passed, Ran::BuildFailed]);
    assert!(matches!(b, Outcome::BuildBroke { .. }), "{b:?}");
    assert_eq!(b.name(), "build-broke");
    for o in [&o, &b] {
        assert!(o.proved(), "{o:?}");
        assert_eq!(o.reason(), None);
    }
}

/// Any other end of the second run answers nothing: an error (the step's stop, which is tried
/// again), so it is neither an outcome that proves nor one that rejects, and it says what it was.
#[test]
fn any_other_end_of_the_second_run_is_a_stop_that_says_what_it_was() {
    let (o, seen) = answer(&[Ran::Passed, Ran::Other(Some(106))]);
    assert_eq!(seen.len(), 2);
    let why = format!("{:#}", o.unwrap_err());
    assert!(
        why.contains("with the fix undone exited with code 106"),
        "{why}"
    );
    assert!(
        why.contains("neither a failing test nor a failing build"),
        "{why}"
    );
    let (o, _) = answer(&[Ran::Passed, Ran::Other(None)]);
    assert!(format!("{:#}", o.unwrap_err()).contains("cut short by a signal"));
}

/// On the branch's own head a failing test and a failing build are its own fault, `BaseRed`, and
/// the words say which of the two it was.
#[test]
fn the_first_run_reads_a_failing_test_and_a_failing_build_as_the_branchs_own_fault() {
    for (ran, build_failed, words) in [
        (
            Ran::Failed,
            false,
            "do not pass on its own head (a test failed)",
        ),
        (
            Ran::BuildFailed,
            true,
            "do not build on its own head (the build failed)",
        ),
    ] {
        let (o, seen) = outcome(&[ran]);
        assert!(
            matches!(&o, Outcome::BaseRed { build_failed: b, .. } if *b == build_failed),
            "{ran:?}: {o:?}"
        );
        assert_eq!(seen.len(), 1);
        let why = o.reason().unwrap();
        assert!(why.contains(words), "{why}");
    }
}

/// On the branch's own head any other end answers nothing either: a stop that says which run and
/// what the exit was, not a rejection of the change for something that may not be its fault.
#[test]
fn any_other_end_of_the_first_run_is_a_stop_that_says_which_run_it_was() {
    for (code, words) in [
        (Some(96), "on its own head exited with code 96"),
        (None, "on its own head was cut short by a signal"),
    ] {
        let (o, seen) = answer(&[Ran::Other(code)]);
        assert_eq!(seen.len(), 1);
        let why = format!("{:#}", o.unwrap_err());
        assert!(why.contains(words), "{why}");
        assert!(!why.contains("with the fix undone"), "{why}");
    }
    let (o, _) = answer(&[Ran::Passed, Ran::Other(Some(96))]);
    let u = planted::Undecided::of(&o.unwrap_err()).cloned().unwrap();
    assert_eq!(u.class(), "second-run-exit-96");
}
