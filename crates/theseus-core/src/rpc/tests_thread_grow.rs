//! A thread's working set grows from its own turns (theseus-98bz.5), through
//! whole cores with a scripted model: a turn in a thread adds the files its
//! calls wrote (and their first directory) and the tickets its message named,
//! so the router's next pre-route finds the thread by them; an unchanged set
//! writes no row; a home turn grows no thread; the switch off writes nothing.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use serde_json::{json, Value};
use theseus_protocol::threads::{ThreadInfo, ThreadNewParams, WorkingSet};
use theseus_protocol::{method, Id, Message, Request, TurnSubmitParams, TurnSubmitResult};
use tokio::io::{duplex, AsyncBufReadExt, AsyncWriteExt, BufReader};

use super::Core;
use crate::approval::{Answerer, Client, Surface};
use crate::ledger::LedgerRow;
use crate::policy::Posture;
use crate::provider::{FakeProvider, Scripted};
use crate::rpc::Parts;
use crate::store::Store;
use crate::Config;

pub(super) struct Rig {
    pub(super) core: Arc<Core>,
    root: PathBuf,
    _dir: tempfile::TempDir,
}

pub(super) fn rig(
    script: impl FnOnce(&Path) -> Vec<Scripted>,
    tweak: impl FnOnce(&mut Config),
) -> Rig {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().join("work");
    std::fs::create_dir_all(root.join("tarnlib/reviews")).unwrap();
    let root = root.canonicalize().unwrap();
    let mut cfg = Config::example();
    cfg.server.state_dir = dir.path().join("state").to_string_lossy().into_owned();
    cfg.tools.projects_dir = Some(root.to_string_lossy().into_owned());
    cfg.tools.cwd = Some(root.to_string_lossy().into_owned());
    cfg.tools.roots = vec![];
    cfg.policy.enforcement = Posture::Notify;
    cfg.policy.tools.remove("fs.write");
    cfg.judge.enabled = false;
    tweak(&mut cfg);
    let store = Store::open(&dir.path().join("store")).unwrap();
    let fake = Arc::new(FakeProvider::scripted(script(&root)));
    let core = Core::build(Parts::for_tests(cfg, fake, store)).unwrap();
    Rig {
        core,
        root,
        _dir: dir,
    }
}

pub(super) fn cli() -> Answerer {
    Answerer {
        label: "the CLI".into(),
        surface: Surface::Cli,
        discord: None,
        agent: None,
    }
}

pub(super) fn thread(c: &Core, slug: &str) {
    c.thread_new(
        &ThreadNewParams {
            slug: slug.into(),
            title: Some(slug.replace('-', " ")),
            ..Default::default()
        },
        cli(),
    )
    .unwrap();
}

fn set_of(c: &Core, slug: &str) -> WorkingSet {
    c.threads().get(slug).unwrap().working_set.clone()
}

fn edits(c: &Core) -> Vec<LedgerRow> {
    crate::test_support::ledger_rows_2k(c, "thread.edited")
}

/// The scripted model writes `rel` under the root, then answers.
fn writes(root: &Path, rel: &str) -> Vec<Scripted> {
    let path = root.join(rel).display().to_string();
    vec![
        Scripted::tools(
            "Writing it.",
            &[(
                "w1",
                "fs_write",
                json!({"path": path, "content": "Looks good.\n"}),
            )],
        ),
        Scripted::text("Written."),
    ]
}

impl Rig {
    /// A place's own session (the CLI's, no place).
    pub(super) fn home(&self) -> String {
        self.core
            .open_session(theseus_protocol::SessionOpenParams::default())
            .unwrap()
            .session_id
    }
}

/// One `turn.submit` from the CLI, in a thread or a session.
async fn submit(
    c: &Arc<Core>,
    thread: Option<&str>,
    session: Option<&str>,
    input: &str,
) -> TurnSubmitResult {
    let p = TurnSubmitParams {
        thread: thread.map(str::to_string),
        session_id: session.map(str::to_string),
        input: input.into(),
        ..TurnSubmitParams::default()
    };
    let v = send(c, p).await;
    serde_json::from_value(v["result"].clone()).unwrap_or_else(|_| panic!("a turn result in {v}"))
}

/// One `turn.submit` from the CLI: its whole response.
pub(super) async fn send(c: &Arc<Core>, p: TurnSubmitParams) -> Value {
    send_as(c, Surface::Cli, p).await
}

