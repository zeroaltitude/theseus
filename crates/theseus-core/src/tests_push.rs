//! The push in the core (theseus-in3): the snapshot and the events agree
//! under the position rule however the turns race the watch, nothing is
//! observed until someone watches, and the observer adds no frame (9b);
//! `session.wait` returns on blocked, settled, and terminal, and a client
//! that stops reading hears what it lost and catches up (9c).

use std::collections::BTreeMap;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use serde_json::{json, Value};
use theseus_protocol::{
    error_code, method, notify, ExecutionView, ExecutionsWatchResult, Id, Message, Notification,
    Request, Response, SessionKind, SessionOpenParams, TurnSubmitResult,
};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader, DuplexStream, ReadHalf, WriteHalf};

use crate::bus::EventSink;
use crate::provider::{FakeProvider, Scripted};
use crate::session::SessionRecord;
use crate::store::Store;
use crate::turn::TurnRequest;
use crate::{Config, Core};

struct Rig {
    core: Arc<Core>,
    _dir: tempfile::TempDir,
}

fn rig() -> Rig {
    rig_full(vec![], false, false)
}

/// A rig whose model answers `script`, then text; with `approve`, every call
/// that acts waits for the operator; with `unsynced`, no frame is synced.
fn rig_full(script: Vec<Scripted>, approve: bool, unsynced: bool) -> Rig {
    let dir = tempfile::tempdir().unwrap();
    let mut cfg = Config::example();
    cfg.server.state_dir = dir.path().to_string_lossy().into_owned();
    let root = dir.path().join("work");
    std::fs::create_dir_all(&root).unwrap();
    cfg.tools.projects_dir = Some(root.canonicalize().unwrap().to_string_lossy().into_owned());
    cfg.tools.roots = vec![];
    if approve {
        cfg.policy.enforcement = crate::policy::Posture::Approve;
    }
    let store = if unsynced {
        Store::open_unsynced(&dir.path().join("store")).unwrap()
    } else {
        Store::open(&dir.path().join("store")).unwrap()
    };
    let core = Core::build(crate::rpc::Parts::for_tests(
        cfg,
        Arc::new(FakeProvider::scripted(script)),
        store,
    ))
    .unwrap();
    Rig { core, _dir: dir }
}

/// One plain turn, on `session` or a new conversation.
async fn turn(core: &Arc<Core>, session: Option<&str>) -> String {
    turn_result(core, session).await.session_id
}

/// One turn, and its result.
async fn turn_result(core: &Arc<Core>, session: Option<&str>) -> TurnSubmitResult {
    let rec = match session {
        Some(id) => core
            .store
            .get_session::<SessionRecord>(id)
            .unwrap()
            .unwrap(),
        None => {
            let r = SessionRecord::new(SessionKind::Conversation, None);
            core.store.put_session(&r.session_id, &r).unwrap();
            r
        }
    };
    let (live, _) = core.live_profile();
    let target = core.runner.resolve_target(&live, None, None, None).unwrap();
    let sink = EventSink::new(core.bus.clone(), &rec.session_id, None);
    core.runner
        .run(TurnRequest {
            session: rec,
            input: Some("go".into()),
            target,
            sink,
            author: "test".into(),
            recompile: None,
            attachments: vec![],
            arrived: None,
            reply_to: None,
        })
        .await
        .unwrap()
}

/// A client on its own connection: it sends `executions.watch` and keeps
/// every message it gets.
async fn watcher(core: &Arc<Core>) -> Arc<Mutex<Vec<Message>>> {
    let (client, server) = tokio::io::duplex(1 << 20);
    let (sr, sw) = tokio::io::split(server);
    tokio::spawn(core.clone().serve_connection(sr, sw, "watcher".into()));
    let (cr, mut cw) = tokio::io::split(client);
    let req = Request::new(
        Id::Num(1),
        theseus_protocol::method::EXECUTIONS_WATCH,
        json!({}),
    );
    let mut line = serde_json::to_string(&req).unwrap();
    line.push('\n');
    cw.write_all(line.as_bytes()).await.unwrap();
    let got: Arc<Mutex<Vec<Message>>> = Arc::default();
    let into = got.clone();
    tokio::spawn(async move {
        let _keep = cw;
        let mut lines = BufReader::new(cr).lines();
        while let Ok(Some(l)) = lines.next_line().await {
            if let Ok(m) = serde_json::from_str::<Message>(&l) {
                into.lock().unwrap().push(m);
            }
        }
    });
    got
}

