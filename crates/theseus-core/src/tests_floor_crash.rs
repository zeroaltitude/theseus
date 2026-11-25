//! The crash files on the floor: a tool's read of the crash file a
//! daemon's panic hook writes, or of one a start has kept under `crashes/`,
//! waits for the owner as a read of the store does. A read of an ordinary
//! file beside them is as it was: outside the roots, a notice, never held.

use std::sync::Arc;

use serde_json::json;

use crate::provider::{FakeProvider, Scripted};
use crate::store::Store;
use crate::test_support::turn_in as turn;
use crate::{Config, Core};

/// The approval a turn that reads the state-dir-relative path `at` waits
/// for, if any, with both daemons' crash files, a kept one and ordinary
/// files beside them in the state dir.
async fn read_of(at: &str) -> Option<String> {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().join("work");
    std::fs::create_dir_all(&root).unwrap();
    let state = dir.path().canonicalize().unwrap();
    std::fs::create_dir_all(state.join("crashes")).unwrap();
    std::fs::create_dir_all(state.join("crashes-old")).unwrap();
    for f in [
        "crash-socket.json",
        "crash-stdio.json",
        "crashes/crash-1000-42.json",
        "crashes-old/crash-1000-42.json",
        "crash-socket.txt",
    ] {
        std::fs::write(state.join(f), "{\"message\": \"a quiet harbour\"}\n").unwrap();
    }
    let mut cfg = Config::example();
    cfg.server.state_dir = state.to_string_lossy().into_owned();
    cfg.tools.projects_dir = Some(root.canonicalize().unwrap().to_string_lossy().into_owned());
    cfg.tools.roots = vec![];
    let store = Store::open(&state.join("store")).unwrap();
    let path = state.join(at);
    let fake = Arc::new(FakeProvider::scripted(vec![
        Scripted::tools("", &[("r1", "fs_read", json!({ "path": path }))]),
        Scripted::text("read it"),
    ]));
    let core = Core::build(crate::rpc::Parts::for_tests(cfg, fake, store)).unwrap();
    assert_eq!(core.cfg.state_dir(), state);
    turn(&core, None, "read that file").await.awaiting_confirm
}

/// A read of either daemon's crash file, or of a kept one, waits for the
/// owner, as a read of the store does.
#[tokio::test]
async fn a_read_of_a_crash_file_waits_for_the_owner() {
    for at in [
        "crash-socket.json",
        "crash-stdio.json",
        "crashes/crash-1000-42.json",
    ] {
        let waited = read_of(at).await;
        assert!(
            waited.is_some(),
            "a read of {at} did not wait for the owner"
        );
    }
}

/// The control: files beside them, named like them, are read as before.
#[tokio::test]
async fn a_read_beside_the_crash_files_is_as_before() {
    for at in ["crash-socket.txt", "crashes-old/crash-1000-42.json"] {
        let waited = read_of(at).await;
        assert!(waited.is_none(), "a read of {at} waited: {waited:?}");
    }
}
