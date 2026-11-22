//! The start's sweep closes only a phase tree no pass of this daemon opened (issue 41, `guard`).

use super::git;
use super::guard::{close_dead, forget_opened_here, left_open, Dead};
use super::tests::git as run_git;
use crate::rsi::tree_lease::Lease;

/// A tree this daemon opened stays, whatever its phase; once the daemon has no record of it (a
/// restart), it is closed with its phase named. A merge's tree and an item's are not touched.
#[test]
fn only_a_phase_tree_this_daemon_did_not_open_is_closed() {
    let f = crate::rsi::tests_tree::fixture();
    let head = run_git(&f.place.repo, &["rev-parse", "origin/main"]);
    let planted = git::detached(&f.place, "planted", "trim", &head).unwrap();
    std::fs::create_dir_all(f.place.trees().join("join-trim")).unwrap();
    std::fs::create_dir_all(f.place.trees().join("tsk-c")).unwrap();
    assert_eq!(
        close_dead(&f.place),
        Vec::<Dead>::new(),
        "a live pass's tree"
    );
    assert!(planted.dir.exists());
    forget_opened_here();
    assert_eq!(
        close_dead(&f.place),
        vec![Dead {
            tree: "planted-trim".into(),
            phase: "planted".into(),
            closed: Ok(()),
        }]
    );
    assert!(!planted.dir.exists());
    assert!(
        !planted.target.exists(),
        "a planted tree's build tree goes with it"
    );
    assert_eq!(left_open(&f.place.trees(), true), ["join-trim"]);
    assert!(
        f.place.trees().join("tsk-c").exists(),
        "an item's tree is the pipeline's"
    );
}

/// A directory git does not know (a tree half made, or its entry pruned) is removed too; one a
/// gate holds is left, with the reason.
#[test]
fn a_tree_git_does_not_know_is_removed_and_one_a_gate_holds_is_left() {
    let f = crate::rsi::tests_tree::fixture();
    let trees = f.place.trees();
    std::fs::create_dir_all(trees.join("keel-trim/src")).unwrap();
    std::fs::create_dir_all(trees.join("probe-0123456789ab")).unwrap();
    let held = std::fs::canonicalize(trees.join("probe-0123456789ab")).unwrap();
    let _gate = Lease::take(&[&held]);
    forget_opened_here();
    let dead = close_dead(&f.place);
    assert_eq!(dead.len(), 2, "{dead:?}");
    assert_eq!(
        dead[0],
        Dead {
            tree: "keel-trim".into(),
            phase: "keel".into(),
            closed: Ok(()),
        }
    );
    assert_eq!(dead[1].tree, "probe-0123456789ab");
    let why = dead[1].closed.clone().unwrap_err();
    assert!(why.contains("a gate is still running"), "{why}");
    assert!(held.exists());
    assert!(!trees.join("keel-trim").exists());
}
