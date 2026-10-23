//! Starting, pausing and resuming a task from the chat, and the picker's search
//! (theseus-ypnxt.4), over a scripted daemon: `/task new BRIEF` and `make that a
//! task` send one `task.start` and no turn; `/pause` and `/resume` send
//! `task.pause` and `task.resume`; typing in ctrl-t's picker narrows it, and its
//! ended tasks come from one `task.list` read as it opens.

use std::collections::HashMap;
use std::sync::Arc;

use crossterm::event::KeyCode;
use serde_json::{json, Value};
use theseus_protocol::work::WorkView;

use super::tests::{script, Rig, SID, T0};
use super::tests_tasks::{board, conversation, task, view};
use crate::tests::{Answer, Script};

const STARTED: &str = r#"{"task_id": "ses_000000e4f9a1", "session_id": "ses_000000e4f9a1",
    "short": "e4f9a1", "home_session": "ses_chat01", "limit_usd": 0.5}"#;

/// A daemon with the conversation and these tasks on its board; `answers` are
/// what other methods say.
fn daemon(views: &[WorkView], answers: Vec<(&str, Answer)>) -> Script {
    let base = script();
    let snapshot = board(views);
    let answers: HashMap<String, Answer> = answers
        .into_iter()
        .map(|(m, a)| (m.to_string(), a))
        .collect();
    Arc::new(move |m, p| -> Answer {
        if let Some(a) = answers.get(m) {
            return a.clone();
        }
        match m {
            "work.watch" => Ok(snapshot.clone()),
            "session.open" => base(m, p).map(|mut v| {
                v["execution_id"] = json!("exe_conv");
                v
            }),
            _ => base(m, p),
        }
    })
}

fn started() -> Answer {
    Ok(serde_json::from_str(STARTED).unwrap())
}

fn held(paused: bool, changed: bool) -> Answer {
    Ok(json!({"task_id": "ses_task1abc1f", "paused": paused, "changed": changed}))
}

async fn ready(views: &[WorkView], answers: Vec<(&str, Answer)>) -> Rig {
    let mut r = Rig::with(100, 30, daemon(views, answers)).await;
    r.ready().await;
    r.asked("work.watch", 1).await;
    r.settle().await;
    r
}

async fn enter(r: &mut Rig, line: &str) {
    r.type_text(line).await;
    r.press(KeyCode::Enter).await;
}

// ------------------------------------------------------------ /task new

#[tokio::test(start_paused = true)]
async fn task_new_starts_a_task_under_this_conversation_with_no_turn() {
    let mut r = ready(&[conversation()], vec![("task.start", started())]).await;
    enter(&mut r, "/task new Survey the north quay").await;
    r.asked("task.start", 1).await;
    // The brief, this conversation, the chat as author; no profile and no
    // budget, so the daemon's defaults apply.
    assert_eq!(
        r.daemon.asked("task.start"),
        [json!({"brief": "Survey the north quay", "home": SID, "author": "theseus chat"})]
    );
    r.shows("◐ started Survey the north quay (e4f9a1)").await;
    r.shows("/task e4f9a1 to look, /join e4f9a1 to talk to it")
        .await;
    assert!(r.daemon.asked("turn.submit").is_empty());
}

#[tokio::test(start_paused = true)]
async fn a_refused_start_prints_the_daemons_words() {
    let refused: Answer = Err((-32005, "the budget has nothing left to carve".into()));
    let mut r = ready(&[conversation()], vec![("task.start", refused)]).await;
    enter(&mut r, "/task new Survey the north quay").await;
    r.shows("✗ the budget has nothing left to carve").await;
}

#[tokio::test(start_paused = true)]
async fn task_alone_and_task_with_an_id_behave_as_before() {
    let mut r = ready(&[conversation(), task(1, "Buoy survey", 3)], vec![]).await;
    enter(&mut r, "/task abc1f").await;
    r.asked("session.history", 1).await;
    r.shows("read-only").await;
    assert!(r.daemon.asked("task.start").is_empty());
    r.press(KeyCode::Esc).await;
    r.settle().await;
    assert!(!r.text().contains("(read-only) ›"), "{}", r.text());
    enter(&mut r, "/task ").await;
    r.asked("task.list", 1).await;
    r.settle().await;
    r.shows("tasks · type to search").await;
    assert!(r.daemon.asked("task.start").is_empty());
}

