//! Diagnostics in edit results (L3, theseus-n88g.9), against theseus-lsp's
//! fake served in this process: an `fs.edit` that makes an error gets the
//! block, errors first and capped, with the other files' count; a clean
//! edit's block says there are none; no server up is no block and no spawn;
//! a shared place's edit gets none; a server that never answers costs the
//! call the edit bound, on tokio's paused clock; and the block adds no
//! frame.

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use serde_json::{json, Value};
use theseus_lsp::fake::{Config as FakeConfig, Diagnostics};
use theseus_protocol::{SessionKind, TurnSubmitResult};

use crate::bus::EventSink;
use crate::policy::Posture;
use crate::provider::{FakeProvider, Scripted};
use crate::session::SessionRecord;
use crate::store::Store;
use crate::tests_lsp::{lsp_config, project, InProcess};
use crate::turn::TurnRequest;
use crate::{Config, Core};

struct Rig {
    core: Arc<Core>,
    work: PathBuf,
    spawner: Arc<InProcess>,
    _dir: tempfile::TempDir,
}

/// A core with `[lsp]` on over the fake (as `fake`, `cfg` its script),
/// answering with `script`, its config changed by `tweak`.
fn rig(script: Vec<Scripted>, fake: FakeConfig, tweak: impl FnOnce(&mut Config, &Path)) -> Rig {
    let dir = tempfile::tempdir().unwrap();
    let work = project(dir.path());
    let mut cfg = Config::example();
    cfg.server.state_dir = dir.path().to_string_lossy().into_owned();
    cfg.tools.projects_dir = Some(work.to_string_lossy().into_owned());
    cfg.tools.roots = vec![];
    cfg.policy.enforcement = Posture::Notify;
    lsp_config(&mut cfg, 10.0);
    tweak(&mut cfg, &work);
    let store = Store::open(&dir.path().join("store")).unwrap();
    let core = Core::build(crate::rpc::Parts::for_tests(
        cfg,
        Arc::new(FakeProvider::scripted(script)),
        store,
    ))
    .unwrap();
    let spawner = Arc::new(InProcess::default());
    *spawner.cfg.lock().unwrap() = Some(fake);
    core.tools
        .lsp
        .as_ref()
        .unwrap()
        .set_spawner(spawner.clone());
    Rig {
        core,
        work,
        spawner,
        _dir: dir,
    }
}

/// A session, posting to `place` when given.
fn session(core: &Core, place: Option<&str>) -> String {
    let r = SessionRecord::new(SessionKind::Conversation, None);
    core.store.put_session(&r.session_id, &r).unwrap();
    if let Some(p) = place {
        core.outbox.bind_place(p, &r.session_id).unwrap();
    }
    r.session_id
}

async fn turn(core: &Arc<Core>, sid: &str, input: &str) -> TurnSubmitResult {
    let rec = core
        .store
        .get_session::<SessionRecord>(sid)
        .unwrap()
        .unwrap();
    let (live, _) = core.live_profile();
    let target = core.runner.resolve_target(&live, None, None, None).unwrap();
    core.runner
        .run(TurnRequest {
            session: rec,
            input: Some(input.into()),
            target,
            sink: EventSink::new(core.bus.clone(), sid, None),
            author: "cli".into(),
            recompile: None,
            attachments: vec![],
            arrived: None,
            reply_to: None,
        })
        .await
        .unwrap()
}

/// A call's result as its session stores it: its text and its meta.
fn result_of(core: &Core, sid: &str, id: &str) -> (String, Value) {
    core.store
        .session_nodes(sid)
        .unwrap()
        .into_iter()
        .find_map(|(_, n)| match n.body {
            crate::node::Body::ToolResult {
                tool_use_id,
                content,
                meta,
                ..
            } if tool_use_id == id => Some((content, meta)),
            _ => None,
        })
        .unwrap_or_else(|| panic!("no result for {id}"))
}

