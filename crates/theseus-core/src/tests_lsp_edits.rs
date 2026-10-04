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
            prompt: None,
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
        .after_edit("s1", "tu_1", std::slice::from_ref(&a))
        .await
        .expect("a block");
    let waited = t0.elapsed();
    assert!(
        waited >= Duration::from_millis(1500) && waited < Duration::from_millis(1550),
        "waited {waited:?}"
    );
    assert!(
        got.text
            .contains(&format!("{}: pending: fake had not answered", a.display())),
        "{}",
        got.text
    );
    assert_eq!(got.meta["freshness"], "pending");
    // Another session's next result carries nothing of it.
    let ctx = r.core.tools.ctx.clone();
    let read = |s: &'static str| {
        let (board, ctx) = (board.clone(), ctx.clone());
        async move {
            board
                .attach(s, "tu_2", "lsp.hover", true, &json!({}), &ctx)
                .await
        }
    };
    assert!(read("s2").await.is_none());
    // Nothing yet: the wait goes on.
    assert!(read("s1").await.is_none());
    // The answer arrives (the fake's 5 s), and the session's next lsp
    // result carries it, once.
    tokio::time::sleep(Duration::from_secs(5)).await;
    let next = read("s1").await.expect("what arrived");
    assert!(
        next.text
            .contains("Diagnostics that arrived since an earlier edit:"),
        "{}",
        next.text
    );
    assert!(
        next.text
            .contains(&format!("{} (fake): 1 error", a.display())),
        "{}",
        next.text
    );
    assert_eq!(next.meta["arrived"]["freshness"], "pulled");
    assert!(read("s1").await.is_none(), "taken once");
}

