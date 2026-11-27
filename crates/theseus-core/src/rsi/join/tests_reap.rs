//! The install's build tree is reaped when the install is provably finished with it, and a
//! join leaves no tree open after any phase (issue 6, the install's reap and its guard).

use crate::rsi::join::tests_publish::PassYes as _;
use std::sync::Arc;

use theseus_protocol::rsi::SelfMode;

use super::guard::{assert_closed, left_open};
use super::install::{self, target_of};
use super::tests::{rig_with, Builds, PROVED, SONNET};
use super::Seams;

/// KEY: the install's build tree is there for the unit after the join, and gone once the new
/// daemon has written `self.installed`; a second look removes nothing more.
#[tokio::test]
async fn the_installs_build_tree_is_reaped_once_it_is_installed() {
    let r = rig_with(SelfMode::Act, |c| {
        c.self_improve.install = crate::config::self_improve::SelfInstall::Auto
    });
    r.seed("trim-the-sails", PROVED, SONNET);
    let installs = Arc::new(Builds::default());
    let seams = Seams {
        installer: installs.clone(),
        ..r.seams()
    };
    r.core.self_join_pass_yes(&seams).await.unwrap();
    let merge = r.upstream_main();
    let target = target_of(&r.f.place, &merge);
    assert!(target.join("release").exists(), "the unit still needs it");
    assert_eq!(
        r.core.reap_installed_targets(),
        0,
        "not installed yet: nothing is reaped"
    );
    assert!(target.exists());
    // The new daemon, serving the merge, writes its row, and the tree goes with it.
    assert_eq!(
        r.core.self_installed_as(&merge).unwrap(),
        Some(merge.clone())
    );
    assert!(!target.exists(), "{} was reaped", target.display());
    assert_eq!(r.core.reap_installed_targets(), 0);
}

/// An install that finished in an earlier run and left its tree is cleared when the daemon
/// serves, by its row alone.
#[tokio::test]
async fn a_leftover_tree_of_an_installed_merge_is_reaped_at_serving() {
    let r = rig_with(SelfMode::Act, |c| {
        c.self_improve.install = crate::config::self_improve::SelfInstall::Auto
    });
    r.seed("trim-the-sails", PROVED, SONNET);
    let seams = Seams {
        installer: Arc::new(Builds::default()),
        ..r.seams()
    };
    r.core.self_join_pass_yes(&seams).await.unwrap();
    let merge = r.upstream_main();
    r.core.self_installed_as(&merge).unwrap();
    // The same tree is left again (as an earlier run left one), and another install's is not
    // touched: it has no row.
    let target = target_of(&r.f.place, &merge);
    std::fs::create_dir_all(target.join("release")).unwrap();
    let other = target_of(&r.f.place, "0123456789abcdef");
    std::fs::create_dir_all(other.join("release")).unwrap();
    assert_eq!(r.core.self_installed_after_serving().unwrap(), None);
    assert!(!target.exists(), "its row says it is installed");
    assert!(other.exists(), "an install with no row is not finished");
}

/// A build that fails leaves nothing the unit will read: its build tree is reaped at once.
#[test]
fn a_failed_install_build_reaps_its_tree() {
    let f = crate::rsi::tests_tree::fixture();
    let head = super::tests::git(&f.place.repo, &["rev-parse", "origin/main"]);
    let seen = Arc::new(std::sync::Mutex::new(None));
    let into = seen.clone();
    let built = install::built_tree(&f.place, &head, &move |_, t, _| {
        std::fs::create_dir_all(t.target.join("release"))?;
        *into.lock().unwrap() = Some(t.target.clone());
        anyhow::bail!("the build failed")
    });
    assert!(built.is_err());
    let target = seen.lock().unwrap().take().unwrap();
    assert!(!target.exists(), "{} was left", target.display());
    assert_eq!(left_open(&f.place.trees(), true), Vec::<String>::new());
}

/// The guard: a tree a phase leaves open is named, and fails a test.
#[test]
#[should_panic(expected = "a join left a tree open before the sign phase: planted-trim")]
fn the_guard_names_a_tree_left_open() {
    let f = crate::rsi::tests_tree::fixture();
    std::fs::create_dir_all(f.place.trees().join("planted-trim")).unwrap();
    assert_closed(&f.place, "before the sign phase", false);
}

/// The merge's tree is open across the phases that use it, so only the end of a pass asks for it.
#[test]
fn the_merge_tree_is_asked_about_only_at_the_end_of_a_pass() {
    let f = crate::rsi::tests_tree::fixture();
    std::fs::create_dir_all(f.place.trees().join("join-trim")).unwrap();
    assert_closed(&f.place, "before the push phase", false);
    assert_eq!(left_open(&f.place.trees(), true), ["join-trim"]);
}
