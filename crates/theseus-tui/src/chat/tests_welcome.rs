//! Since you last looked, in the chat (theseus-xock5): the digest line when it
//! opens after an absence or its connection comes back, a first start that is
//! told nothing, and the machine's seen file deciding what rings: a question
//! already shown elsewhere does not ring here, and the operator who has been
//! away hears the conversation's own.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use serde_json::{json, Value};
use theseus_client::seen;
use theseus_protocol::work::WorkView;
use theseus_protocol::{Event, WorkChanged};

use super::state::{Chat, Effect, Purpose};
use super::tests::{cfg, script, Rig, SID, T0};
use crate::tests::Script;

const HOUR: u64 = 3_600_000;

fn digest() -> Value {
    json!({
        "from": 500, "to": 900, "from_ledger": false,
        "now": {"needs_you": 0, "working": 0, "done": 0, "failed": 0, "total": 0},
        "questions": [],
        "lines": [
            {"at_ms": T0 - 3_000_000, "kind": "reported", "count": 2, "words": "📋 Tide table reported",
             "work": {"id": "exe_t", "kind": "task", "title": "Tide table"}},
            {"at_ms": T0 - 2_000_000, "kind": "answered", "count": 1, "words": "✓ you answered: fs.write notes.txt",
             "work": {"id": "act_1", "kind": "question", "title": "fs.write notes.txt"}}
        ]
    })
}

fn with_digest() -> Script {
    let base = script();
    Arc::new(move |m, p| match m {
        "work.digest" => Ok(digest()),
        "work.watch" => Ok(json!({"position": 700, "views": [], "total": 0})),
        _ => base(m, p),
    })
}

fn seen_file() -> (tempfile::TempDir, PathBuf) {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("seen.json");
    (dir, path)
}

/// The shown positions as another client's write leaves them.
fn shown(path: &Path, id: &str, position: u64) {
    let file = json!({
        "version": 1, "board_read": true,
        "executions": {id: {"displayed": position, "finished": 0, "position": position,
            "level": "idle", "turns": 1}},
    });
    std::fs::write(path, file.to_string()).unwrap();
}

async fn rig(path: &Path) -> Rig {
    let at = path.to_path_buf();
    Rig::with_cfg(100, 30, with_digest(), move |c| c.seen_path = Some(at)).await
}

// ------------------------------------------------------------ the digest

#[tokio::test(start_paused = true)]
async fn opening_after_an_absence_tells_what_happened_in_one_line() {
    let (_dir, path) = seen_file();
    seen::record_looked(&path, 500, T0 - HOUR).unwrap();
    let mut r = rig(&path).await;
    r.asked("work.digest", 1).await;
    assert_eq!(r.daemon.asked("work.digest"), [json!({"since": 500})]);
    r.shows("◆ 2 reported, 1 answered by you since").await;
    assert!(r.text().contains("theseus digest"), "{}", r.text());
    // The banner comes first: the line is told under it, never above.
    let text = r.text();
    assert!(
        text.find("◆ 2 reported").unwrap() > text.find("harbour-tides").unwrap(),
        "{text}"
    );
    // And the look moved to the digest's end, in the file.
    for _ in 0..100 {
        if seen::looked(&path).position == 900 {
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(1)).await;
        std::thread::sleep(std::time::Duration::from_millis(5));
    }
    assert_eq!(seen::looked(&path).position, 900);
}

#[tokio::test(start_paused = true)]
async fn a_first_start_is_told_nothing_and_its_board_is_the_first_look() {
    let (_dir, path) = seen_file();
    let mut r = rig(&path).await;
    r.ready().await;
    r.asked("work.watch", 1).await;
    r.settle().await;
    assert!(r.daemon.asked("work.digest").is_empty());
    assert!(!r.text().contains('◆'), "{}", r.text());
    for _ in 0..100 {
        if seen::looked(&path).position == 700 {
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(1)).await;
        std::thread::sleep(std::time::Duration::from_millis(5));
    }
    assert_eq!(seen::looked(&path).position, 700);
}

#[tokio::test(start_paused = true)]
async fn a_look_a_moment_ago_is_no_absence() {
    let (_dir, path) = seen_file();
    seen::record_looked(&path, 500, T0 - 10_000).unwrap();
    let mut r = rig(&path).await;
    r.ready().await;
    r.asked("work.watch", 1).await;
    r.settle().await;
    assert!(r.daemon.asked("work.digest").is_empty());
}