#[tokio::test(start_paused = true)]
async fn task_new_with_nothing_to_start_from_says_so_and_sends_nothing() {
    let mut r = ready(&[conversation()], vec![]).await;
    enter(&mut r, "/task new").await;
    r.shows("no message of yours here yet · /task new BRIEF")
        .await;
    r.advance(50).await;
    assert!(r.daemon.asked("task.start").is_empty());
    assert!(r.daemon.asked("turn.submit").is_empty());
}

// ------------------------------------------------------ make that a task

#[tokio::test(start_paused = true)]
async fn make_that_a_task_takes_the_owners_last_message_as_the_brief() {
    let mut r = ready(&[conversation()], vec![("task.start", started())]).await;
    enter(&mut r, "Survey the north quay\nand count the buoys").await;
    r.asked("turn.submit", 1).await;
    // The model answers; its reply is not the brief.
    r.starts_turn("trn_1");
    r.answers("trn_1", "The north quay has nine buoys.");
    r.ends_turn("trn_1", "The north quay has nine buoys.");
    r.settle().await;
    enter(&mut r, "Make That A Task").await;
    r.asked("task.start", 1).await;
    let s = &r.daemon.asked("task.start")[0];
    assert_eq!(
        s["brief"],
        json!("Survey the north quay\nand count the buoys")
    );
    assert_eq!(s["home"], json!(SID));
    r.shows("◐ started Survey the north quay (e4f9a1)").await;
    // The words were a command: no second turn.
    assert_eq!(r.daemon.asked("turn.submit").len(), 1);
}

#[tokio::test(start_paused = true)]
async fn task_new_alone_starts_from_the_last_message_too() {
    let mut r = ready(&[conversation()], vec![("task.start", started())]).await;
    enter(&mut r, "Sound the north channel").await;
    r.asked("turn.submit", 1).await;
    enter(&mut r, "/task new").await;
    r.asked("task.start", 1).await;
    assert_eq!(
        r.daemon.asked("task.start")[0]["brief"],
        json!("Sound the north channel")
    );
}

#[tokio::test(start_paused = true)]
async fn make_that_a_task_with_no_message_before_it_says_so() {
    let mut r = ready(&[conversation()], vec![]).await;
    enter(&mut r, "make that a task").await;
    r.shows("no message of yours here yet").await;
    r.advance(50).await;
    assert!(r.daemon.asked("task.start").is_empty());
    assert!(r.daemon.asked("turn.submit").is_empty());
}

#[tokio::test(start_paused = true)]
async fn make_that_a_task_please_is_an_ordinary_message() {
    let mut r = ready(&[conversation()], vec![("task.start", started())]).await;
    enter(&mut r, "Sound the north channel").await;
    r.asked("turn.submit", 1).await;
    enter(&mut r, "make that a task please").await;
    r.asked("turn.submit", 2).await;
    assert_eq!(
        r.daemon.asked("turn.submit")[1]["input"],
        json!("make that a task please")
    );
    assert!(r.daemon.asked("task.start").is_empty());
}

// ------------------------------------------------------ pause and resume

#[tokio::test(start_paused = true)]
async fn pause_in_a_tasks_view_pauses_that_task() {
    let mut r = ready(
        &[conversation(), task(1, "Buoy survey", 3)],
        vec![("task.pause", held(true, true))],
    )
    .await;
    enter(&mut r, "/task abc1f").await;
    r.shows("read-only").await;
    enter(&mut r, "/pause").await;
    r.asked("task.pause", 1).await;
    assert_eq!(
        r.daemon.asked("task.pause"),
        [json!({"task": "ses_task1abc1f", "author": "theseus chat"})]
    );
    r.shows("Pausing the task 1abc1f").await;
}

#[tokio::test(start_paused = true)]
async fn resume_with_an_id_resumes_that_task() {
    let mut r = ready(
        &[
            conversation(),
            task(1, "Buoy survey", 3),
            task(2, "Tide tables", 4),
        ],
        vec![("task.resume", held(false, true))],
    )
    .await;
    enter(&mut r, "/resume abc1f").await;
    r.asked("task.resume", 1).await;
    assert_eq!(r.daemon.asked("task.resume")[0]["task"], json!("abc1f"));
    assert!(r.daemon.asked("task.pause").is_empty());
    r.shows("Resuming the task abc1f").await;
}