/// What a client holds after applying `got` by the position rule: each
/// execution's view with the greatest position, from the snapshot or an
/// event, in whatever order they came.
fn applied(got: &[Message]) -> BTreeMap<String, ExecutionView> {
    let mut views: BTreeMap<String, ExecutionView> = BTreeMap::new();
    let mut apply = |v: ExecutionView| {
        if views
            .get(&v.execution_id)
            .is_none_or(|old| v.position > old.position)
        {
            views.insert(v.execution_id.clone(), v);
        }
    };
    for m in got {
        match m {
            Message::Response(r) if r.id == Id::Num(1) => {
                let snap: ExecutionsWatchResult =
                    serde_json::from_value(r.result.clone().unwrap()).unwrap();
                snap.executions.into_iter().for_each(&mut apply);
            }
            Message::Notification(n) if n.method == theseus_protocol::notify::EXECUTION_CHANGED => {
                apply(serde_json::from_value(n.params.clone()).unwrap())
            }
            _ => {}
        }
    }
    views
}

/// The highest position of any execution or action record: the board has
/// applied every frame once its position reaches it.
fn last_kernel_position(core: &Core) -> u64 {
    let e = core.kernel.executions_at().unwrap();
    let a = core.kernel.actions_at().unwrap();
    e.iter()
        .map(|(p, _)| *p)
        .chain(a.iter().map(|(p, _)| *p))
        .max()
        .unwrap_or(0)
}

