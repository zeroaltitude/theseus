//! The start's sweep closes only a tree it can prove is the loop's own, completely created, and abandoned (issue 41,
//! `guard`): its `created` mark names the worktree git lists now, and this daemon does not have it open. Everything
//! else is left as it is; and the opener (`git::detached`) turns no refusal into a delete.

use super::git;
use super::guard::{
    close_dead, forget_opened_here, left_open, mark_of, marked, Dead, NoSweep, Stage, Swept,
};
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

/// A marked tree a gate holds is left, with the reason; a directory with only a reservation is left (nothing proves
/// the loop made it); a reservation with nothing at its path is cleared.
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
    assert!(
        matches!(of("keel-trim"), Swept::NotTheTreeTheLoopMade(w) if w.contains("reservation")),
        "{dead:?}"
    );
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

/// The opener turns no refusal into a delete: a marked tree a gate holds, and a marked directory git does not know,
/// are both still there after a refused open; and a second open of a path this daemon has open is refused.
#[test]
fn the_opener_turns_no_refusal_into_a_delete() {
    let f = crate::rsi::tests_tree::fixture();
    let head = run_git(&f.place.repo, &["rev-parse", "origin/main"]);
    // A path this daemon has open: refused, and the tree is as it was.
    let held = git::detached(&f.place, "probe", "0123456789ab", &head).unwrap();
    std::fs::write(held.dir.join("work.txt"), "in use").unwrap();
    let e = git::detached(&f.place, "probe", "0123456789ab", &head).unwrap_err();
    assert!(
        format!("{e:#}").contains("open in this daemon already"),
        "{e:#}"
    );
    assert!(held.dir.join("work.txt").exists());
    // The same tree after a restart, a gate holding it: the open fails on the gate's refusal, and the tree stays.
    forget_opened_here();
    let _gate = Lease::take(&[&held.dir]);
    let e = git::detached(&f.place, "probe", "0123456789ab", &head).unwrap_err();
    assert!(format!("{e:#}").contains("would not close"), "{e:#}");
    assert!(
        held.dir.join("work.txt").exists(),
        "a gate's tree is not deleted"
    );
    // A marked directory git does not know (a created mark copied onto it): left, and the open fails.
    let stray = f.place.trees().join("planted-trim");
    std::fs::create_dir_all(stray.join("src")).unwrap();
    std::fs::write(stray.join("src/draft.md"), "not git's").unwrap();
    let m = mark_of(&f.place, "probe-0123456789ab").unwrap();
    std::fs::write(
        f.place.opened().join("planted-trim"),
        serde_json::to_string(&m).unwrap(),
    )
    .unwrap();
    let e = git::detached(&f.place, "planted", "trim", &head).unwrap_err();
    assert!(
        format!("{e:#}").contains("not the tree the loop made"),
        "{e:#}"
    );
    assert!(stray.join("src/draft.md").exists());
}

/// A mark is a reservation until the tree exists. Daemon A reserves a name and stops before it makes the tree; a
/// person then makes a worktree at that path, on a branch, with work in it; daemon B's start finds A's old genuine
/// mark and leaves the person's worktree as it is. A `created` mark of the loop's own tree is closed (above).
#[test]
fn a_stale_reservation_never_closes_a_tree_the_loop_did_not_make() {
    let f = crate::rsi::tests_tree::fixture();
    super::guard::mark(&f.place, "probe-notes", "probe").unwrap();
    assert_eq!(
        mark_of(&f.place, "probe-notes").unwrap().stage,
        Stage::Reserved
    );
    let path = f.place.trees().join("probe-notes");
    std::fs::create_dir_all(f.place.trees()).unwrap();
    run_git(
        &f.place.repo,
        &[
            "worktree",
            "add",
            "-q",
            "-b",
            "notes",
            path.to_str().unwrap(),
        ],
    );
    std::fs::write(path.join("uncommitted.txt"), "the owner's").unwrap();
    forget_opened_here();
    let dead = close_dead(&f.place).unwrap();
    assert!(
        matches!(&dead[0].swept, Swept::NotTheTreeTheLoopMade(_)),
        "{dead:?}"
    );
    assert!(path.join("uncommitted.txt").exists());
    // And with a `created` mark from the loop copied onto that name, the worktree still is not the one it made.
    let head = run_git(&f.place.repo, &["rev-parse", "origin/main"]);
    let made = git::detached(&f.place, "probe", "0123456789ab", &head).unwrap();
    let m = mark_of(&f.place, &made.slug).unwrap();
    std::fs::write(
        f.place.opened().join("probe-notes"),
        serde_json::to_string(&m).unwrap(),
    )
    .unwrap();
    forget_opened_here();
    let dead = close_dead(&f.place).unwrap();
    let notes = dead.iter().find(|d| d.tree == "probe-notes").unwrap();
    assert!(
        matches!(&notes.swept, Swept::NotTheTreeTheLoopMade(_)),
        "{dead:?}"
    );
    assert!(path.join("uncommitted.txt").exists());
    let own = dead.iter().find(|d| d.tree == made.slug).unwrap();
    assert_eq!(
        own.swept,
        Swept::Closed,
        "the loop's own made tree is closed"
    );
}

/// An item's opener at a name the loop marked: a stale reservation is cleared (nothing was made), a `created` mark
/// refuses the open. With no trees directory at all, the marks are still read and stale ones cleared.
#[test]
fn an_items_opener_clears_a_reservation_and_refuses_a_made_tree() {
    let f = crate::rsi::tests_tree::fixture();
    super::guard::mark(&f.place, "keel-gone", "keel").unwrap();
    assert!(!f.place.trees().exists());
    assert_eq!(
        close_dead(&f.place),
        Ok(vec![Dead {
            tree: "keel-gone".into(),
            phase: "keel".into(),
            swept: Swept::MarkCleared,
        }])
    );
    super::guard::mark(&f.place, "probe-notes", "probe").unwrap();
    let t =
        crate::rsi::tree::open(&f.place, "probe-notes", theseus_protocol::now_unix_ms()).unwrap();
    assert!(
        !marked(&f.place, "probe-notes"),
        "the reservation is cleared"
    );
    crate::rsi::tree::close(&f.place, &t).unwrap();
    let head = run_git(&f.place.repo, &["rev-parse", "origin/main"]);
    let made = git::detached(&f.place, "probe", "0123456789ab", &head).unwrap();
    let e =
        crate::rsi::tree::open(&f.place, &made.slug, theseus_protocol::now_unix_ms()).unwrap_err();
    assert!(
        format!("{e:#}").contains("open already") || format!("{e:#}").contains("marked"),
        "{e:#}"
    );
    // With the tree gone from disk but its created mark standing, the item's open is refused on the mark.
    forget_opened_here();
    std::fs::remove_dir_all(&made.dir).unwrap();
    let e =
        crate::rsi::tree::open(&f.place, &made.slug, theseus_protocol::now_unix_ms()).unwrap_err();
    assert!(format!("{e:#}").contains("is marked at"), "{e:#}");
}