#[tokio::test(start_paused = true)]
async fn pause_alone_acts_on_the_only_running_task() {
    let mut r = ready(
        &[conversation(), task(1, "Buoy survey", 3)],
        vec![("task.pause", held(true, true))],
    )
    .await;
    enter(&mut r, "/pause").await;
    r.asked("task.pause", 1).await;
    assert_eq!(
        r.daemon.asked("task.pause")[0]["task"],
        json!("ses_task1abc1f")
    );
}

#[tokio::test(start_paused = true)]
async fn with_two_running_tasks_and_no_id_nothing_is_sent_and_both_are_listed() {
    let mut r = ready(
        &[
            conversation(),
            task(1, "Buoy survey", 3),
            task(2, "Tide tables", 4),
        ],
        vec![],
    )
    .await;
    enter(&mut r, "/pause").await;
    r.shows("Buoy survey").await;
    r.shows("Tide tables").await;
    r.advance(50).await;
    assert!(r.daemon.asked("task.pause").is_empty());
}

#[tokio::test(start_paused = true)]
async fn resume_alone_finds_the_paused_one_among_running_tasks() {
    let paused = view(json!({
        "id": "exe_t2", "session_id": "ses_task2abc2f", "title": "Tide tables",
        "position": 4, "paused": true
    }));
    let mut r = ready(
        &[conversation(), task(1, "Buoy survey", 3), paused],
        vec![("task.resume", held(false, true))],
    )
    .await;
    enter(&mut r, "/resume").await;
    r.asked("task.resume", 1).await;
    assert_eq!(
        r.daemon.asked("task.resume")[0]["task"],
        json!("ses_task2abc2f")
    );
}

#[tokio::test(start_paused = true)]
async fn a_refused_pause_prints_the_daemons_words() {
    let refused: Answer = Err((-32602, "Task ses_task1abc1f has ended (complete).".into()));
    let mut r = ready(
        &[conversation(), task(1, "Buoy survey", 3)],
        vec![("task.pause", refused)],
    )
    .await;
    enter(&mut r, "/pause abc1f").await;
    r.shows("✗ Task ses_task1abc1f has ended (complete).").await;
}

// ------------------------------------------------------------ the picker

/// The picker's own rows: the footer above it names the same tasks.
fn picker(r: &Rig) -> String {
    let s = r.screen();
    let from = s.iter().position(|l| l.starts_with("tasks ·")).unwrap_or(0);
    let to = s
        .iter()
        .position(|l| l.starts_with("↑↓ move"))
        .unwrap_or(s.len());
    s[from..to].join("\n")
}

fn ended_info(n: usize, title: &str, state: &str) -> Value {
    json!({
        "task_id": format!("ses_old{n}0000{n}z"), "short": format!("old{n}z"),
        "execution_id": format!("exe_old{n}"), "parent_session_id": SID,
        "parent_execution_id": "exe_conv", "title": title, "state": state,
        "spent_usd": 0.4, "limit_usd": 2.0, "cost_usd": 0.4, "turns": 3,
        "created_at_ms": T0 - 3_600_000, "updated_at_ms": T0 - 3_000_000,
    })
}

fn listed(infos: Vec<Value>) -> Answer {
    Ok(json!({"tasks": infos, "records": []}))
}