/// Turns race a client's `executions.watch` on four threads: whatever came
/// first, the snapshot or an event, the client that applies each view only
/// if its position is greater holds exactly the board's views at the end.
/// And nothing was observed until the watch came.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn the_snapshot_and_the_events_agree_under_the_position_rule() {
    let r = rig();
    let mut first = String::new();
    for _ in 0..4 {
        first = turn(&r.core, None).await;
    }
    assert!(
        !r.core.kernel.observed(),
        "no observer until someone watches"
    );
    assert!(!r.core.push.seeded());
    // Sixteen new conversations' turns at once; the watch lands among them.
    let mut turns = Vec::new();
    for i in 0..16 {
        let core = r.core.clone();
        turns.push(tokio::spawn(async move { turn(&core, None).await }));
        if i == 5 {
            tokio::task::yield_now().await;
        }
    }
    let got = watcher(&r.core).await;
    for t in turns {
        t.await.unwrap();
    }
    assert!(r.core.kernel.observed());
    // The observer hands each frame on as its commit returns, on the
    // committing thread, so the board applies frames in the order their
    // commits returned, not by position: under load a frame of one turn,
    // at a lower position, can follow another turn's frame at the highest
    // (theseus-amr2), and a board at the highest position has not yet
    // applied it. One more turn, after every turn above has returned, on a
    // session that has one, commits last: once the board has applied its
    // last frame, which is the highest, it has applied every frame before.
    turn(&r.core, Some(&first)).await;
    let last = last_kernel_position(&r.core);
    let mut feed = r.core.push.feed();
    tokio::time::timeout(Duration::from_secs(5), feed.wait_for(|p| *p >= last))
        .await
        .expect("the board caught up within 5 s")
        .expect("the board's feed is open");
    // The board's own views, as a fresh client would get them.
    let (_, board, total) = r.core.push.snapshot(usize::MAX);
    assert_eq!(total as usize, board.len());
    assert_eq!(board.len(), 4 + 16, "four sessions, and sixteen new ones");
    let mut client = BTreeMap::new();
    for _ in 0..500 {
        client = applied(&got.lock().unwrap());
        if client.len() == board.len()
            && board
                .iter()
                .all(|v| client.get(&v.execution_id).map(|c| c.position) == Some(v.position))
        {
            break;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    for v in &board {
        let c = client
            .get(&v.execution_id)
            .unwrap_or_else(|| panic!("the client lacks {}", v.execution_id));
        assert_eq!(
            (c.position, &c.state, &c.attention.label),
            (v.position, &v.state, &v.attention.label),
            "{}",
            v.execution_id
        );
        assert_eq!(v.attention.label, "ready", "every turn ended");
    }
    let snapshot: Value = got
        .lock()
        .unwrap()
        .iter()
        .find_map(|m| match m {
            Message::Response(r) if r.id == Id::Num(1) => r.result.clone(),
            _ => None,
        })
        .unwrap();
    assert!(snapshot["position"].as_u64().unwrap() > 0);
}

/// The observer adds no frame: with the board seeded and a client watching,
/// a plain turn still writes 5 frames, and the board sees the turn.
#[tokio::test]
async fn a_plain_turn_keeps_its_frame_budget_while_the_push_watches() {
    let r = rig();
    let sid = turn(&r.core, None).await;
    let got = watcher(&r.core).await;
    for _ in 0..200 {
        if r.core.push.seeded() && r.core.bus.all_watchers() == 1 {
            break;
        }
        tokio::time::sleep(Duration::from_millis(5)).await;
    }
    assert!(r.core.kernel.observed());
    let before = r.core.store.stats().unwrap().frames_appended;
    turn(&r.core, Some(&sid)).await;
    let frames = r.core.store.stats().unwrap().frames_appended - before;
    assert!(frames <= 5, "a plain turn wrote {frames} frames");
    let last = last_kernel_position(&r.core);
    for _ in 0..200 {
        let seen = applied(&got.lock().unwrap());
        if seen.values().any(|v| v.position >= last) {
            break;
        }
        tokio::time::sleep(Duration::from_millis(5)).await;
    }
    let seen = applied(&got.lock().unwrap());
    let v = seen.values().find(|v| v.session_id == sid).unwrap();
    assert!(
        v.position >= last,
        "the turn's last frame ends at or after its execution record: {} < {last}",
        v.position
    );
    assert_eq!((v.state.as_str(), v.turns), ("waiting", 2));
    let changed = got
        .lock()
        .unwrap()
        .iter()
        .filter(|m| matches!(m, Message::Notification(n) if n.method == theseus_protocol::notify::EXECUTION_CHANGED))
        .count();
    assert!(
        (2..=5).contains(&changed),
        "queued+running, then waiting, and spend: {changed}"
    );
}

// ---------------------------------------------------------------- 9c

/// A client on its own connection that reads only when told to, so a test
/// can stop reading.
struct Raw {
    lines: tokio::io::Lines<BufReader<ReadHalf<DuplexStream>>>,
    w: WriteHalf<DuplexStream>,
    /// Every notification read so far, in order.
    notes: Vec<Notification>,
}

impl Raw {
    /// A connection whose pipe holds `buffer` bytes.
    fn connect(core: &Arc<Core>, name: &str, buffer: usize) -> Raw {
        let (client, server) = tokio::io::duplex(buffer);
        let (sr, sw) = tokio::io::split(server);
        tokio::spawn(core.clone().serve_connection(sr, sw, name.into()));
        let (cr, w) = tokio::io::split(client);
        Raw {
            lines: BufReader::new(cr).lines(),
            w,
            notes: Vec::new(),
        }
    }

    async fn send(&mut self, id: u64, m: &str, params: Value) {
        let mut line = serde_json::to_string(&Request::new(Id::Num(id), m, params)).unwrap();
        line.push('\n');
        self.w.write_all(line.as_bytes()).await.unwrap();
    }

    /// Read until the answer to `id`, keeping the notifications on the way.
    async fn answer(&mut self, id: u64) -> Response {
        let read = async {
            loop {
                let line = self.lines.next_line().await.unwrap().expect("open");
                match serde_json::from_str::<Message>(&line).unwrap() {
                    Message::Response(r) if r.id == Id::Num(id) => return r,
                    Message::Notification(n) => self.notes.push(n),
                    _ => {}
                }
            }
        };
        tokio::time::timeout(Duration::from_secs(30), read)
            .await
            .unwrap_or_else(|_| panic!("no answer to {id} in 30 s"))
    }

    async fn call(&mut self, id: u64, m: &str, params: Value) -> Response {
        self.send(id, m, params).await;
        self.answer(id).await
    }

    /// Read until an `execution.changed` view satisfies `f`, keeping every
    /// notification on the way; None if none comes in 30 s.
    async fn until_view(&mut self, f: impl Fn(&ExecutionView) -> bool) -> Option<ExecutionView> {
        let read = async {
            loop {
                let line = self.lines.next_line().await.unwrap().expect("open");
                if let Message::Notification(n) = serde_json::from_str::<Message>(&line).unwrap() {
                    self.notes.push(n.clone());
                    if n.method == notify::EXECUTION_CHANGED {
                        let v: ExecutionView = serde_json::from_value(n.params).unwrap();
                        if f(&v) {
                            return v;
                        }
                    }
                }
            }
        };
        tokio::time::timeout(Duration::from_secs(30), read)
            .await
            .ok()
    }
}

/// Until `f` holds, polled every 5 ms for at most 10 s.
async fn until(what: &str, mut f: impl FnMut() -> bool) {
    for _ in 0..2000 {
        if f() {
            return;
        }
        tokio::time::sleep(Duration::from_millis(5)).await;
    }
    panic!("not {what} in 10 s");
}

/// `session.wait` returns on blocked, settled, and terminal; a wait already
/// satisfied answers at once; `after_position` makes the current view not
/// count; a wait times out; `terminal` is refused for a conversation; and a
/// wait for a session that does not exist, or with no params, fails at once.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[expect(clippy::too_many_lines, reason = "shape budget: split it")]
async fn session_wait_returns_on_blocked_settled_and_terminal() {
    let r = rig_full(
        vec![
            Scripted::text("Hello."),
            Scripted::tools(
                "",
                &[("t1", "fs_write", json!({"path": "a.txt", "content": "a"}))],
            ),
            Scripted::text("Not written, then."),
        ],
        true,
        false,
    );
    let sid = turn(&r.core, None).await;
    let mut c = Raw::connect(&r.core, "waiter", 1 << 20);

    // Already settled: at once.
    let a = c
        .call(
            1,
            method::SESSION_WAIT,
            json!({"session_id": sid, "until": "settled"}),
        )
        .await;
    let a = a.result.unwrap();
    assert_eq!(
        (a["reached"].as_str(), a["already"].as_bool()),
        (Some("settled"), Some(true))
    );
    let ready_at = a["execution"]["position"].as_u64().unwrap();

    // Blocked: parked, then woken by the turn that asks.
    c.send(
        2,
        method::SESSION_WAIT,
        json!({"session_id": sid, "until": "blocked", "timeout_ms": 20_000}),
    )
    .await;
    until("the wait parked", || r.core.push.status(0).waiting == 1).await;
    let asked = turn_result(&r.core, Some(&sid)).await;
    let corr = asked.awaiting_confirm.clone().unwrap();
    let b = c.answer(2).await.result.unwrap();
    assert_eq!(
        (b["reached"].as_str(), b["already"].as_bool()),
        (Some("blocked"), Some(false))
    );
    assert_eq!(b["execution"]["attention"]["level"], "needs_you");
    assert_eq!(b["confirms"][0]["correlation_id"], corr.as_str());
    let blocked_at = b["execution"]["position"].as_u64().unwrap();
    assert!(blocked_at > ready_at);

    // Needs you is settled too: the parked session is settled at once.
    let p = c
        .call(
            9,
            method::SESSION_WAIT,
            json!({"session_id": sid, "until": "settled"}),
        )
        .await
        .result
        .unwrap();
    assert_eq!(
        (
            p["already"].as_bool(),
            p["execution"]["attention"]["level"].as_str()
        ),
        (Some(true), Some("needs_you"))
    );
    let parked_at = p["execution"]["position"].as_u64().unwrap();
    assert!(parked_at >= blocked_at);
    // The answer queues it (no driver runs here, so it stays queued: working);
    // a wait for settled after that view parks, and the continuation's end
    // wakes it. (A wait parked before the answer does too: the answer is one
    // frame, theseus-jj9f, in the test after this one.)
    let exec = asked.execution_id.clone().unwrap();
    r.core.confirm_action(&corr, false, None, "test").unwrap();
    until("the board saw the answer queue it", || {
        r.core
            .push
            .view_of_session(&sid)
            .is_some_and(|v| v.state == "queued")
    })
    .await;
    let queued_at = r.core.push.view_of_session(&sid).unwrap().position;
    assert!(queued_at > parked_at);
    c.send(
        3,
        method::SESSION_WAIT,
        json!({"session_id": sid, "until": "settled", "after_position": queued_at, "timeout_ms": 20_000}),
    )
    .await;
    until("the second wait parked", || {
        r.core.push.status(0).waiting == 1
    })
    .await;
    r.core.continue_execution(&exec).await.unwrap();
    let s = c.answer(3).await.result.unwrap();
    assert_eq!(
        (s["reached"].as_str(), s["already"].as_bool()),
        (Some("settled"), Some(false))
    );
    assert_eq!(s["execution"]["attention"]["label"], "ready");
    assert!(s["execution"]["position"].as_u64().unwrap() > queued_at);

    // After the current view, nothing more happens: a timeout.
    let at = s["execution"]["position"].as_u64().unwrap();
    let t = c
        .call(
            4,
            method::SESSION_WAIT,
            json!({"session_id": sid, "until": "settled", "after_position": at, "timeout_ms": 200}),
        )
        .await
        .result
        .unwrap();
    assert_eq!(
        (t["reached"].as_str(), t["already"].as_bool()),
        (Some("timeout"), Some(false))
    );
    assert_eq!(t["execution"]["position"].as_u64(), Some(at));

    // Terminal: refused for a conversation; a task session's end.
    let e = c
        .call(
            5,
            method::SESSION_WAIT,
            json!({"session_id": sid, "until": "terminal"}),
        )
        .await
        .error
        .unwrap();
    assert_eq!(e.code, error_code::INVALID_PARAMS);
    assert!(
        e.message.contains("a conversation never ends"),
        "{}",
        e.message
    );
    let task = r
        .core
        .open_session(SessionOpenParams {
            kind: Some(SessionKind::Task),
            label: None,
            opened_from: None,
        })
        .unwrap();
    c.send(
        6,
        method::SESSION_WAIT,
        json!({"session_id": task.session_id, "until": "terminal", "timeout_ms": 20_000}),
    )
    .await;
    until("the task's wait parked", || {
        r.core.push.status(0).waiting == 1
    })
    .await;
    r.core
        .cancel_execution(task.execution_id.as_deref().unwrap(), "test")
        .await
        .unwrap();
    let d = c.answer(6).await.result.unwrap();
    assert_eq!(d["reached"], "terminal");
    assert_eq!(d["execution"]["state"], "cancelled");

    // No such session, and no params: at once.
    let e = c
        .call(
            7,
            method::SESSION_WAIT,
            json!({"session_id": "ses_none", "until": "settled"}),
        )
        .await
        .error
        .unwrap();
    assert_eq!(e.code, error_code::NOT_FOUND);
    let e = c
        .call(8, method::SESSION_WAIT, Value::Null)
        .await
        .error
        .unwrap();
    assert_eq!(e.code, error_code::INVALID_PARAMS);
    assert_eq!(r.core.push.status(0).waiting, 0);
}

/// An answer is one frame (theseus-jj9f): the bind or the decline, the
/// answer's row, and the wake. So a settled wait parked before an answer, a
/// decline or an approval, stays parked through it and returns when the
/// continuation ends; and the answer's view is the execution queued, never
/// waiting on the question it no longer has (`● waiting on you`, which is
/// settled).
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_settled_wait_parked_before_an_answer_returns_after_its_continuation() {
    let r = rig_full(
        vec![
            Scripted::tools(
                "",
                &[("t1", "fs_write", json!({"path": "a.txt", "content": "a"}))],
            ),
            Scripted::text("Not written, then."),
            Scripted::tools(
                "",
                &[("t2", "fs_write", json!({"path": "b.txt", "content": "b"}))],
            ),
            Scripted::text("Written."),
        ],
        true,
        false,
    );
    let seen = watcher(&r.core).await;
    let mut c = Raw::connect(&r.core, "waiter", 1 << 20);
    let mut session: Option<String> = None;
    for (id, approve) in [(10u64, false), (20, true)] {
        let asked = turn_result(&r.core, session.as_deref()).await;
        let sid = asked.session_id.clone();
        session = Some(sid.clone());
        let corr = asked.awaiting_confirm.clone().unwrap();
        let exec = asked.execution_id.clone().unwrap();
        until("the board saw the question", || {
            r.core
                .push
                .view_of_session(&sid)
                .is_some_and(|v| v.state == "waiting" && !v.pending.is_empty())
        })
        .await;
        let parked_at = r.core.push.view_of_session(&sid).unwrap().position;
        c.send(
            id,
            method::SESSION_WAIT,
            json!({"session_id": sid, "until": "settled", "after_position": parked_at, "timeout_ms": 20_000}),
        )
        .await;
        until("the wait parked", || r.core.push.status(0).waiting == 1).await;
        let before = r.core.store.stats().unwrap().frames_appended;
        r.core.confirm_action(&corr, approve, None, "test").unwrap();
        assert_eq!(
            r.core.store.stats().unwrap().frames_appended - before,
            1,
            "the answer (approve: {approve}) is one frame"
        );
        until("the board saw the answer queue it", || {
            r.core
                .push
                .view_of_session(&sid)
                .is_some_and(|v| v.state == "queued")
        })
        .await;
        assert_eq!(
            r.core.push.status(0).waiting,
            1,
            "the answer (approve: {approve}) woke the settled wait parked before it"
        );
        r.core.continue_execution(&exec).await.unwrap();
        let s = c.answer(id).await.result.unwrap();
        assert_eq!(
            (s["reached"].as_str(), s["already"].as_bool()),
            (Some("settled"), Some(false)),
            "approve: {approve}"
        );
        assert_eq!(s["execution"]["attention"]["label"], "ready");
        let end = s["execution"]["position"].as_u64().unwrap();
        until("the watcher saw the continuation end", || {
            applied(&seen.lock().unwrap())
                .get(&exec)
                .is_some_and(|v| v.position >= end)
        })
        .await;
        let views: Vec<ExecutionView> = seen
            .lock()
            .unwrap()
            .iter()
            .filter_map(|m| match m {
                Message::Notification(n) if n.method == notify::EXECUTION_CHANGED => {
                    serde_json::from_value::<ExecutionView>(n.params.clone()).ok()
                }
                _ => None,
            })
            .filter(|v| v.execution_id == exec && v.position > parked_at && v.position < end)
            .collect();
        let answer = views.first().expect("the answer's view");
        assert_eq!(
            (answer.state.as_str(), answer.attention.level.as_str()),
            ("queued", "working"),
            "the answer's view (approve: {approve}): {answer:?}"
        );
        assert!(
            views
                .iter()
                .all(|v| v.attention.level.as_str() != "needs_you"),
            "a view between the answer and its continuation's end needs you: {views:?}"
        );
    }
}

/// A connection holds at most 64 waits, and a closed connection ends its
/// waits: health's count goes back to none.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_closed_connection_ends_its_waits_and_one_holds_at_most_64() {
    let r = rig();
    let sid = turn(&r.core, None).await;
    let mut c = Raw::connect(&r.core, "many", 1 << 20);
    for i in 0..64 {
        c.send(
            i,
            method::SESSION_WAIT,
            json!({"session_id": sid, "until": "blocked", "timeout_ms": 60_000}),
        )
        .await;
    }
    until("64 waits parked", || r.core.push.status(0).waiting == 64).await;
    let e = c
        .call(
            64,
            method::SESSION_WAIT,
            json!({"session_id": sid, "until": "blocked", "timeout_ms": 60_000}),
        )
        .await
        .error
        .unwrap();
    assert_eq!(e.code, error_code::LIMIT, "{}", e.message);
    // Another connection has its own 64.
    let mut other = Raw::connect(&r.core, "other", 1 << 20);
    let ok = other
        .call(
            1,
            method::SESSION_WAIT,
            json!({"session_id": sid, "until": "settled", "timeout_ms": 1000}),
        )
        .await;
    assert_eq!(ok.result.unwrap()["already"], true);
    drop(c);
    until("the closed connection's waits ended", || {
        r.core.push.status(0).waiting == 0
    })
    .await;
}