/// An `fs.edit` call, then the model's last word.
fn edit(id: &str, path: &str, old: &str, new: &str) -> Vec<Scripted> {
    vec![
        Scripted::tools(
            "Editing.",
            &[(
                id,
                "fs_edit",
                json!({"path": path, "old_string": old, "new_string": new}),
            )],
        ),
        Scripted::text("Done."),
    ]
}

/// Start the fake on the project, with `files` open in it, as an
/// `lsp.diagnostics` call does.
async fn open(r: &Rig, files: &[&str]) {
    let tool = r
        .core
        .tools
        .registry
        .get("lsp.diagnostics")
        .unwrap()
        .clone();
    for f in files {
        tool.run_async(&json!({"path": r.work.join(f)}), &r.core.tools.ctx)
            .await
            .unwrap();
    }
}

/// The brief's first proof: an edit that makes errors gets the block, its
/// errors first and capped at twenty lines with a count of the rest, and
/// the count of the errors the server reported meanwhile in another file;
/// `meta.lsp` says what was attached. The fix's block says there are none.
#[tokio::test]
async fn an_edit_that_makes_an_error_gets_the_block_and_a_clean_one_says_none() {
    let errors: String = (0..25).map(|i| format!("ERROR {i}\n")).collect();
    let mut script = edit("e1", "a.fake", "let y = 2\n", &errors);
    script.extend(edit("e2", "a.fake", &errors, "let y = 3\n"));
    script.extend(edit("e3", "a.fake", "ERROR here\n", "fine here\n"));
    let r = rig(script, FakeConfig::default(), |_, _| {});
    let a = r.work.join("a.fake");
    open(&r, &["a.fake", "b.fake"]).await;
    // Another file gains an error behind the server's back: the edit's
    // sync sends it, and its push lands during the wait.
    std::fs::write(r.work.join("b.fake"), "use total here\nERROR there\n").unwrap();
    let sid = session(&r.core, None);
    turn(&r.core, &sid, "make it fail").await;
    let (text, meta) = result_of(&r.core, &sid, "e1");
    assert!(text.contains("\nErrors after this edit:\n"), "{text}");
    assert!(
        text.contains(&format!("{} (fake): 26 errors", a.display())),
        "{text}"
    );
    let shown = text
        .lines()
        .filter(|l| l.contains("error [F1]: planted error"))
        .count();
    assert_eq!(shown, 20, "{text}");
    assert!(text.contains("…[6 more not shown"), "{text}");
    assert!(text.contains("Other files: 1 new error"), "{text}");
    let lsp = &meta["lsp"];
    assert_eq!(lsp["server"], "fake");
    assert_eq!(lsp["freshness"], "pushed");
    assert_eq!(
        (lsp["errors"].as_u64(), lsp["errors_shown"].as_u64()),
        (Some(26), Some(20))
    );
    assert_eq!(lsp["other_errors"], 1);
    assert!(lsp["waited_ms"].as_u64().is_some());
    assert_eq!(meta["replacements"], 1, "the tool's own meta stays");
    turn(&r.core, &sid, "fix most").await;
    let (text, meta) = result_of(&r.core, &sid, "e2");
    assert!(text.contains("(fake): 1 error"), "{text}");
    assert_eq!(meta["lsp"]["other_errors"], 0, "b's error is not new");
    turn(&r.core, &sid, "fix the last").await;
    let (text, meta) = result_of(&r.core, &sid, "e3");
    assert!(
        text.contains(&format!("{} (fake): no errors", a.display())),
        "{text}"
    );
    assert_eq!(meta["lsp"]["errors"], 0);
    assert_eq!(
        r.spawner.count(),
        1,
        "one server, started by the first read"
    );
}

/// No server up for the file's root: the edit gets no block, and no server
/// starts.
#[tokio::test]
async fn no_server_up_is_no_block_and_no_spawn() {
    let r = rig(
        edit("e1", "a.fake", "let y = 2\n", "ERROR 2\n"),
        FakeConfig::default(),
        |_, _| {},
    );
    let sid = session(&r.core, None);
    turn(&r.core, &sid, "edit").await;
    let (text, meta) = result_of(&r.core, &sid, "e1");
    assert!(!text.contains("Errors after this edit"), "{text}");
    assert!(meta.get("lsp").is_none(), "{meta}");
    assert_eq!(r.spawner.count(), 0);
}

