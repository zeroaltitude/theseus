//! The floor under `--stdio` (theseus-0h9f3 L21, `(P1)`). A `--stdio`
//! daemon serves a store of its own -- the store is single-process, and a
//! spawned stdio server must not fight a running daemon for the same
//! directory -- so it opens `<state>/store-stdio`, and `rpc::spool_dir`
//! puts its completion spool beside it as `<state>/spool-stdio`. The floor
//! named the bare literals `<state>/store` and `<state>/spool`, so in that
//! mode **the pair in use was not held and a pair not in use was**: a tool
//! call naming the owner's own `--stdio` session history ran at the tool's
//! posture, and an L1 job's view showed it.
//!
//! `--stdio` is not a diagnostic mode. It is the mode the CLI spawns
//! (`crates/theseus/src/client.rs`, `.arg("--stdio")`, behind
//! `theseus --spawn`), and the stdio daemon builds the same `Core` over the
//! same floor as the socket daemon.
//!
//! The floor now floors every store a mode can serve
//! (`rpc::STORE_DIR_NAMES`) and the spool beside each. **These tests name
//! `store-stdio` and `spool-stdio` as words on purpose**: they are the
//! row's negative, and re-introducing the bare literals in
//! `toolrun::guarded` reds them.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use serde_json::{json, Value};

use crate::provider::{FakeProvider, Scripted};
use crate::store::Store;
use crate::test_support::turn_in as turn;
use crate::{Config, Core};

struct Rig {
    core: Arc<Core>,
    /// The state dir the daemon serves from. Both store/spool pairs live
    /// under it, so one rig can ask about either mode.
    state: PathBuf,
    /// The workspace's root, where a read is open: the control.
    root: PathBuf,
    _dir: tempfile::TempDir,
}

/// A core whose state dir already holds both pairs -- the socket daemon's
/// `store`/`spool` and `--stdio`'s `store-stdio`/`spool-stdio`, each with a
/// file in it -- and one workspace root where reads are open. `script` is
/// given the canonical state dir and root.
fn rig(script: impl FnOnce(&Path, &Path) -> Vec<Scripted>) -> Rig {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().join("work");
    std::fs::create_dir_all(&root).unwrap();
    // Both pairs, as the daemon's two `private_dir` calls make them:
    // `store`/`spool` for the socket, `store-stdio`/`spool-stdio` for
    // `--stdio`. A file in each, so a read of it is a real read.
    for name in ["store", "store-stdio"] {
        let store = dir.path().join(name);
        let spool = crate::rpc::spool_dir(&store);
        for d in [&store, &spool] {
            std::fs::create_dir_all(d).unwrap();
            std::fs::write(d.join("history"), "the owner's own words\n").unwrap();
        }
    }
    let state = dir.path().canonicalize().unwrap();
    let root = root.canonicalize().unwrap();
    let mut cfg = Config::example();
    cfg.server.state_dir = state.to_string_lossy().into_owned();
    cfg.tools.projects_dir = Some(root.to_string_lossy().into_owned());
    cfg.tools.roots = vec![];
    let store = Store::open(&state.join("store")).unwrap();
    let fake = Arc::new(FakeProvider::scripted(script(&state, &root)));
    let core = Core::build(crate::rpc::Parts::for_tests(cfg, fake, store)).unwrap();
    Rig {
        core,
        state,
        root,
        _dir: dir,
    }
}

fn read(id: &str, path: &Path) -> (String, String, Value) {
    (id.into(), "fs_read".into(), json!({"path": path}))
}

fn tools(calls: &[(String, String, Value)]) -> Scripted {
    let calls: Vec<(&str, &str, Value)> = calls
        .iter()
        .map(|(id, name, input)| (id.as_str(), name.as_str(), input.clone()))
        .collect();
    Scripted::tools("", &calls)
}

/// Whether a read of the state-dir-relative path `at` waited for the
/// owner's approval.
async fn read_waits(at: &str) -> bool {
    let rel = at.to_string();
    let r = rig(move |state, _| {
        vec![
            tools(&[read("r1", &state.join(rel))]),
            Scripted::text("read it"),
        ]
    });
    let res = turn(&r.core, None, "read that file").await;
    res.awaiting_confirm.is_some()
}

/// The hole itself: under `--stdio` the store in use is `store-stdio` and
/// its spool is `spool-stdio`, and a read of either waits for the owner now.
/// Before the fix this reds on both -- the floor named `store` and `spool`,
/// so the two directories a `--stdio` daemon actually serves were read at
/// the tool's own posture, unheld (theseus-0h9f3 L21).
#[tokio::test]
async fn the_stdio_store_and_spool_in_use_are_held() {
    assert!(
        read_waits("store-stdio/history").await,
        "a read of the store a --stdio daemon serves must wait for the owner"
    );
    assert!(
        read_waits("spool-stdio/history").await,
        "a read of the spool beside it must wait for the owner"
    );
}

/// The other mode is unchanged: the socket daemon's `store` and `spool` are
/// held exactly as they were before this row. Half a fix floors one pair and
/// drops the other; this is the half that was already right.
#[tokio::test]
async fn the_socket_store_and_spool_are_held_as_before() {
    assert!(
        read_waits("store/history").await,
        "a read of the socket daemon's store must still wait"
    );
    assert!(
        read_waits("spool/history").await,
        "a read of the socket daemon's spool must still wait"
    );
}

/// The control, so that neither test above passes because every read waits:
/// a read inside the workspace is open at the tool's posture and asks
/// nothing. It also pins the rig itself -- the core's state dir is the
/// directory the two tests above name their paths under.
#[tokio::test]
async fn a_read_inside_the_workspace_still_asks_nothing() {
    let r = rig(|_, root| {
        vec![
            tools(&[read("r1", &root.join("note.txt"))]),
            Scripted::text("read it"),
        ]
    });
    std::fs::write(r.root.join("note.txt"), "a quiet harbour\n").unwrap();
    let res = turn(&r.core, None, "read the note").await;
    assert!(
        res.awaiting_confirm.is_none(),
        "a read in the workspace waited: {res:?}"
    );
    assert_eq!(r.core.cfg.state_dir(), r.state);
}
