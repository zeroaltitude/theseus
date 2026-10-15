//! Coming back (theseus-6yka): `-c` takes this directory's last conversation,
//! never a task's, a DM's or a shared place's; `-r` picks one, searched as
//! you type, and Tab widens it; the last 30 turns are replayed and the older
//! ones folded; each directory keeps its own history.

use std::sync::Arc;

use crossterm::event::KeyCode;
use serde_json::{json, Value};

use super::state::Start;
use super::tests::{script, Rig, SID, T0};
use crate::tests::{Answer, Script};

const DIR: &str = "/home/ada/work/harbour-tides";

/// A session as `session.list` answers it.
fn session(sid: &str, title: &str, extra: Value) -> Value {
    let mut v = json!({
        "session_id": sid, "kind": "conversation", "label": null, "title": title,
        "created_at_unix_ms": T0 - 1000, "last_active_ms": T0 - 500, "turns": 3,
        "cost_usd": 0.42, "dir": DIR, "started_on": "cli",
        "first_text": title, "last_text": "done",
    });
    for (k, x) in extra.as_object().cloned().unwrap_or_default() {
        v[k] = x;
    }
    v
}

fn node(pos: u64, kind: &str, text: &str) -> Value {
    json!({
        "node_id": format!("nod_{pos}"), "kind": kind, "session_id": "ses_old000001",
        "position": pos, "at_unix_ms": T0 - 1000, "text": text, "detail": null, "bytes": 0,
    })
}

/// `turns` turns of a prompt and a reply, positions from 100.
fn transcript(turns: u64) -> Vec<Value> {
    (0..turns)
        .flat_map(|t| {
            [
                node(100 + 2 * t, "user_message", &format!("prompt number {t}")),
                node(
                    101 + 2 * t,
                    "assistant_message",
                    &format!("reply number {t}"),
                ),
            ]
        })
        .collect()
}

/// A daemon that lists `sessions` and answers a history with `nodes`.
fn daemon_with(sessions: Vec<Value>, wide: Vec<Value>, nodes: Vec<Value>, turns: u64) -> Script {
    let base = script();
    Arc::new(move |m, p| -> Answer {
        match m {
            "session.list" if p.get("dir").is_none() => Ok(json!({ "sessions": wide })),
            "session.list" => Ok(json!({ "sessions": sessions })),
            "session.history" => {
                let older = p.get("before").is_some().then_some(1);
                let mut r = json!({
                    "session": session("ses_old000001", "the earlier work", json!({"turns": turns})),
                    "nodes": nodes, "pending_confirms": [],
                });
                if let Some(o) = older {
                    r["older"] = json!(o);
                }
                Ok(r)
            }
            _ => base(m, p),
        }
    })
}

impl Rig {
    /// Step until the chat has written `text` (it may have scrolled off the
    /// screen the terminal emulator reads back).
    async fn written(&mut self, text: &str) {
        let t = text.to_string();
        self.until(&format!("{text:?} to be written"), move |r| {
            r.written_since(0).contains(&t)
        })
        .await;
    }
}

async fn started(start: Start, script: Script) -> Rig {
    Rig::with_cfg(100, 40, script, |c| c.start = start).await
}

fn sids(asks: &[Value]) -> Vec<String> {
    asks.iter()
        .map(|p| p["session_id"].as_str().unwrap_or_default().to_string())
        .collect()
}

#[tokio::test(start_paused = true)]
async fn continue_takes_this_directorys_last_never_a_task_a_dm_or_a_shared_place() {
    let sessions = vec![
        // The newest of all is a task's session, then a DM's, then a place's.
        session(
            "ses_task00001",
            "a task",
            json!({"kind": "task", "last_active_ms": T0 - 10}),
        ),
        session(
            "ses_dm0000001",
            "a dm",
            json!({"started_on": "discord", "last_active_ms": T0 - 20}),
        ),
        session(
            "ses_place00001",
            "a place",
            json!({"started_on": "discord", "last_active_ms": T0 - 30}),
        ),
        session(
            "ses_empty00001",
            "no turn yet",
            json!({"turns": 0, "last_active_ms": T0 - 40}),
        ),
        session(
            "ses_old000001",
            "the earlier work",
            json!({"last_active_ms": T0 - 100}),
        ),
        session(
            "ses_older00001",
            "older still",
            json!({"last_active_ms": T0 - 900}),
        ),
    ];
    let mut r = started(
        Start::Continue,
        daemon_with(sessions, vec![], transcript(2), 2),
    )
    .await;
    r.asked("session.watch", 1).await;
    r.shows("prompt number 1").await;
    assert_eq!(sids(&r.daemon.asked("session.watch")), ["ses_old000001"]);
    assert!(
        r.daemon.asked("session.open").is_empty(),
        "no new conversation"
    );
    let list = &r.daemon.asked("session.list")[0];
    assert_eq!(list["dir"], DIR, "the question is of this directory");
}