/// A later edit of a pending file supersedes its wait: the next result
/// carries the new edit's own diagnostics, not the old wait's.
#[tokio::test(start_paused = true)]
async fn a_later_edit_supersedes_a_pending_wait() {
    let fake = FakeConfig {
        diagnostics: Diagnostics::Pull,
        slow_ms: 5_000,
        ..FakeConfig::default()
    };
    let r = rig(vec![], fake, |_, _| {});
    let board = r.core.tools.lsp.clone().unwrap();
    let a = r.work.join("a.fake");
    let (spec, root) = board.server_for(&a).unwrap();
    board.live(&spec, &root).await.unwrap();
    std::fs::write(&a, "def total\nERROR new\n").unwrap();
    let ctx = r.core.tools.ctx.clone();
    let meta = json!({"path": a});
    let first = board
        .attach("s1", "tu_1", "fs.write", true, &meta, &ctx)
        .await
        .unwrap();
    assert_eq!(first.meta["freshness"], "pending");
    std::fs::write(&a, "def total\n").unwrap();
    let second = board
        .attach("s1", "tu_2", "fs.write", true, &meta, &ctx)
        .await
        .unwrap();
    assert_eq!(second.meta["freshness"], "pending");
    assert!(second.meta.get("arrived").is_none(), "{}", second.meta);
    tokio::time::sleep(Duration::from_secs(6)).await;
    let next = board
        .attach("s1", "tu_3", "lsp.symbols", true, &json!({}), &ctx)
        .await
        .unwrap();
    assert!(next.text.contains("(fake): no errors"), "{}", next.text);
    assert_eq!(next.meta["arrived"]["files"].as_array().unwrap().len(), 1);
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
    // `open` started the server, whose `lsp.ready` row lands after the call
    // returns (`lsp::readiness`): count from a still store.
    let mut last = frames(&r);
    for _ in 0..50 {
        tokio::time::sleep(Duration::from_millis(100)).await;
        let now = frames(&r);
        if now == last {
            break;
        }
        last = now;
    }
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

/// `[lsp.servers.fake] start_on_edit`: with no server up, an edit's gate
/// judges the start at `proc.run`'s posture (here `approve`), naming it,
/// though not in a shared place; off, the edit keeps its own posture.
#[tokio::test]
async fn an_edit_that_starts_its_server_is_judged_as_proc_run() {
    use crate::places::PlaceClass;
    let decided = |r: &Rig, class: PlaceClass| {
        let rt = &r.core.tools;
        let tool = rt.registry.get("fs.edit").unwrap().clone();
        let input =
            json!({"path": r.work.join("a.fake"), "old_string": "let y", "new_string": "let z"});
        let plan = tool.plan(&input, &rt.ctx).unwrap();
        let d = rt.policy.decide_with(tool.as_ref(), &plan, None);
        crate::lsp::edits::gate(rt, class, tool.as_ref(), &plan, d)
    };
    let approve = |cfg: &mut Config| {
        cfg.policy.tools.insert("fs.edit".into(), Posture::Open);
        cfg.policy.tools.insert("proc.run".into(), Posture::Approve);
    };
    let on = rig(vec![], FakeConfig::default(), |cfg, _| {
        approve(cfg);
        cfg.lsp.servers.get_mut("fake").unwrap().start_on_edit = Some(true);
    });
    let d = decided(&on, PlaceClass::Private);
    assert_eq!(d.posture, Posture::Approve, "{}", d.reason);
    assert!(d.reason.contains("starts fake on"), "{}", d.reason);
    assert_eq!(decided(&on, PlaceClass::Shared).posture, Posture::Open);
    let off = rig(vec![], FakeConfig::default(), |cfg, _| approve(cfg));
    assert_eq!(decided(&off, PlaceClass::Private).posture, Posture::Open);
}

/// `start_on_edit` on, nothing up: the edit starts the server and carries
/// its block, and health's line counts the block.
#[tokio::test]
async fn start_on_edit_starts_the_server_and_health_counts_the_block() {
    let r = rig(
        edit("e1", "a.fake", "let y = 2\n", "ERROR 2\n"),
        FakeConfig::default(),
        |cfg, _| cfg.lsp.servers.get_mut("fake").unwrap().start_on_edit = Some(true),
    );
    let sid = session(&r.core, None);
    turn(&r.core, &sid, "edit").await;
    let (text, meta) = result_of(&r.core, &sid, "e1");
    assert!(text.contains("(fake): 2 errors"), "{text}");
    assert_eq!(meta["lsp"]["errors"], 2);
    assert_eq!(r.spawner.count(), 1);
    let health = r.core.tools.lsp.as_ref().unwrap().health();
    assert_eq!(health.len(), 1);
    assert_eq!(health[0].edit_blocks, 1);
}

/// `[lsp] edit_diagnostics = false`: a server up, and no block.
#[tokio::test]
async fn edit_diagnostics_off_is_no_block() {
    let r = rig(
        edit("e1", "a.fake", "let y = 2\n", "ERROR 2\n"),
        FakeConfig::default(),
        |cfg, _| cfg.lsp.edit_diagnostics = false,
    );
    open(&r, &["a.fake"]).await;
    let sid = session(&r.core, None);
    turn(&r.core, &sid, "edit").await;
    let (text, meta) = result_of(&r.core, &sid, "e1");
    assert!(!text.contains("Errors after this edit"), "{text}");
    assert!(meta.get("lsp").is_none(), "{meta}");
}

/// `[lsp] edit_wait_ms` is the bound, on tokio's paused clock.
#[tokio::test(start_paused = true)]
async fn edit_wait_ms_is_the_bound() {
    let fake = FakeConfig {
        diagnostics: Diagnostics::Pull,
        slow_ms: 5_000,
        ..FakeConfig::default()
    };
    let r = rig(vec![], fake, |cfg, _| cfg.lsp.edit_wait_ms = 300);
    let board = r.core.tools.lsp.clone().unwrap();
    let a = r.work.join("a.fake");
    let (spec, root) = board.server_for(&a).unwrap();
    board.live(&spec, &root).await.unwrap();
    let t0 = tokio::time::Instant::now();
    let got = board
        .after_edit("s1", "tu_1", std::slice::from_ref(&a))
        .await
        .unwrap();
    let waited = t0.elapsed();
    assert!(
        waited >= Duration::from_millis(300) && waited < Duration::from_millis(350),
        "waited {waited:?}"
    );
    assert!(got.text.contains("within 300 ms"), "{}", got.text);
}

/// Unset, `start_on_edit` is on for rust-analyzer, ty, and tsgo
/// (theseus-ext.12): an edit of each one's file starts it, and
/// rust-analyzer's `initialize` carries `cargo.targetDir`, so its checks
/// build apart from the agent's. Set false, an edit starts none.
#[tokio::test]
async fn the_three_presets_start_on_an_edit_and_false_stops_it() {
    let files = |work: &Path| {
        std::fs::write(work.join("Cargo.toml"), "[workspace]\n").unwrap();
        std::fs::write(work.join("pyproject.toml"), "").unwrap();
        std::fs::write(work.join("tsconfig.json"), "{}").unwrap();
        ["m.rs", "m.py", "m.ts"].map(|f| {
            std::fs::write(work.join(f), "ERROR here\n").unwrap();
            work.join(f)
        })
    };
    let quick = |cfg: &mut Config| {
        cfg.lsp.edit_wait_ms = 200;
        cfg.lsp.request_timeout_secs = 1;
    };
    let r = rig(vec![], FakeConfig::default(), |cfg, _| quick(cfg));
    let board = r.core.tools.lsp.clone().unwrap();
    for f in files(&r.work) {
        board.after_edit("s1", "tu_1", &[f]).await;
    }
    for _ in 0..100 {
        if board.up().len() >= 3 {
            break;
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    let mut names: Vec<String> = board.up().iter().map(|l| l.server().to_string()).collect();
    names.sort();
    assert_eq!(names, ["rust-analyzer", "tsgo", "ty"]);
    assert_eq!(r.spawner.count(), 3);
    let ra = board
        .up()
        .into_iter()
        .find(|l| l.server() == "rust-analyzer")
        .unwrap();
    let seen = ra.client.request("fake/seen", Value::Null).await.unwrap();
    assert_eq!(
        seen["initialization_options"],
        json!({"cargo": {"targetDir": true}}),
        "{seen}"
    );

    let off = rig(vec![], FakeConfig::default(), |cfg, _| {
        quick(cfg);
        cfg.lsp.servers.insert(
            "rust-analyzer".into(),
            crate::config::LspServerConfig {
                start_on_edit: Some(false),
                ..Default::default()
            },
        );
    });
    let board = off.core.tools.lsp.clone().unwrap();
    let [rs, ..] = files(&off.work);
    assert!(board.after_edit("s1", "tu_1", &[rs]).await.is_none());
    assert_eq!(off.spawner.count(), 0, "start_on_edit = false");
}

/// An `lsp.diagnostics` right after a pending edit lists the file's errors
/// once: what arrived for that file since is left out of its result, while
/// another file's still rides (theseus-ext.12).
#[tokio::test(start_paused = true)]
async fn lsp_diagnostics_after_a_pending_edit_lists_each_error_once() {
    let fake = FakeConfig {
        diagnostics: Diagnostics::Pull,
        slow_ms: 5_000,
        ..FakeConfig::default()
    };
    let r = rig(vec![], fake, |_, _| {});
    let board = r.core.tools.lsp.clone().unwrap();
    let (a, b) = (r.work.join("a.fake"), r.work.join("b.fake"));
    let (spec, root) = board.server_for(&a).unwrap();
    board.live(&spec, &root).await.unwrap();
    std::fs::write(&a, "def total\nERROR new\n").unwrap();
    std::fs::write(&b, "use total\nERROR too\n").unwrap();
    for f in [&a, &b] {
        let got = board
            .after_edit("s1", "tu_1", std::slice::from_ref(f))
            .await
            .unwrap();
        assert_eq!(got.meta["freshness"], "pending");
    }
    // Both waits get their answers.
    tokio::time::sleep(Duration::from_secs(12)).await;
    let ctx = r.core.tools.ctx.clone();
    let tool = r
        .core
        .tools
        .registry
        .get("lsp.diagnostics")
        .unwrap()
        .clone();
    let (out, _) = tool.run_async(&json!({"path": a}), &ctx).await.unwrap();
    assert_eq!(out.text.matches("planted error").count(), 1, "{}", out.text);
    let extra = board
        .attach("s1", "tu_3", "lsp.diagnostics", true, &out.meta, &ctx)
        .await
        .expect("b's arrival rides");
    assert!(
        extra
            .text
            .contains(&format!("{} (fake): 1 error", b.display())),
        "{}",
        extra.text
    );
    assert!(
        !extra.text.contains(&a.display().to_string()),
        "a is listed by the result itself: {}",
        extra.text
    );
    assert_eq!(extra.meta["arrived"]["files"].as_array().unwrap().len(), 1);
}