#[test]
fn a_connection_back_after_a_long_loss_asks_since_the_board_it_knew() {
    let mut c = Chat::new(cfg(PathBuf::from("/nonexistent/history")), T0);
    c.session = Some(SID.to_string());
    c.board.position = 640;
    c.now_ms = T0 + 5_000;
    c.disconnected();
    let asks = |effects: Vec<Effect>| {
        effects
            .into_iter()
            .filter_map(|e| match e {
                Effect::Call(call) if call.purpose == Purpose::Welcome => Some(call.params),
                _ => None,
            })
            .collect::<Vec<_>>()
    };
    // A restart of the daemon is not an absence.
    assert!(asks(c.connected(T0 + 15_000)).is_empty());
    // Two minutes is.
    c.disconnected();
    assert_eq!(
        asks(c.connected(T0 + 15_000 + 120_000)),
        [json!({"since": 640})]
    );
}

// ------------------------------------------------------------ what rings

fn notice(why: &str, position: u64, work: Value) -> Value {
    json!({
        "urgency": "interrupt", "why": why, "work": work,
        "root": {"id": "exe_conv", "kind": "conversation", "session_id": SID, "title": ""},
        "line": "x", "position": position, "at_ms": T0
    })
}

fn paint() -> Value {
    json!({"id": "exe_paint", "kind": "task", "session_id": "ses_paint2a329e", "title": "Paint log"})
}

#[tokio::test(start_paused = true)]
async fn a_question_already_shown_elsewhere_does_not_ring_twice() {
    let (_dir, path) = seen_file();
    shown(&path, "exe_paint", 30);
    let mut r = rig(&path).await;
    r.ready().await;
    let mark = r.bytes();
    // Shown up to position 30: the question at 21 was seen, the one at 31 was not.
    r.daemon.notify("work.notice", notice("asked", 21, paint()));
    r.settle().await;
    assert!(
        !r.written_since(mark).contains('\x07'),
        "a seen question rang"
    );
    r.daemon.notify("work.notice", notice("asked", 31, paint()));
    r.settle().await;
    assert_eq!(r.written_since(mark).matches('\x07').count(), 1);
}

#[tokio::test(start_paused = true)]
async fn the_operator_away_hears_the_conversations_own_question() {
    let (_dir, path) = seen_file();
    let mut r = rig(&path).await;
    r.ready().await;
    let own = json!({"id": "exe_conv", "kind": "conversation", "session_id": SID, "title": ""});
    // Present and typing, the conversation on screen rings nothing.
    let mark = r.bytes();
    r.daemon
        .notify("work.notice", notice("asked", 40, own.clone()));
    r.settle().await;
    assert!(!r.written_since(mark).contains('\x07'));
    // A quarter hour without a key, and it is told.
    r.advance(15 * 60_000).await;
    let mark = r.bytes();
    r.daemon.notify("work.notice", notice("asked", 41, own));
    r.settle().await;
    assert_eq!(r.written_since(mark).matches('\x07').count(), 1);
}

/// A question asked as a view of its own (an approval's) counts as seen by
/// the mark of the task that asked it.
#[tokio::test(start_paused = true)]
async fn a_question_of_its_own_is_seen_by_its_tasks_mark() {
    let (_dir, path) = seen_file();
    shown(&path, "exe_paint", 800);
    let mut r = rig(&path).await;
    r.ready().await;
    let q: WorkView = serde_json::from_value(json!({
        "id": "act_q1", "kind": "question", "position": 701, "at_ms": T0, "root": "exe_conv",
        "parent": "exe_paint", "title": "fs.write", "state": "needs_you",
        "attention": {"level": "needs_you", "label": "confirm fs.write", "since_ms": T0},
        "started_at_ms": T0, "state_since_ms": T0
    }))
    .unwrap();
    r.tell(Event::WorkChanged(WorkChanged {
        position: 701,
        at_ms: T0,
        views: vec![q],
        gone: vec![],
        threads: vec![],
    }));
    r.settle().await;
    let mark = r.bytes();
    let work = json!({"id": "act_q1", "kind": "question", "title": "fs.write"});
    r.daemon.notify("work.notice", notice("asked", 750, work));
    r.settle().await;
    assert!(
        !r.written_since(mark).contains('\x07'),
        "a question its task showed rang"
    );
}
