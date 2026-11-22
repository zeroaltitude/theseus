//! A restart in the middle of the self loop's install (issue 41): the install's tree stays, every
//! start is held on the slots, and the next daemon's start closes it, writes a row that says which
//! and why, and the next tick starts a build.

use std::panic::AssertUnwindSafe;

use super::tests::rig_with;
use crate::rsi::join::guard::{forget_opened_here, left_open, Dead};
use crate::rsi::join::install;

/// KEY: an install's build interrupted by a restart leaves `install-<merge>` open; until the next
/// start's sweep the slot is held on it; after it, the tree is gone, its row says which tree and
/// why, and a build starts.
#[tokio::test]
async fn a_restart_mid_install_leaves_a_tree_the_next_start_closes_and_a_build_starts() {
    let r = rig_with(1, 30.0);
    let place = &r.f.place;
    let merge = place
        .git_ok(&place.repo, &["rev-parse", "refs/remotes/origin/main"])
        .unwrap();
    let tree = format!("install-{}", &merge[..12]);
    // The daemon is restarted while the install builds: the build never returns, so the install
    // never reaches its close.
    let killed = std::panic::catch_unwind(AssertUnwindSafe(|| {
        install::built_tree(place, &merge, &|_, _, _| {
            panic!("the daemon is restarted in the middle of the install's build")
        })
    }));
    assert!(killed.is_err());
    assert_eq!(left_open(&place.trees(), false), [tree.clone()]);
    // This daemon opened it, so to this daemon it is a live pass's: the sweep leaves it.
    assert_eq!(r.core.close_dead_trees(), Vec::<Dead>::new());
    let t = r.tick().await;
    assert!(t.started.is_empty(), "{t:?}");
    assert_eq!(t.hold.as_deref(), Some("slots"));

    // The restart: the next daemon has no memory of the trees the last one opened.
    forget_opened_here();
    let dead = r.core.close_dead_trees();
    assert_eq!(
        dead,
        vec![Dead {
            tree: tree.clone(),
            phase: "install".into(),
            closed: Ok(()),
        }]
    );
    assert_eq!(left_open(&place.trees(), true), Vec::<String>::new());
    let rows = r.rows("self.pipeline.cleared");
    assert_eq!(rows.len(), 1, "{rows:?}");
    assert_eq!(rows[0]["tree"], tree.as_str());
    assert_eq!(rows[0]["phase"], "install");
    assert_eq!(rows[0]["closed"], true);
    assert!(
        rows[0]["why"]
            .as_str()
            .unwrap()
            .contains("a daemon that is gone"),
        "{rows:?}"
    );
    let t = r.tick().await;
    assert_eq!(t.started.len(), 1, "the slot is free: {t:?}");
    assert_ne!(t.hold.as_deref(), Some("slots"));
}