#[tokio::test(start_paused = true)]
async fn continue_with_none_says_so_and_opens_a_new_conversation() {
    let only_a_task = vec![session("ses_task00001", "a task", json!({"kind": "task"}))];
    let mut r = started(Start::Continue, daemon_with(only_a_task, vec![], vec![], 0)).await;
    r.asked("session.open", 1).await;
    r.shows("no conversation here yet · starting a new one")
        .await;
    assert_eq!(r.daemon.asked("session.open")[0]["dir"], DIR);
    r.asked("session.watch", 1).await;
    assert_eq!(sids(&r.daemon.asked("session.watch")), [SID]);
}

#[tokio::test(start_paused = true)]
async fn the_picker_filters_by_a_word_of_a_message_and_enter_continues_it() {
    let mut a = session("ses_aaaa000001", "survey the log lines", json!({}));
    a["last_text"] = json!("found three slow requests");
    let mut b = session("ses_bbbb000001", "fix the parser", json!({}));
    b["first_text"] = json!("the parser drops trailing commas");
    let mut r = started(
        Start::Pick,
        daemon_with(vec![a, b], vec![], transcript(1), 1),
    )
    .await;
    r.shows("resume a conversation").await;
    r.shows("survey the log lines").await;
    r.shows("fix the parser").await;
    assert!(
        r.daemon.asked("session.open").is_empty(),
        "nothing is opened yet"
    );
    // A word that is in a first message and in no title.
    r.type_text("commas").await;
    assert!(r.text().contains("fix the parser"), "{}", r.text());
    assert!(!r.text().contains("survey the log lines"), "{}", r.text());
    r.press(KeyCode::Enter).await;
    r.asked("session.history", 1).await;
    r.asked("session.watch", 1).await;
    assert_eq!(sids(&r.daemon.asked("session.watch")), ["ses_bbbb000001"]);
    assert!(
        !r.text().contains("resume a conversation"),
        "the picker closes"
    );
}

#[tokio::test(start_paused = true)]
async fn a_search_reads_no_more_from_the_daemon() {
    let sessions = (0..20)
        .map(|n| {
            session(
                &format!("ses_{n:010}"),
                &format!("conversation {n}"),
                json!({}),
            )
        })
        .collect();
    let mut r = started(Start::Pick, daemon_with(sessions, vec![], vec![], 1)).await;
    r.shows("conversation 7").await;
    let asked = r.daemon.methods().len();
    r.type_text("conversation 1").await;
    r.press(KeyCode::Backspace).await;
    r.press(KeyCode::Down).await;
    assert_eq!(r.daemon.methods().len(), asked, "{:?}", r.daemon.methods());
}

#[tokio::test(start_paused = true)]
async fn tab_widens_the_picker_to_every_directory_and_back() {
    let here = vec![session("ses_here000001", "tides work", json!({}))];
    let elsewhere = vec![
        session("ses_here000001", "tides work", json!({})),
        session(
            "ses_else000001",
            "reef survey",
            json!({"dir": "/home/ada/work/reef"}),
        ),
    ];
    let mut r = started(Start::Pick, daemon_with(here, elsewhere, vec![], 1)).await;
    r.shows("tides work").await;
    assert!(!r.text().contains("reef survey"), "{}", r.text());
    r.press(KeyCode::Tab).await;
    r.shows("reef survey").await;
    r.shows("every directory").await;
    let wide = r.daemon.asked("session.list").pop().unwrap();
    assert!(
        wide.get("dir").is_none() && wide["with_text"] == true,
        "{wide}"
    );
    r.press(KeyCode::Tab).await;
    assert!(!r.text().contains("reef survey"), "{}", r.text());
    assert_eq!(
        r.daemon.asked("session.list").len(),
        2,
        "the wide list is read once"
    );
}