/// One `turn.submit` from a connection of `surface`: its whole response.
pub(super) async fn send_as(c: &Arc<Core>, surface: Surface, p: TurnSubmitParams) -> Value {
    let (client, server) = duplex(1 << 20);
    let (sr, sw) = tokio::io::split(server);
    tokio::spawn(
        c.clone()
            .serve_connection(sr, sw, Client::new("test", surface)),
    );
    let (cr, mut cw) = tokio::io::split(client);
    let mut line =
        serde_json::to_string(&Request::new(Id::Num(1), method::TURN_SUBMIT, p)).unwrap();
    line.push('\n');
    cw.write_all(line.as_bytes()).await.unwrap();
    let mut lines = BufReader::new(cr).lines();
    while let Some(l) = lines.next_line().await.unwrap() {
        if let Message::Response(r) = serde_json::from_str(&l).unwrap() {
            return serde_json::to_value(r).unwrap();
        }
    }
    panic!("no response");
}

/// The key test: a turn in `pr-round` whose model writes
/// `tarnlib/reviews/comment-1.md`, its message naming PRR-4142, leaves the
/// thread holding the root, the file and the ticket, in one `thread.edited`
/// row by the harness; the pre-router then finds the thread by "comment-2.md
/// under tarnlib/reviews". A second turn that adds nothing writes no row.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_threads_turn_grows_its_working_set() {
    let r = rig(
        |root| {
            let mut s = writes(root, "tarnlib/reviews/comment-1.md");
            s.push(Scripted::text("Nothing to add."));
            s
        },
        |_| {},
    );
    let c = &r.core;
    thread(c, "pr-round");
    assert!(set_of(c, "pr-round").is_empty());
    let t0 = std::time::Instant::now();
    let res = submit(
        c,
        Some("pr-round"),
        None,
        "answer the first review comment on PRR-4142",
    )
    .await;
    let took = t0.elapsed();
    assert!(r.root.join("tarnlib/reviews/comment-1.md").exists());
    let w = set_of(c, "pr-round");
    assert_eq!(w.roots, ["tarnlib"]);
    assert_eq!(w.files, ["tarnlib/reviews/comment-1.md"]);
    assert_eq!(w.refs, ["PRR-4142"]);
    let rows = edits(c);
    assert_eq!(rows.len(), 1, "one row for the growth");
    assert_eq!(rows[0].data["changed"], json!(["working_set"]));
    assert_eq!(rows[0].data["who"], "harness");
    assert_eq!(rows[0].turn_id.as_deref(), Some(res.turn_id.as_str()));
    let t: ThreadInfo = c.threads().get("pr-round").unwrap().clone();
    let pre = crate::thread_route::pre_route(
        "comment-2.md under tarnlib/reviews",
        &[&t],
        None,
        theseus_protocol::now_unix_ms(),
        600_000,
    );
    assert!(!pre[0].hits.is_empty(), "an entity hit: {pre:?}");
    assert!(pre[0].entity > 0.0);
    eprintln!("a turn that grew its thread took {took:?}, the growth included");
    submit(c, Some("pr-round"), None, "and PRR-4142 again").await;
    assert_eq!(edits(c).len(), 1, "an unchanged set writes no row");
    assert_eq!(set_of(c, "pr-round"), w);
}

/// A turn at home (a session no thread runs on) grows no thread.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_home_turn_grows_no_thread() {
    let r = rig(|root| writes(root, "tarnlib/reviews/comment-1.md"), |_| {});
    let c = &r.core;
    thread(c, "pr-round");
    let sid = c
        .open_session(theseus_protocol::SessionOpenParams::default())
        .unwrap()
        .session_id;
    submit(
        c,
        None,
        Some(&sid),
        "answer the first review comment on PRR-4142",
    )
    .await;
    assert!(set_of(c, "pr-round").is_empty());
    assert!(edits(c).is_empty());
}

/// `[threads] working_set_grows = false`: the same turn writes no row, and
/// the thread's record is byte for byte what it was.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn with_the_switch_off_no_set_grows() {
    let r = rig(
        |root| writes(root, "tarnlib/reviews/comment-1.md"),
        |cfg| cfg.threads.working_set_grows = false,
    );
    let c = &r.core;
    thread(c, "pr-round");
    let before: Value = c.store.get_meta("thread:pr-round").unwrap().unwrap();
    submit(
        c,
        Some("pr-round"),
        None,
        "answer the first review comment on PRR-4142",
    )
    .await;
    assert!(r.root.join("tarnlib/reviews/comment-1.md").exists());
    let after: Value = c.store.get_meta("thread:pr-round").unwrap().unwrap();
    assert_eq!(after, before);
    assert!(edits(c).is_empty());
}
