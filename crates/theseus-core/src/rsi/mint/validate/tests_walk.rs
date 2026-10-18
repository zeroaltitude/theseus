//! The walker on the owner's example flow, in real temp git repositories
//! (`fixtures::GitFixtures`), one for each way it ends: the same table the
//! real flow's tests (`flow/tests_run.rs`) hold `flow::run` to.

use serde_json::json;

use super::fixtures::{Fixtures, GitFixtures};
use super::tests_dry::{block_on, Local};
use super::walk::walk;
use crate::flow::spec::Spec;

pub(crate) fn land_flow() -> Spec {
    Spec::parse(
        json!({
          "v": 1, "kind": "flow", "name": "repo_land",
          "scope": { "repos": ["r:5f1c0a"], "places": ["cli", "task"] },
          "inputs": { "branch": {"type": "ref"},
                      "test": {"type": "argv", "default": ["./pytest"]} },
          "steps": [
            {"id": "test", "tool": "proc.run", "args": {"argv": "$test"}, "on": {"err": "exit:tests-red"}},
            {"id": "merge", "tool": "proc.run", "args": {"argv": ["git", "merge", "--no-ff", "--no-commit", "$branch"]},
             "on": {"conflict": "goto:rerere"}},
            {"id": "rerere", "tool": "proc.run", "args": {"argv": ["git", "rerere", "status"]},
             "on": {"rerere-left": "handback:conflicts-left", "ok": "goto:add"}},
            {"id": "add", "tool": "proc.run", "args": {"argv": ["git", "add", "-A"]}},
            {"id": "commit", "tool": "proc.run", "args": {"argv": ["git", "commit", "--no-edit"]}},
            {"id": "retest", "tool": "proc.run", "args": {"argv": "$test"}, "on": {"err": "exit:tests-red"}},
            {"id": "push", "tool": "proc.run", "args": {"argv": ["git", "push"]}}
          ],
          "exits": { "ok": "landed $branch",
                     "handback:conflicts-left": {"state": ["conflict_files", "rerere_resolved"]} }
        })
        .to_string()
        .as_bytes(),
    )
    .unwrap()
}

/// The fixture for an arm, and the walk of the flow over it.
fn land(on: &str, expect: &str) -> super::walk::Walk {
    let spec = land_flow();
    let dir = tempfile::tempdir().unwrap();
    let fx = GitFixtures
        .make(on, expect, &spec, dir.path())
        .unwrap_or_else(|| panic!("no fixture for {on}"));
    let mut steps = Local::new(&fx.dir);
    block_on(walk(&spec, &fx.inputs, &mut steps)).unwrap()
}

#[test]
fn a_clean_land_runs_every_step_and_exits_ok() {
    let w = land("ok", "ok");
    assert_eq!(w.exit, "ok");
    let ids: Vec<&str> = w.ran.iter().map(|r| r.id.as_str()).collect();
    assert_eq!(
        ids,
        ["test", "merge", "rerere", "add", "commit", "retest", "push"],
        "{w:?}"
    );
    assert!(w.text.contains("landed feature"));
}

#[test]
fn a_conflict_rerere_knows_is_resolved_and_the_land_goes_on() {
    let w = land("err:1+conflict+rerere-resolved", "ok");
    assert_eq!(w.exit, "ok", "{w:?}");
    assert!(w.ran.iter().any(|r| r.id == "commit"));
}

#[test]
fn a_conflict_it_does_not_know_hands_back_with_the_files() {
    let w = land("err:1+conflict+rerere-left", "handback:conflicts-left");
    assert_eq!(w.exit, "handback:conflicts-left", "{w:?}");
    assert!(w.text.contains("a.txt"), "{}", w.text);
    assert!(
        !w.ran.iter().any(|r| r.id == "commit"),
        "it never committed: {w:?}"
    );
}

#[test]
fn red_tests_stop_with_tests_red() {
    let w = land("err:1+test-fail", "tests-red");
    assert_eq!(w.exit, "tests-red", "{w:?}");
    assert_eq!(w.ran.len(), 1, "it stopped at the first test run");
}

#[test]
fn an_input_that_does_not_resolve_does_not_start_the_walk() {
    let spec = land_flow();
    let dir = tempfile::tempdir().unwrap();
    let mut steps = Local::new(dir.path());
    let e = block_on(walk(&spec, &json!({}), &mut steps)).unwrap_err();
    assert!(e.contains("branch"), "{e}");
    let e = block_on(walk(&spec, &json!({"branch": "-x"}), &mut steps)).unwrap_err();
    assert!(e.contains("option"), "{e}");
}

#[test]
fn a_step_no_class_maps_that_failed_hands_back_step_failed() {
    let spec = land_flow();
    let dir = tempfile::tempdir().unwrap();
    // No repository: `git merge` fails with a class the flow maps nowhere.
    std::fs::write(dir.path().join("pytest"), "#!/bin/sh\nexit 0\n").unwrap();
    std::os::unix::fs::PermissionsExt::set_mode(
        &mut std::fs::metadata(dir.path().join("pytest"))
            .unwrap()
            .permissions(),
        0o755,
    );
    std::fs::set_permissions(
        dir.path().join("pytest"),
        std::os::unix::fs::PermissionsExt::from_mode(0o755),
    )
    .unwrap();
    let mut steps = Local::new(dir.path());
    let w = block_on(walk(&spec, &json!({"branch": "feature"}), &mut steps)).unwrap();
    assert_eq!(w.exit, "handback:step-failed", "{w:?}");
    assert_eq!(w.ran.last().map(|r| r.id.as_str()), Some("merge"));
}

/// The fixture tier over the flow the miner's own skeleton makes for the
/// owner's example: one fixture for each of its three arms.
#[test]
fn the_fixture_tier_runs_one_fixture_for_each_arm_of_the_mined_flow() {
    use super::fixtures::{arms_of, run_fixtures};
    let m = crate::rsi::mint::tests::land();
    let sk = crate::rsi::mint::spec::flow_spec(&m, "land_it", &|s| s.to_string()).unwrap();
    let base = tempfile::tempdir().unwrap();
    let arms = arms_of(&m);
    assert_eq!(arms.len(), 3, "{arms:?}");
    let r = block_on(run_fixtures(
        &arms,
        &sk.spec,
        &GitFixtures,
        base.path(),
        Local::new,
    ));
    assert!(r.pass, "{r:?}");
    assert_eq!((r.arms, r.passed), (3, 3));
}

#[test]
fn an_arm_no_fixture_builds_fails_the_tier_by_name() {
    use super::fixtures::{run_fixtures, Fixture};
    struct None_;
    impl Fixtures for None_ {
        fn make(&self, _: &str, _: &str, _: &Spec, _: &std::path::Path) -> Option<Fixture> {
            None
        }
    }
    let spec = land_flow();
    let base = tempfile::tempdir().unwrap();
    let arms = vec![("err:1+conflict".to_string(), "ok".to_string())];
    let r = block_on(run_fixtures(&arms, &spec, &None_, base.path(), |d| {
        Local::new(d)
    }));
    assert!(
        !r.pass && r.failed[0].contains("err:1+conflict") && r.failed[0].contains("no fixture"),
        "{r:?}"
    );
}
