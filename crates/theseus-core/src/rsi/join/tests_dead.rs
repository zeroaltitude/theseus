//! The start's sweep closes only a tree it can prove is the loop's own and abandoned (issue 41, `guard`): its mark
//! is there and this daemon did not open it. Everything else is left as it is.

use super::git;
use super::guard::{close_dead, forget_opened_here, left_open, marked, Dead, NoSweep, Swept};
use super::tests::git as run_git;
use crate::rsi::tree_lease::Lease;

/// A tree this daemon opened stays, whatever its kind; once the daemon has no record of it (a restart), it is
/// closed through its mark, with its build tree, and its mark goes. A merge's tree with no mark and an item's are
/// not touched.
#[test]
fn only_a_marked_tree_this_daemon_did_not_open_is_closed() {
    let f = crate::rsi::tests_tree::fixture();
    let head = run_git(&f.place.repo, &["rev-parse", "origin/main"]);
    let planted = git::detached(&f.place, "planted", "trim", &head).unwrap();
    assert!(
        marked(&f.place, "planted-trim"),
        "the mark is written with the tree"
    );
    std::fs::create_dir_all(f.place.trees().join("join-trim")).unwrap();
    std::fs::create_dir_all(f.place.trees().join("tsk-c")).unwrap();
    assert_eq!(
        close_dead(&f.place),
        Ok(vec![]),
        "a live pass's tree, and no mark elsewhere"
    );
    assert!(planted.dir.exists());
    forget_opened_here();
    assert_eq!(
        close_dead(&f.place),
        Ok(vec![Dead {
            tree: "planted-trim".into(),
            phase: "planted".into(),
            swept: Swept::Closed,
        }])
    );
    assert!(!planted.dir.exists());
    assert!(
        !planted.target.exists(),
        "a planted tree's build tree goes with it"
    );
    assert!(!marked(&f.place, "planted-trim"), "its mark goes with it");
    assert_eq!(left_open(&f.place.trees(), true), ["join-trim"]);
    assert!(
        f.place.trees().join("tsk-c").exists(),
        "an item's tree is the pipeline's"
    );
}

/// An owner's directory named like a phase's tree, with work in it and no mark, survives a start, and so does an
/// attached worktree of that kind of name: both are reported as not the loop's. (Round 1 removed the first; this
/// test replaces `a_tree_git_does_not_know_is_removed_and_one_a_gate_holds_is_left`.)
#[test]
fn a_phase_named_directory_the_loop_did_not_open_is_left_as_it_is() {
    let f = crate::rsi::tests_tree::fixture();
    let trees = f.place.trees();
    std::fs::create_dir_all(trees.join("keel-notes/src")).unwrap();
    std::fs::write(
        trees.join("keel-notes/src/draft.md"),
        "not committed anywhere",
    )
    .unwrap();
    let attached = trees.join("probe-notes");
    run_git(
        &f.place.repo,
        &[
            "worktree",
            "add",
            "-q",
            "-b",
            "notes",
            attached.to_str().unwrap(),
        ],
    );
    std::fs::write(attached.join("uncommitted.txt"), "the owner's").unwrap();
    forget_opened_here();
    let dead = close_dead(&f.place).unwrap();
    assert_eq!(
        dead,
        vec![
            Dead {
                tree: "keel-notes".into(),
                phase: "keel".into(),
                swept: Swept::NotTheLoops,
            },
            Dead {
                tree: "probe-notes".into(),
                phase: "probe".into(),
                swept: Swept::NotTheLoops,
            },
        ]
    );
    assert!(trees.join("keel-notes/src/draft.md").exists());
    assert!(attached.join("uncommitted.txt").exists());
    // The loop will not open over it either: the open fails, naming it.
    let head = run_git(&f.place.repo, &["rev-parse", "origin/main"]);
    let e = git::detached(&f.place, "probe", "notes", &head).unwrap_err();
    assert!(
        format!("{e:#}").contains("the loop did not open it"),
        "{e:#}"
    );
    assert!(attached.join("uncommitted.txt").exists());
}

/// A marked tree a gate holds is left, with the reason; a marked tree git does not know is left too (no plain
/// delete stands in for the close); a mark whose tree is gone is cleared.
#[test]
fn a_marked_tree_a_gate_holds_or_git_does_not_know_is_left() {
    let f = crate::rsi::tests_tree::fixture();
    let head = run_git(&f.place.repo, &["rev-parse", "origin/main"]);
    let probe = git::detached(&f.place, "probe", "0123456789ab", &head).unwrap();
    let _gate = Lease::take(&[&probe.dir]);
    std::fs::create_dir_all(f.place.trees().join("keel-trim/src")).unwrap();
    super::guard::mark(&f.place, "keel-trim", "keel").unwrap();
    super::guard::mark(&f.place, "revert-gone", "revert").unwrap();
    forget_opened_here();
    let dead = close_dead(&f.place).unwrap();
    let of = |name: &str| dead.iter().find(|d| d.tree == name).unwrap().swept.clone();
    assert!(
        matches!(of("probe-0123456789ab"), Swept::NotClosed(w) if w.contains("a gate is still running")),
        "{dead:?}"
    );
    assert!(matches!(of("keel-trim"), Swept::NotClosed(_)), "{dead:?}");
    assert_eq!(of("revert-gone"), Swept::MarkCleared);
    assert!(probe.dir.exists());
    assert!(f.place.trees().join("keel-trim/src").exists());
    assert!(!marked(&f.place, "revert-gone"));
}

/// A daemon that does not own the trees sweeps nothing and opens no tree; a trees directory that leads out of the
/// state directory is not swept either.
#[test]
fn without_the_trees_lock_or_inside_the_state_nothing_is_swept_or_opened() {
    let f = crate::rsi::tests_tree::fixture();
    let head = run_git(&f.place.repo, &["rev-parse", "origin/main"]);
    let mut other = f.place.clone();
    other.owner = None;
    assert_eq!(close_dead(&other), Err(NoSweep::NotOwner));
    let e = git::detached(&other, "install", "0123456789ab", &head).unwrap_err();
    assert!(
        format!("{e:#}").contains(crate::rsi::trees_lock::NOT_OWNER),
        "{e:#}"
    );
    // The trees directory, a link out of the state directory.
    let away = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(f.place.root()).unwrap();
    std::os::unix::fs::symlink(away.path(), f.place.trees()).unwrap();
    assert_eq!(close_dead(&f.place), Err(NoSweep::Outside));
    assert!(git::detached(&f.place, "install", "0123456789ab", &head).is_err());
}
