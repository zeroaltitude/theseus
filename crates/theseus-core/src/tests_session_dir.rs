//! A session works in the directory it was started in (theseus-aab7):
//! through the protocol, as `theseus ask` sends it. Its tools follow it
//! (`proc_run ["pwd"]` prints it, `fs_read a.txt` reads it from there), a
//! resumed session keeps it and `dir` moves it, a client that sends none
//! gets `[tools] cwd`, and a directory outside the roots changes no policy.

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use serde_json::{json, Value};
use theseus_protocol::{method, Id, Message, Request, TurnSubmitResult};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};

use crate::node::Body;
use crate::policy::Posture;
use crate::provider::{FakeProvider, Scripted};
use crate::session::SessionRecord;
use crate::store::Store;
use crate::{Config, Core};

struct Rig {
    core: Arc<Core>,
    fake: Arc<FakeProvider>,
    /// The workspace's one root, canonical.
    root: PathBuf,
    dir: tempfile::TempDir,
}

/// A deployment whose `proc.run` runs with a notice, in process, with one
/// root, `<tmp>/work`, which is `[tools] cwd` too.
fn rig(script: Vec<Scripted>) -> Rig {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().join("work");
    std::fs::create_dir_all(&root).unwrap();
    let root = root.canonicalize().unwrap();
    let mut cfg = Config::example();
    cfg.server.state_dir = dir.path().join("state").to_string_lossy().into_owned();
    cfg.tools.projects_dir = Some(root.to_string_lossy().into_owned());
    cfg.tools.roots = vec![];
    cfg.tools.proc_sync_secs = 10;
    cfg.policy.enforcement = Posture::Notify;
    cfg.policy.tools.remove("proc.run");
    let store = Store::open(&dir.path().join("store")).unwrap();
    let fake = Arc::new(FakeProvider::scripted(script));
    let core = Core::build(crate::rpc::Parts::for_tests(cfg, fake.clone(), store)).unwrap();
    Rig {
        core,
        fake,
        root,
        dir,
    }
}

/// A project directory under `base`, with `a.txt` in it.
fn project(base: &Path, name: &str) -> PathBuf {
    let p = base.join(name);
    std::fs::create_dir_all(&p).unwrap();
    std::fs::write(p.join("a.txt"), format!("the tide table of {name}\n")).unwrap();
    p.canonicalize().unwrap()
}

/// One `turn.submit` over a protocol connection, as a client sends it.
async fn submit(core: &Arc<Core>, params: Value) -> TurnSubmitResult {
    let (client, server) = tokio::io::duplex(1 << 20);
    let (sr, sw) = tokio::io::split(server);
    let srv = tokio::spawn(core.clone().serve_connection(sr, sw, "cli".into()));
    let (cr, mut cw) = tokio::io::split(client);
    let mut lines = BufReader::new(cr).lines();
    let req = Request::new(Id::Num(1), method::TURN_SUBMIT, params);
    let mut line = serde_json::to_string(&req).unwrap();
    line.push('\n');
    cw.write_all(line.as_bytes()).await.unwrap();
    let r = tokio::time::timeout(Duration::from_secs(60), async {
        loop {
            let l = lines.next_line().await.unwrap().unwrap();
            if let Message::Response(r) = serde_json::from_str::<Message>(&l).unwrap() {
                break r;
            }
        }
    })
    .await
    .expect("turn.submit answered");
    cw.shutdown().await.unwrap();
    drop((cw, lines));
    let _ = srv.await;
    assert!(r.error.is_none(), "{:?}", r.error);
    serde_json::from_value(r.result.unwrap()).unwrap()
}

fn record(core: &Core, sid: &str) -> SessionRecord {
    core.store.get_session(sid).unwrap().unwrap()
}

/// The text of the result the session wrote for the call `id`.
fn result_of(core: &Core, sid: &str, id: &str) -> String {
    core.store
        .transcript(sid)
        .unwrap()
        .iter()
        .find_map(|(_, n)| match &n.body {
            Body::ToolResult {
                tool_use_id,
                content,
                ..
            } if tool_use_id == id => Some(content.clone()),
            _ => None,
        })
        .unwrap_or_else(|| panic!("no result for {id}"))
}

fn pwd(id: &str) -> Scripted {
    Scripted::tools("", &[(id, "proc_run", json!({"argv": ["pwd"]}))])
}