#[tokio::test(start_paused = true)]
async fn typing_in_the_picker_narrows_it_by_title_and_report() {
    let reported = view(json!({
        "id": "exe_t3", "session_id": "ses_task3abc3f", "title": "Rigging check",
        "position": 6, "state": "done", "ended_at_ms": T0 - 1_000,
        "attention": {"level": "idle", "label": "done", "since_ms": T0},
        "report": {"outcome": "done", "first_line": "the QUAY ropes are sound"}
    }));
    let mut r = ready(
        &[
            conversation(),
            task(1, "Survey the north quay", 3),
            task(2, "Tide tables", 4),
            reported,
        ],
        vec![("task.list", listed(vec![]))],
    )
    .await;
    r.ctrl('t').await;
    r.shows("Tide tables").await;
    r.type_text("quay").await;
    r.shows("search: quay").await;
    // The title of one, the report's first line of another; the third is out.
    let text = r.text();
    let dock = picker(&r);
    assert!(dock.contains("Survey the north quay"), "{text}");
    assert!(dock.contains("Rigging check"), "{text}");
    assert!(!dock.contains("Tide tables"), "{dock}");
    r.type_text(" ropes").await;
    let dock = picker(&r);
    assert!(!dock.contains("Survey the north quay"), "{dock}");
    assert!(dock.contains("Rigging check"), "{dock}");
    // Esc clears the query, then closes.
    r.press(KeyCode::Esc).await;
    r.shows("type to search").await;
    r.press(KeyCode::Esc).await;
    assert!(!r.text().contains("↑↓ move"));
}

#[tokio::test(start_paused = true)]
async fn the_pickers_command_keys_work_until_a_query_starts_and_slash_starts_one() {
    let mut r = ready(
        &[
            conversation(),
            task(1, "Channel buoys", 3),
            task(2, "Anchor drill", 4),
        ],
        vec![("task.list", listed(vec![]))],
    )
    .await;
    r.ctrl('t').await;
    // `c` is cancel while nothing is typed; after `/` it is a letter.
    r.type_text("/cha").await;
    r.shows("search: cha").await;
    let dock = picker(&r);
    assert!(dock.contains("Channel buoys"), "{dock}");
    assert!(!dock.contains("Anchor drill"), "{dock}");
    assert!(r.daemon.asked("task.cancel").is_empty());
}

#[tokio::test(start_paused = true)]
async fn an_ended_task_from_task_list_appears_and_opens_read_only() {
    let mut r = ready(
        &[conversation(), task(1, "Survey the north quay", 3)],
        vec![(
            "task.list",
            listed(vec![ended_info(1, "Chart the old harbour", "complete")]),
        )],
    )
    .await;
    r.ctrl('t').await;
    r.shows("Chart the old harbour").await;
    r.type_text("harbour").await;
    r.press(KeyCode::Enter).await;
    r.asked("session.history", 1).await;
    r.shows("read-only").await;
    assert_eq!(
        r.daemon.asked("session.history")[0]["session_id"],
        json!("ses_old100001z")
    );
}

#[tokio::test(start_paused = true)]
async fn opening_the_picker_reads_task_list_once_not_per_key() {
    let mut r = ready(
        &[conversation(), task(1, "Survey the north quay", 3)],
        vec![("task.list", listed(vec![]))],
    )
    .await;
    r.ctrl('t').await;
    r.asked("task.list", 1).await;
    r.type_text("quay").await;
    r.press(KeyCode::Backspace).await;
    r.press(KeyCode::Down).await;
    r.settle().await;
    assert_eq!(r.daemon.asked("task.list"), [json!({"session_id": SID})]);
}

#[tokio::test(start_paused = true)]
async fn a_picker_over_an_empty_board_shows_the_ended_tasks_task_list_found() {
    let mut r = ready(
        &[conversation()],
        vec![(
            "task.list",
            listed(vec![ended_info(2, "Chart the old harbour", "failed")]),
        )],
    )
    .await;
    r.ctrl('t').await;
    r.shows("Chart the old harbour").await;
    r.shows("tasks · type to search").await;
}

#[tokio::test(start_paused = true)]
async fn nothing_on_the_board_and_nothing_ended_draws_the_tree_as_before() {
    let mut r = ready(&[conversation()], vec![("task.list", listed(vec![]))]).await;
    r.ctrl('t').await;
    r.asked("task.list", 1).await;
    r.settle().await;
    assert!(!r.text().contains("type to search"), "{}", r.text());
}

#[tokio::test(start_paused = true)]
async fn the_picker_counts_a_task_that_ran_out_of_budget_as_ended() {
    let mut r = ready(
        &[conversation()],
        vec![(
            "task.list",
            listed(vec![ended_info(
                3,
                "Dredge the east slip",
                "budget_exhausted",
            )]),
        )],
    )
    .await;
    r.ctrl('t').await;
    r.shows("Dredge the east slip").await;
}