/// The lag prove (design `stage2` §3.2): a client that stops reading while
/// 5,000 events pass. Its queue reaches the cap, what comes after is dropped
/// and counted, and once it reads again it drains and hears one
/// `events.lost` naming the stream. Its re-snapshot then equals a fresh
/// client's, and health counts what was lost.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_client_that_stops_reading_hears_what_it_lost_and_catches_up() {
    const EVENTS: usize = 5000;
    let r = rig_full(vec![], false, true);
    let mut slow = Raw::connect(&r.core, "slow", 16 * 1024);
    let snap = slow
        .call(1, method::EXECUTIONS_WATCH, json!({"limit": 10_000}))
        .await;
    assert_eq!(snap.result.unwrap()["total"], 0);
    // It stops reading. Each new session is one frame and one event.
    for _ in 0..EVENTS {
        r.core.open_session(SessionOpenParams::default()).unwrap();
    }
    let last = last_kernel_position(&r.core);
    until("the board applied every frame", || {
        r.core.push.status(0).position >= last
    })
    .await;
    assert!(
        r.core.push.status(0).lost > 0,
        "the cap dropped some: {:?}",
        r.core.push.status(0)
    );
    // It reads again: everything queued, then one notice.
    let mut changed = 0;
    let lost = loop {
        let line = tokio::time::timeout(Duration::from_secs(30), slow.lines.next_line())
            .await
            .expect("a line in 30 s")
            .unwrap()
            .unwrap();
        let Message::Notification(n) = serde_json::from_str::<Message>(&line).unwrap() else {
            continue;
        };
        match n.method.as_str() {
            notify::EXECUTION_CHANGED => changed += 1,
            notify::EVENTS_LOST => break n.params,
            other => panic!("unexpected {other}"),
        }
    };
    let dropped = lost["dropped"].as_u64().unwrap() as usize;
    eprintln!("lag prove: {changed} execution.changed, then events.lost {lost}");
    assert_eq!(lost["streams"], json!(["executions"]));
    assert!(
        dropped > 0 && changed >= crate::outbound::BACKLOG_CAP - 16,
        "{changed} then {dropped}"
    );
    assert_eq!(changed + dropped, EVENTS, "every event came or was counted");
    assert_eq!(r.core.push.status(0).lost as usize, dropped);
    // Its re-snapshot equals a fresh client's.
    let again: ExecutionsWatchResult = serde_json::from_value(
        slow.call(2, method::EXECUTIONS_WATCH, json!({"limit": 10_000}))
            .await
            .result
            .unwrap(),
    )
    .unwrap();
    assert!(
        slow.notes.iter().all(|n| n.method != notify::EVENTS_LOST),
        "once only"
    );
    let mut fresh = Raw::connect(&r.core, "fresh", 1 << 20);
    let fresh: ExecutionsWatchResult = serde_json::from_value(
        fresh
            .call(1, method::EXECUTIONS_WATCH, json!({"limit": 10_000}))
            .await
            .result
            .unwrap(),
    )
    .unwrap();
    let key = |s: &ExecutionsWatchResult| -> BTreeMap<String, (u64, String)> {
        s.executions
            .iter()
            .map(|v| {
                (
                    v.execution_id.clone(),
                    (v.position, v.attention.label.clone()),
                )
            })
            .collect()
    };
    assert_eq!(again.executions.len(), EVENTS);
    assert_eq!(key(&again), key(&fresh));
    assert_eq!(again.position, fresh.position);
}