/// The place rule: a server reads the whole project, so a shared place's
/// edit, which `[places] public_paths` lets it make, gets no block, though
/// a server is up for its root.
#[tokio::test]
async fn a_shared_places_edit_gets_no_block() {
    let r = rig(
        edit("e1", "a.fake", "let y = 2\n", "ERROR 2\n"),
        FakeConfig::default(),
        |cfg, work| cfg.places.public_paths = vec![work.to_string_lossy().into_owned()],
    );
    open(&r, &["a.fake"]).await;
    let sid = session(&r.core, Some("channel:31415926535"));
    turn(&r.core, &sid, "edit").await;
    let (text, meta) = result_of(&r.core, &sid, "e1");
    assert!(text.starts_with("Replaced 1 occurrence"), "it ran: {text}");
    assert!(!text.contains("Errors after this edit"), "{text}");
    assert!(meta.get("lsp").is_none(), "{meta}");
}

/// A server that never answers in time: the edit waits the bound and no
/// longer, on tokio's paused clock, and says the file is pending.
#[tokio::test(start_paused = true)]
async fn a_server_that_never_answers_costs_the_bound_and_is_pending() {
    let fake = FakeConfig {
        diagnostics: Diagnostics::Pull,
        slow_ms: 5_000,
        ..FakeConfig::default()
    };
    let r = rig(vec![], fake, |_, _| {});
    let board = r.core.tools.lsp.clone().unwrap();
    let a = r.work.join("a.fake");
    // Up, with no request answered yet (the fake's `initialize` is never
    // slow).
    let (spec, root) = board.server_for(&a).unwrap();
    board.live(&spec, &root).await.unwrap();
    std::fs::write(&a, "def total\nERROR new\n").unwrap();
    let t0 = tokio::time::Instant::now();
    let got = board
        .after_edit("tu_1", std::slice::from_ref(&a))
        .await
        .expect("a block");
    let waited = t0.elapsed();
    assert!(
        waited >= crate::lsp::edits::EDIT_WAIT
            && waited < crate::lsp::edits::EDIT_WAIT + Duration::from_millis(50),
        "waited {waited:?}"
    );
    assert!(
        got.text
            .contains(&format!("{}: pending: fake had not answered", a.display())),
        "{}",
        got.text
    );
    assert_eq!(got.meta["freshness"], "pending");
}

/// The frames: a tool loop whose edit carries the block writes no more
/// frames than the same loop without it, since the block rides in the
/// result node, in its completion's frame.
#[tokio::test]
async fn the_block_adds_no_frame() {
    let mut script = edit("e1", "a.fake", "let y = 2\n", "ERROR 2\n");
    script.extend(edit("e2", "a.fake", "ERROR 2\n", "let y = 2\n"));
    script.extend(edit("e3", "a.fake", "let y = 2\n", "ERROR 3\n"));
    let r = rig(script, FakeConfig::default(), |_, _| {});
    let sid = session(&r.core, None);
    turn(&r.core, &sid, "warm up").await;
    let frames = |r: &Rig| r.core.store.stats().unwrap().frames_appended;
    let before = frames(&r);
    turn(&r.core, &sid, "without").await;
    let without = frames(&r) - before;
    assert!(!result_of(&r.core, &sid, "e2").0.contains("Errors after"));
    open(&r, &["b.fake"]).await;
    let before = frames(&r);
    turn(&r.core, &sid, "with").await;
    let with = frames(&r) - before;
    assert!(result_of(&r.core, &sid, "e3")
        .0
        .contains("Errors after this edit"));
    assert!(
        with <= without,
        "with the block {with} frames, without {without}"
    );
}
