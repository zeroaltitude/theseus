//! The push in the core (theseus-in3): the snapshot and the events agree
//! under the position rule however the turns race the watch, nothing is
//! observed until someone watches, and the observer adds no frame.

use std::collections::BTreeMap;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use serde_json::{json, Value};
use theseus_protocol::{ExecutionView, ExecutionsWatchResult, Id, Message, Request, SessionKind};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};

use crate::bus::EventSink;
use crate::provider::FakeProvider;
use crate::session::SessionRecord;
use crate::store::Store;
use crate::turn::TurnRequest;
use crate::{Config, Core};

struct Rig {
    core: Arc<Core>,
    _dir: tempfile::TempDir,
}

fn rig() -> Rig {
    let dir = tempfile::tempdir().unwrap();
    let mut cfg = Config::example();
    cfg.server.state_dir = dir.path().to_string_lossy().into_owned();
    let store = Store::open(&dir.path().join("store")).unwrap();
    let core = Core::build(crate::rpc::Parts::for_tests(
        cfg,
        Arc::new(FakeProvider::default()),
        store,
    ))
    .unwrap();
    Rig { core, _dir: dir }
}

/// One plain turn, on `session` or a new conversation.
async fn turn(core: &Arc<Core>, session: Option<&str>) -> String {
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
            config_wait_us: 0,
            reply_to: None,
        })
        .await
        .unwrap()
        .session_id
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
    for _ in 0..4 {
        turn(&r.core, None).await;
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
    let last = last_kernel_position(&r.core);
    for _ in 0..500 {
        if r.core.push.status(0).position >= last {
            break;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    assert!(
        r.core.push.status(0).position >= last,
        "the board caught up"
    );
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