/// The board keeps up while every runtime worker is held (theseus-hanu). A
/// burst of requests whose handlers write the store held them all, and the
/// board, applied on a task of the runtime, fell seconds behind the commits
/// (in3's live check: 417 views beside 4,782 sessions). Its frames are applied
/// on a thread of the blocking pool now: here both workers wait on a gate that
/// the test opens only once the board holds every frame a thread of its own
/// committed meanwhile.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn the_board_keeps_up_while_every_worker_is_held() {
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::time::Instant;
    let r = rig();
    r.core.push.ensure(&r.core).await.unwrap();
    let gate = Arc::new((Mutex::new(false), std::sync::Condvar::new()));
    let holding = Arc::new(AtomicUsize::new(0));
    for _ in 0..2 {
        let (g, h) = (gate.clone(), holding.clone());
        tokio::spawn(async move {
            h.fetch_add(1, Ordering::SeqCst);
            // A blocking wait on a worker, as a handler's fsync is.
            let (open, cv) = &*g;
            let mut open = open.lock().unwrap();
            while !*open {
                open = cv.wait(open).unwrap();
            }
        });
    }
    let t0 = Instant::now();
    while holding.load(Ordering::SeqCst) < 2 {
        assert!(
            t0.elapsed() < Duration::from_secs(10),
            "the workers were not held"
        );
        std::thread::sleep(Duration::from_millis(1));
    }
    // Twenty executions, committed from a thread that is no worker.
    let core = r.core.clone();
    std::thread::spawn(move || {
        for _ in 0..20 {
            core.kernel
                .open_execution(
                    &theseus_kernel::new_id("ses"),
                    SessionKind::Conversation,
                    theseus_kernel::Authority {
                        principal: crate::turn::OPERATOR.into(),
                        ..Default::default()
                    },
                    None,
                    None,
                )
                .unwrap();
        }
    })
    .join()
    .unwrap();
    let want = r.core.store.last_position();
    let t0 = Instant::now();
    let caught_up = loop {
        if r.core.push.status(0).position >= want {
            break true;
        }
        if t0.elapsed() > Duration::from_secs(10) {
            break false;
        }
        std::thread::sleep(Duration::from_millis(2));
    };
    let board = r.core.push.status(0);
    {
        let (open, cv) = &*gate;
        *open.lock().unwrap() = true;
        cv.notify_all();
    }
    assert!(
        caught_up,
        "the board stopped at {} of {want} while every worker was held",
        board.position
    );
    assert_eq!(board.board, 20, "a view for each execution");
}

