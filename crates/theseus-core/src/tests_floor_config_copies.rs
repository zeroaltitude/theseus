//! The copies the daemon keeps when it changes its own settings are on the
//! floor (issue 22, undo): `<state>/config-changes/` holds each previous
//! copy of the config, with every setting and the name of every secret in
//! it, the candidates, and the restart's marker.
//!
//! A tool that could read one would see all of them, and one that could write
//! one would have a later undo, or the restart's way back, put its text
//! back as the config. So `fs.read` and `fs.write` of any of them wait for
//! the owner even under `open` and inside the workspace's root, and no L1 view
//! shows the directory.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use serde_json::{json, Value};

use crate::policy::Posture;
use crate::provider::{FakeProvider, Scripted};
use crate::store::Store;
use crate::test_support::turn_in as turn;
use crate::{Config, Core};

const COPY: &str = "narrative = false\n[secrets]\nanthropic_api_key = \"op://v/i/f\"\n";

struct Rig {
    core: Arc<Core>,
    copy: PathBuf,
    _dir: tempfile::TempDir,
}

/// A core at `open`, whose state dir is inside the workspace's root (so only
/// the floor can hold a call), with one kept copy in it. `script` is given the
/// copy's path.
fn rig(script: impl FnOnce(&Path) -> Vec<Scripted>) -> Rig {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().join("work");
    let state = root.join("state");
    let kept = crate::config_change::write::dir(&state);
    std::fs::create_dir_all(&kept).unwrap();
    let root = root.canonicalize().unwrap();
    let state = state.canonicalize().unwrap();
    let copy = crate::config_change::write::dir(&state).join("previous-0001.toml");
    std::fs::write(&copy, COPY).unwrap();
    let mut cfg = Config::example();
    cfg.server.state_dir = state.to_string_lossy().into_owned();
    cfg.tools.projects_dir = Some(root.to_string_lossy().into_owned());
    cfg.tools.roots = vec![];
    cfg.policy.enforcement = Posture::Open;
    let store = Store::open(&state.join("store")).unwrap();
    let fake = Arc::new(FakeProvider::scripted(script(&copy)));
    let core = Core::build(crate::rpc::Parts::for_tests(cfg, fake, store)).unwrap();
    Rig {
        core,
        copy,
        _dir: dir,
    }
}

fn call(id: &str, tool: &str, input: Value) -> Scripted {
    Scripted::tools("", &[(id, tool, input)])
}

/// Whether a call of `tool` on the kept copy waited for the owner, as a floor
/// hold, and left the copy as it was.
async fn held(tool: &'static str, input: impl FnOnce(&Path) -> Value + Send + 'static) -> bool {
    let r = rig(move |copy| vec![call("c1", tool, input(copy)), Scripted::text("done")]);
    let res = turn(&r.core, None, "look at the kept copy").await;
    let waits = res.awaiting_confirm.is_some();
    if waits {
        let pending = r.core.pending_confirms(&res.session_id).unwrap();
        assert!(pending[0].floor, "{}", pending[0].reason);
    }
    assert_eq!(std::fs::read_to_string(&r.copy).unwrap(), COPY);
    waits
}

/// `fs.read` of a kept copy waits: it holds every setting.
#[tokio::test]
async fn a_tool_cannot_read_a_kept_copy() {
    assert!(held("fs_read", |c| json!({"path": c})).await);
}

/// `fs.write` over a kept copy waits, and so does a new file beside the
/// copies: an undo would restore what a tool put there.
#[tokio::test]
async fn a_tool_cannot_write_a_kept_copy() {
    assert!(held("fs_write", |c| json!({"path": c, "content": "x = 1\n"})).await);
    assert!(
        held("fs_write", |c| json!({
            "path": c.with_file_name("previous-0099.toml"),
            "content": "[policy]\nenforcement = \"open\"\n"
        }))
        .await
    );
}

/// The control: a read of a file in the workspace beside the state dir asks
/// nothing, so neither test above passes because every call waits.
#[tokio::test]
async fn a_read_beside_the_copies_asks_nothing() {
    let r = rig(|copy| {
        let note = copy.parent().unwrap().parent().unwrap().join("note.txt");
        std::fs::write(&note, "a quiet harbour\n").unwrap();
        vec![
            call("c1", "fs_read", json!({"path": note})),
            Scripted::text("done"),
        ]
    });
    let res = turn(&r.core, None, "read the note").await;
    assert!(res.awaiting_confirm.is_none(), "{res:?}");
}
