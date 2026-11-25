//! Since you last looked, in the chat (theseus-xock5): the digest line when it
//! opens after an absence or its connection comes back, both asked since the
//! store's look and moving it (issue 46), a first start that is told nothing,
//! and the machine's seen file deciding what rings: a question already shown
//! elsewhere does not ring here, and the operator who has been away hears the
//! conversation's own.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use serde_json::{json, Value};
use theseus_protocol::work::WorkView;
use theseus_protocol::{Event, WorkChanged};

use super::state::{Chat, Effect, Purpose};
use super::tests::{cfg, script, Rig, SID, T0};
use crate::tests::{Answer, Script};

const HOUR: u64 = 3_600_000;

/// The digest, with the store's look at `look_ms` (none: the store has none).
fn digest_from(look_ms: Option<u64>) -> Value {
    let mut d = digest();
    match look_ms {
        Some(ms) => d["look"] = json!({"position": 500, "at_ms": ms}),
        None => {
            d["lines"] = json!([]);
        }
    }
    d
}

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

fn with_digest(look_ms: Option<u64>) -> Script {
    let base = script();
    Arc::new(move |m, p| match m {
        "work.digest" => Ok(digest_from(look_ms)),
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
    rig_looked(path, Some(T0 - HOUR)).await
}

/// A chat whose store's look is at `look_ms`.
async fn rig_looked(path: &Path, look_ms: Option<u64>) -> Rig {
    let at = path.to_path_buf();
    Rig::with_cfg(100, 30, with_digest(look_ms), move |c| {
        c.seen_path = Some(at);
    })
    .await
}

/// How each digest was asked: its `since_look`, and whether it named a
/// position (never: a position this chat holds may be another history's).
fn asked_how(asked: &[Value]) -> Vec<(Value, bool)> {
    asked
        .iter()
        .map(|a| (a["since_look"].clone(), a.get("since").is_some()))
        .collect()
}

// ------------------------------------------------------------ the digest

#[tokio::test(start_paused = true)]
async fn opening_after_an_absence_tells_what_happened_in_one_line() {
    let (_dir, path) = seen_file();
    let mut r = rig_looked(&path, Some(T0 - HOUR)).await;
    r.asked("work.digest", 1).await;
    assert_eq!(
        asked_how(&r.daemon.asked("work.digest")),
        [(json!(true), false)]
    );
    r.shows("◆ 2 reported, 1 answered by you since").await;
    assert!(r.text().contains("theseus digest"), "{}", r.text());
    // The banner comes first: the line is told under it, never above.
    let text = r.text();
    assert!(
        text.find("◆ 2 reported").unwrap() > text.find("harbour-tides").unwrap(),
        "{text}"
    );
    // And the store's look moved to the digest's end.
    r.asked("work.look", 1).await;
    assert_eq!(r.daemon.asked("work.look"), [json!({"position": 900})]);
}

#[tokio::test(start_paused = true)]
async fn a_first_start_is_told_nothing_and_its_board_is_the_first_look() {
    let (_dir, path) = seen_file();
    let mut r = rig_looked(&path, None).await;
    r.ready().await;
    r.asked("work.watch", 1).await;
    r.asked("work.digest", 1).await;
    r.settle().await;
    assert_eq!(
        asked_how(&r.daemon.asked("work.digest")),
        [(json!(true), false)]
    );
    assert!(!r.text().contains('◆'), "{}", r.text());
    r.asked("work.look", 1).await;
    assert_eq!(r.daemon.asked("work.look"), [json!({"position": 900})]);
}

#[tokio::test(start_paused = true)]
async fn a_look_a_moment_ago_is_no_absence() {
    let (_dir, path) = seen_file();
    let mut r = rig_looked(&path, Some(T0 - 10_000)).await;
    r.ready().await;
    r.asked("work.watch", 1).await;
    r.asked("work.digest", 1).await;
    r.settle().await;
    assert!(!r.text().contains('◆'), "{}", r.text());
}

#[test]
fn the_welcome_and_the_reconnect_ask_since_the_stores_look() {
    let (_dir, path) = seen_file();
    let mut c = Chat::new(cfg(PathBuf::from("/nonexistent/history")), T0);
    c.cfg.seen_path = Some(path);
    let asks = |effects: Vec<Effect>| {
        effects
            .into_iter()
            .filter_map(|e| match e {
                Effect::Call(call) if call.purpose == Purpose::Welcome => Some(call.params),
                _ => None,
            })
            .collect::<Vec<_>>()
    };
    let welcome = asks(c.connected(T0));
    assert_eq!(welcome.len(), 1, "{welcome:?}");
    assert_eq!(asked_how(&welcome), [(json!(true), false)]);
    c.session = Some(SID.to_string());
    c.board.position = 640;
    c.now_ms = T0 + 5_000;
    c.disconnected();
    // A restart of the daemon is not an absence.
    assert!(asks(c.connected(T0 + 15_000)).is_empty());
    // Two minutes is, and the board this chat held is no position to ask from.
    c.disconnected();
    let back = asks(c.connected(T0 + 15_000 + 120_000));
    assert_eq!(back.len(), 1, "{back:?}");
    assert_eq!(asked_how(&back), [(json!(true), false)]);
}

/// A store with no look, whose `work.look` answers `look`.
fn looking(look: Answer) -> Script {
    let base = with_digest(None);
    Arc::new(move |m, p| match m {
        "work.look" => look.clone(),
        _ => base(m, p),
    })
}

/// A chat on `path` against `script`.
async fn rig_on(path: &Path, script: Script) -> Rig {
    let at = path.to_path_buf();
    Rig::with_cfg(100, 30, script, move |c| {
        c.seen_path = Some(at);
    })
    .await
}

/// The seen file's old time once the loop's writes off its thread are done: none
/// is awaited, so give them a moment, then read it until `want` or a bound.
fn old_time_settles(path: &Path, want: Option<u64>) -> Option<u64> {
    let mut got = theseus_client::seen::old_look_time(path);
    for _ in 0..100 {
        std::thread::sleep(std::time::Duration::from_millis(20));
        got = theseus_client::seen::old_look_time(path);
        if got == want {
            break;
        }
    }
    got
}

/// The time an older build's seen file kept (issue 46) goes only when the store
/// has taken a look: a welcome (or a reconnect, which is the same answer's
/// path) whose look the daemon refuses keeps it, the next start asks from it
/// again, and the look the daemon takes hands it over.
#[tokio::test(start_paused = true)]
async fn the_welcome_keeps_the_old_time_until_the_store_takes_a_look() {
    let (_dir, path) = seen_file();
    let old = T0 - 3 * HOUR;
    let file = json!({"version": 1, "board_read": true, "executions": {},
        "looked": {"position": 999_999, "at_ms": old}});
    std::fs::write(&path, file.to_string()).unwrap();
    let busy = Err((-32000, "the store is busy".to_string()));
    let mut r = rig_on(&path, looking(busy)).await;
    r.asked("work.digest", 1).await;
    assert_eq!(r.daemon.asked("work.digest")[0]["since_ms"], old);
    r.asked("work.look", 1).await;
    r.settle().await;
    assert_eq!(
        old_time_settles(&path, None),
        Some(old),
        "a refused look forgets nothing"
    );
    drop(r);
    let took = Ok(json!({"look": {"position": 900, "at_ms": T0}}));
    let mut r = rig_on(&path, looking(took)).await;
    r.asked("work.digest", 1).await;
    assert_eq!(
        r.daemon.asked("work.digest")[0]["since_ms"],
        old,
        "asked from it again"
    );
    r.asked("work.look", 1).await;
    r.settle().await;
    assert_eq!(old_time_settles(&path, None), None, "handed over");
    let text = std::fs::read_to_string(&path).unwrap();
    let on_disk: Value = serde_json::from_str(&text).unwrap();
    assert_eq!(on_disk["store_look"], json!({"position": 900, "at_ms": T0}));
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
