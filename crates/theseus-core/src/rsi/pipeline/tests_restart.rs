//! A restart in the middle of the self loop's install (issue 41): the install's tree stays, every
//! start is held on the slots, and the next daemon's start closes it through the loop's mark, writes
//! a row that says which and why, and the next tick starts a build. A second daemon on the same state
//! directory (a `--stdio` daemon beside the service, with a store of its own) neither sweeps nor opens
//! a tree the loop marks.

use std::panic::AssertUnwindSafe;

use super::tests::rig_with;
use crate::rsi::join::guard::{forget_opened_here, left_open, Dead, NoSweep, Swept};
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
    assert_eq!(
        left_open(&place.trees(), false),
        std::slice::from_ref(&tree)
    );
    // This daemon opened it, so to this daemon it is a live pass's: the sweep leaves it.
    assert_eq!(r.core.close_dead_trees(), Ok(Vec::<Dead>::new()));
    let t = r.tick().await;
    assert!(t.started.is_empty(), "{t:?}");
    assert_eq!(t.hold.as_deref(), Some("slots"));

    // The restart: the next daemon has no memory of the trees the last one opened.
    forget_opened_here();
    let dead = r.core.close_dead_trees();
    assert_eq!(
        dead,
        Ok(vec![Dead {
            tree: tree.clone(),
            phase: "install".into(),
            swept: Swept::Closed,
        }])
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
    assert_eq!(r.rows("self.pipeline.started").len(), 1);
}

/// A daemon over the same state directory with a store of its own (as a `--stdio` daemon is beside the service),
/// while another process holds the trees' lock: it does not sweep, and it opens no tree the loop marks. The tree
/// the other daemon's install left is still there after its start.
#[tokio::test]
async fn a_second_daemon_on_the_state_directory_neither_sweeps_nor_opens_a_tree() {
    let mut f = crate::rsi::tests_tree::fixture();
    let head = f
        .place
        .git_ok(&f.place.repo, &["rev-parse", "refs/remotes/origin/main"])
        .unwrap();
    let live = crate::rsi::join::git::detached(&f.place, "install", &head[..12], &head).unwrap();
    forget_opened_here();
    // This process lets go of the lock, and another process takes it: the service daemon, here a shell holding it.
    f.place.owner = None;
    let lock = crate::rsi::trees_lock::lock_path(&f.place.root());
    let mut holder = std::process::Command::new("sh")
        .arg("-c")
        .arg("exec 9>\"$1\" && flock -x 9 && echo locked && exec sleep 60")
        .arg("sh")
        .arg(&lock)
        .stdout(std::process::Stdio::piped())
        .spawn()
        .unwrap();
    let mut said = String::new();
    std::io::BufRead::read_line(
        &mut std::io::BufReader::new(holder.stdout.take().unwrap()),
        &mut said,
    )
    .unwrap();
    assert_eq!(said.trim(), "locked");
    let stdio = core_on(&f, "store-stdio");
    assert_eq!(stdio.close_dead_trees(), Err(NoSweep::NotOwner));
    assert!(live.dir.exists(), "the other daemon's tree is not touched");
    let repo = stdio.cfg.self_improve.repo.clone().unwrap();
    let place = stdio.tools.self_pr.place(&repo);
    assert!(place.owner.is_none());
    let e = crate::rsi::join::git::detached(&place, "keel", "self/x", &head).unwrap_err();
    assert!(
        format!("{e:#}").contains(crate::rsi::trees_lock::NOT_OWNER),
        "{e:#}"
    );
    let _ = holder.kill();
    let _ = holder.wait();
}

/// A core over `f`'s state directory with the store `store` (a daemon of its own).
fn core_on(f: &crate::rsi::tests_tree::Fixture, store: &str) -> std::sync::Arc<crate::Core> {
    let s = crate::store::Store::open(&f.place.state.join(store)).unwrap();
    let mut cfg = crate::Config::example();
    cfg.server.state_dir = f.place.state.to_string_lossy().into_owned();
    cfg.server.disk_warn_mb = 0;
    cfg.self_improve.mode = theseus_protocol::rsi::SelfMode::Act;
    cfg.self_improve.repo = Some(f.place.repo.to_string_lossy().into_owned());
    let core = crate::Core::build(crate::rpc::Parts::for_tests(
        cfg,
        std::sync::Arc::new(crate::provider::FakeProvider::default()),
        s,
    ))
    .unwrap();
    core.tools
        .self_pr
        .set_env(crate::rsi::tests_tree::hermetic());
    core
}