#[tokio::test(start_paused = true)]
async fn esc_leaves_the_picker_for_a_new_conversation() {
    let mut r = started(Start::Pick, daemon_with(vec![], vec![], vec![], 0)).await;
    r.shows("no conversation here yet").await;
    r.press(KeyCode::Esc).await;
    r.asked("session.open", 1).await;
    r.asked("session.watch", 1).await;
    assert_eq!(sids(&r.daemon.asked("session.watch")), [SID]);
}

#[tokio::test(start_paused = true)]
async fn the_last_thirty_turns_are_replayed_and_the_older_ones_folded() {
    let sessions = vec![session("ses_old000001", "the earlier work", json!({}))];
    let mut r = started(
        Start::Continue,
        daemon_with(sessions, vec![], transcript(35), 35),
    )
    .await;
    r.asked("session.watch", 1).await;
    r.written("5 earlier turns · /history shows them").await;
    let out = r.written_since(0);
    assert!(
        out.contains("prompt number 5"),
        "turn 6 is the first replayed"
    );
    assert!(out.contains("reply number 34"));
    assert!(!out.contains("prompt number 4"), "older turns are folded");
    assert_eq!(out.matches("› prompt number").count(), 30);
    assert_eq!(
        r.daemon.asked("session.history")[0]["session_id"],
        "ses_old000001"
    );
}

#[tokio::test(start_paused = true)]
async fn a_short_conversation_is_replayed_whole_with_no_fold() {
    let sessions = vec![session("ses_old000001", "the earlier work", json!({}))];
    let mut r = started(
        Start::Continue,
        daemon_with(sessions, vec![], transcript(4), 4),
    )
    .await;
    r.asked("session.watch", 1).await;
    r.shows("reply number 3").await;
    let out = r.written_since(0);
    assert!(out.contains("prompt number 0"));
    assert!(!out.contains("earlier turn"), "nothing is folded");
}

#[tokio::test(start_paused = true)]
async fn history_prints_the_folded_turns_from_the_cursor() {
    let sessions = vec![session("ses_old000001", "the earlier work", json!({}))];
    let mut r = started(
        Start::Continue,
        daemon_with(sessions, vec![], transcript(35), 35),
    )
    .await;
    r.asked("session.watch", 1).await;
    r.written("5 earlier turns").await;
    r.type_text("/history").await;
    r.press(KeyCode::Enter).await;
    r.asked("session.history", 2).await;
    let ask = &r.daemon.asked("session.history")[1];
    assert_eq!(
        ask["before"], 110,
        "from the first replayed node, turn 6's prompt"
    );
    r.shows("earlier turn").await;
}

#[tokio::test(start_paused = true)]
async fn history_with_nothing_folded_says_so() {
    let mut r = Rig::new(100, 30).await;
    r.ready().await;
    r.type_text("/history").await;
    r.press(KeyCode::Enter).await;
    r.shows("no earlier turns to show").await;
    assert!(r.daemon.asked("session.history").is_empty());
}

#[tokio::test(start_paused = true)]
async fn each_directory_keeps_its_own_up_arrow_history() {
    let shared = tempfile::tempdir().unwrap();
    let folder = shared.path().join("history");
    for (dir, said) in [("/work/tides", "alpha tides"), ("/work/reef", "beta reef")] {
        let at = folder.clone();
        let mut r = Rig::with_cfg(80, 24, script(), |c| {
            c.dir = dir.into();
            c.history_path = Some(at);
        })
        .await;
        r.ready().await;
        r.type_text(said).await;
        r.press(KeyCode::Enter).await;
        r.settle().await;
        // The file is written off the loop's thread.
        for _ in 0..50 {
            if super::env::file_for(&folder, dir).exists() {
                break;
            }
            tokio::time::sleep(std::time::Duration::from_millis(20)).await;
        }
    }
    assert_eq!(
        super::env::load_history(&folder, "/work/tides"),
        ["alpha tides"]
    );
    assert_eq!(
        super::env::load_history(&folder, "/work/reef"),
        ["beta reef"]
    );
    // A chat opened in one directory recalls that directory's, with ↑.
    let at = folder.clone();
    let mut r = Rig::with_cfg(80, 24, script(), |c| {
        c.dir = "/work/reef".into();
        c.history = super::env::load_history(&at, "/work/reef");
        c.history_path = Some(at);
    })
    .await;
    r.ready().await;
    r.press(KeyCode::Up).await;
    assert!(r.text().contains("› beta reef"), "{}", r.text());
    assert!(!r.text().contains("alpha tides"));
}
