//! Threads on the board (theseus-5i8i): `work.watch`'s groups in the sidebar,
//! folded and opened with `T`, at two widths; an older daemon says nothing.

use crossterm::event::KeyCode;
use serde_json::{json, Value};
use theseus_protocol::{WorkChanged, WorkWatchResult};

use crate::tests::{harbour_world, script, Rig, T0};
use crate::threads::{Kind, ThreadBoard};

fn view(
    id: &str,
    kind: &str,
    parent: Option<&str>,
    thread: &str,
    level: &str,
    label: &str,
    pos: u64,
) -> Value {
    json!({
        "id": id, "kind": kind, "position": pos, "at_ms": T0 - 60_000, "parent": parent,
        "root": parent.unwrap_or(id), "session_id": format!("ses_{id}"), "title": id,
        "state": if level == "needs_you" { "needs_you" } else { "running" },
        "attention": {"level": level, "label": label, "since_ms": 3},
        "started_at_ms": 1, "state_since_ms": 3, "thread": thread
    })
}

fn group(slug: &str, title: &str) -> Value {
    json!({"slug": slug, "title": title, "state": "active", "session_id": format!("ses_exe_{slug}"),
        "last_active_ms": T0 - 120_000, "context_tokens": 41000, "today_usd": 1.2})
}

/// Two threads, each with a task and a question.
fn snapshot() -> WorkWatchResult {
    serde_json::from_value(json!({
        "position": 10, "total": 2,
        "views": [
            view("exe_pr", "conversation", None, "pr-round", "ready", "ready", 4),
            view("exe_pr_t", "task", Some("exe_pr"), "pr-round", "working", "turn 2", 5),
            view("act_pr_q", "question", Some("exe_pr"), "pr-round", "needs_you", "confirm fs.write", 6),
            view("exe_tides", "conversation", None, "tides", "ready", "ready", 7),
            view("exe_tides_t", "task", Some("exe_tides"), "tides", "working", "turn 1", 8),
        ],
        "threads": [group("pr-round", "PR round"), group("tides", "Tides")]
    }))
    .unwrap()
}

fn texts(b: &ThreadBoard, width: usize) -> Vec<String> {
    b.lines(T0, width).into_iter().map(|l| l.text).collect()
}

/// The block at 80 and at 40 columns: each thread a header with its count, its
/// open work under it; the narrow header gives up details, never the title.
#[test]
fn the_threads_block_at_two_widths() {
    let mut b = ThreadBoard::default();
    b.snapshot(snapshot());
    assert_eq!(
        texts(&b, 80),
        [
            "▾ PR round · active · 1m ago · warm · 41k ctx · $1.20 today  ● 1",
            "  ● act_pr_q  confirm fs.write",
            "  ◐ exe_pr_t  turn 2",
            "▾ Tides · active · 1m ago · warm · 41k ctx · $1.20 today  ◐ 1",
            "  ◐ exe_tides_t  turn 1",
        ]
    );
    assert_eq!(texts(&b, 40)[0], "▾ PR round · active · 1m ago · warm  ● 1");
    assert_eq!(texts(&b, 40)[3], "▾ Tides · active · 1m ago · warm  ◐ 1");
    assert_eq!(texts(&b, 30)[0], "▾ PR round · active  ● 1");
}

/// A group folds to its header and unfolds; the cursor opens an item's session.
#[test]
fn a_thread_folds_and_an_item_opens_its_session() {
    let mut b = ThreadBoard::default();
    b.snapshot(snapshot());
    b.toggle(T0, 80);
    assert_eq!(texts(&b, 80).len(), 3);
    assert!(texts(&b, 80)[0].starts_with("▸ PR round"));
    b.toggle(T0, 80);
    assert_eq!(texts(&b, 80).len(), 5);
    b.step(1, T0, 80);
    assert_eq!(b.session_at(T0, 80).as_deref(), Some("ses_act_pr_q"));
    b.step(-1, T0, 80);
    assert_eq!(b.session_at(T0, 80), None);
}

/// The position rule, and a gone id; a group's update keeps the stats it did not read.
#[test]
fn changes_apply_by_the_position_rule() {
    let mut b = ThreadBoard::default();
    b.snapshot(snapshot());
    let older = view(
        "exe_pr_t",
        "task",
        Some("exe_pr"),
        "pr-round",
        "needs_you",
        "stale",
        2,
    );
    b.changed(WorkChanged {
        position: 11,
        at_ms: T0,
        views: vec![serde_json::from_value(older).unwrap()],
        gone: vec![],
        threads: vec![],
    });
    assert!(
        texts(&b, 80)[2].contains("turn 2"),
        "an older view is not applied"
    );
    b.changed(WorkChanged {
        position: 12,
        at_ms: T0,
        views: vec![],
        gone: vec!["exe_pr_t".into()],
        threads: vec![serde_json::from_value(json!({
            "slug": "pr-round", "title": "PR round", "state": "paused",
            "session_id": "ses_exe_pr", "last_active_ms": T0 - 120_000
        }))
        .unwrap()],
    });
    let t = texts(&b, 80);
    assert!(t[0].contains("paused") && t[0].contains("41k ctx"), "{t:?}");
    assert!(!t.iter().any(|l| l.contains("exe_pr_t")));
    // A paused thread keeps its question and leaves the count.
    assert!(t.iter().any(|l| l.contains("act_pr_q")));
}

/// A seal waiting is a line that needs you, and a count the header adds.
#[test]
fn a_pending_seal_is_a_line_and_a_need() {
    let mut b = ThreadBoard::default();
    let mut snap = snapshot();
    snap.threads[1].seal_pending = Some("the harbour notes are private".into());
    b.snapshot(snap);
    assert_eq!(b.seals_pending(), 1);
    let lines = b.lines(T0, 80);
    assert!(lines
        .iter()
        .any(|l| l.kind == Kind::Seal
            && l.text == "  ● seal recommended: the harbour notes are private"));
}

/// Through the loop: `work.watch` is asked at the connect, the groups draw above
/// the sessions, `T` moves into them and enter folds one.
#[tokio::test]
async fn the_groups_draw_above_the_sessions_and_t_folds_one() {
    let world = harbour_world();
    world.lock().unwrap().answers.insert(
        "work.watch".into(),
        Ok(serde_json::to_value(snapshot()).unwrap()),
    );
    let mut rig = Rig::new(120, 30, script(world));
    rig.shows("▾ PR round").await;
    assert_eq!(
        rig.daemon().asked("work.watch")[0],
        json!({"limit": 0, "interactive": true})
    );
    assert!(rig.screen().iter().any(|l| l.contains("● act_pr_q")));
    rig.press(&[KeyCode::Char('T'), KeyCode::Enter]).await;
    rig.shows("▸ PR round").await;
    assert!(!rig.screen().iter().any(|l| l.contains("act_pr_q")));
    rig.press(&[KeyCode::Esc]).await;
}

/// A daemon that does not know `work.watch` costs the operator no word and no block.
#[tokio::test]
async fn an_older_daemon_has_no_threads_block() {
    let world = harbour_world();
    world
        .lock()
        .unwrap()
        .answers
        .insert("work.watch".into(), Err((-32601, "unknown method".into())));
    let mut rig = Rig::new(120, 24, script(world));
    rig.asked("work.watch", 1).await;
    rig.settle().await;
    let screen = rig.screen().join("\n");
    assert!(
        !screen.contains("unknown method") && !screen.contains('▾'),
        "{screen}"
    );
    rig.press(&[KeyCode::Char('T')]).await;
    assert!(rig
        .screen()
        .iter()
        .any(|l| l.contains("no threads on the board")));
}