// ------------------------------------------------------------- tq04

/// A `session.watch` alone seeds the push (theseus-tq04): on a daemon that
/// nothing else watches, a client of one session gets that session's
/// `execution.changed`, and no other session's. Before the fix the watch
/// never called `push.ensure`, so the board stayed unseeded and the client
/// heard only the turn's own events.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_session_watch_alone_seeds_the_push_and_gets_its_executions_changes() {
    let r = rig();
    let mine = turn(&r.core, None).await;
    let other = turn(&r.core, None).await;
    assert!(!r.core.push.seeded(), "nothing has watched yet");
    assert!(!r.core.kernel.observed());

    let mut c = Raw::connect(&r.core, "session-watcher", 1 << 20);
    let a = c
        .call(1, method::SESSION_WATCH, json!({"session_id": mine}))
        .await;
    assert_eq!(a.result.unwrap()["watching"].as_str(), Some(mine.as_str()));
    assert!(
        r.core.push.seeded(),
        "the first session watch seeds the board"
    );
    assert!(r.core.kernel.observed());
    assert_eq!(r.core.bus.all_watchers(), 0, "no executions.watch was made");

    // The other session's turn first, so anything of it that would reach
    // this client is read before the watched session's last view.
    turn(&r.core, Some(&other)).await;
    turn(&r.core, Some(&mine)).await;
    let last = last_kernel_position(&r.core);
    let settled = c
        .until_view(|v| v.session_id == mine && v.position >= last)
        .await
        .expect("the watched session's execution.changed reaches its watcher");
    assert_eq!((settled.state.as_str(), settled.turns), ("waiting", 2));
    for n in &c.notes {
        if n.method == notify::EXECUTION_CHANGED {
            let v: ExecutionView = serde_json::from_value(n.params.clone()).unwrap();
            assert_eq!(v.session_id, mine, "only the watched session's changes");
        }
    }

    // A second watcher on a seeded board seeds nothing again.
    let mut d = Raw::connect(&r.core, "second-watcher", 1 << 20);
    d.call(1, method::SESSION_WATCH, json!({"session_id": other}))
        .await
        .result
        .unwrap();
    assert_eq!(r.core.bus.watchers(&other), 1);
}