/// A session started in X works in X: `proc_run ["pwd"]` prints X, and
/// `fs_read a.txt` reads X's `a.txt`, not the root's.
#[tokio::test]
async fn a_session_started_in_a_directory_works_there() {
    let r = rig(vec![
        pwd("t1"),
        Scripted::tools("", &[("t2", "fs_read", json!({"path": "a.txt"}))]),
        Scripted::text("Done."),
    ]);
    std::fs::write(r.root.join("a.txt"), "the root's own\n").unwrap();
    let x = project(&r.root, "harbour-tides");
    let res = submit(
        &r.core,
        json!({"input": "where are we", "dir": x.to_string_lossy()}),
    )
    .await;
    let sid = &res.session_id;
    assert_eq!(record(&r.core, sid).dir.as_deref(), x.to_str());
    assert!(res.outside_roots.is_none(), "inside the root: no notice");
    let printed = result_of(&r.core, sid, "t1");
    assert!(
        printed.contains(&format!("{}\n", x.display())),
        "pwd ran in X: {printed}"
    );
    let read = result_of(&r.core, sid, "t2");
    assert!(read.contains("the tide table of harbour-tides"), "{read}");
    assert_eq!(r.fake.requests().len(), 3, "a loop a call, and the answer");
}

/// A directory outside the workspace roots changes no policy: the run
/// there waits for the owner's approval, as a run outside the roots always
/// did, and the answer names the roots for the CLI's one notice.
#[tokio::test]
async fn a_directory_outside_the_roots_waits_as_today() {
    let r = rig(vec![pwd("t1"), Scripted::text("Done.")]);
    let elsewhere = project(r.dir.path(), "elsewhere");
    let res = submit(
        &r.core,
        json!({"input": "where are we", "dir": elsewhere.to_string_lossy()}),
    )
    .await;
    assert!(
        res.awaiting_confirm.is_some(),
        "the run outside the roots waits: {res:?}"
    );
    assert_eq!(res.outside_roots, Some(vec![r.root.display().to_string()]));
}

/// `ask -s <id>` keeps the session's directory: a turn that names none runs
/// where the session was created; one that names another moves it there,
/// for good.
#[tokio::test]
async fn a_resumed_session_keeps_its_directory_and_dir_moves_it() {
    let r = rig(vec![
        Scripted::text("hello"),
        pwd("t1"),
        Scripted::text("Still here."),
        pwd("t2"),
        Scripted::text("Moved."),
    ]);
    let a = project(&r.root, "harbour-tides");
    let b = project(&r.root, "harbour-gauges");
    let first = submit(&r.core, json!({"input": "hi", "dir": a.to_string_lossy()})).await;
    let sid = first.session_id.clone();
    let kept = submit(&r.core, json!({"input": "where", "session_id": sid})).await;
    assert!(kept.outside_roots.is_none());
    assert_eq!(record(&r.core, &sid).dir.as_deref(), a.to_str());
    assert!(result_of(&r.core, &sid, "t1").contains(&format!("{}\n", a.display())));
    submit(
        &r.core,
        json!({"input": "move", "session_id": sid, "dir": b.to_string_lossy()}),
    )
    .await;
    assert_eq!(record(&r.core, &sid).dir.as_deref(), b.to_str());
    assert!(result_of(&r.core, &sid, "t2").contains(&format!("{}\n", b.display())));
}

/// A client that sends no directory (Discord, the cockpit) gets today's
/// `[tools] cwd`: the session stores none, and `pwd` prints the root.
#[tokio::test]
async fn a_client_that_sends_no_directory_gets_todays_cwd() {
    let r = rig(vec![pwd("t1"), Scripted::text("Done.")]);
    let res = submit(&r.core, json!({"input": "where are we"})).await;
    let sid = &res.session_id;
    assert_eq!(record(&r.core, sid).dir, None);
    assert!(res.outside_roots.is_none());
    let printed = result_of(&r.core, sid, "t1");
    assert!(
        printed.contains(&format!("{}\n", r.root.display())),
        "{printed}"
    );
}

/// A relative `dir` is refused as invalid params, before any session opens.
#[tokio::test]
async fn a_relative_directory_is_refused() {
    let r = rig(vec![]);
    let (client, server) = tokio::io::duplex(1 << 16);
    let (sr, sw) = tokio::io::split(server);
    let srv = tokio::spawn(r.core.clone().serve_connection(sr, sw, "cli".into()));
    let (cr, mut cw) = tokio::io::split(client);
    let mut lines = BufReader::new(cr).lines();
    let req = Request::new(
        Id::Num(1),
        method::TURN_SUBMIT,
        json!({"input": "hi", "dir": "harbour-tides"}),
    );
    let mut line = serde_json::to_string(&req).unwrap();
    line.push('\n');
    cw.write_all(line.as_bytes()).await.unwrap();
    let err = loop {
        let l = lines.next_line().await.unwrap().unwrap();
        if let Message::Response(r) = serde_json::from_str::<Message>(&l).unwrap() {
            break r.error.expect("refused");
        }
    };
    assert_eq!(err.code, theseus_protocol::error_code::INVALID_PARAMS);
    assert!(err.message.contains("absolute"), "{}", err.message);
    cw.shutdown().await.unwrap();
    drop((cw, lines));
    let _ = srv.await;
}
