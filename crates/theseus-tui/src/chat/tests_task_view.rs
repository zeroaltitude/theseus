//! Inspect, join and return (theseus-rrkb7) over a scripted daemon: the
//! picker's keys, `j` sending `turn.submit` to the task's session, Esc back
//! and the next line going to main, a task that ends while joined, and the
//! header.

use std::sync::Arc;

use crossterm::event::KeyCode;
use serde_json::{json, Value};
use theseus_protocol::error_code::NOT_FOUND;
use theseus_protocol::work::WorkView;

use super::tests::{script, Rig, SID, T0};
use super::tests_tasks::{board, conversation, push, task, view};
use crate::tests::{Answer, Script};

/// Task 1's session, and the end of it a person types.
const TASK: &str = "ses_task1abc1f";
const SHORT: &str = "1abc1f";
const TITLE: &str = "Sound the north channel";

fn running() -> WorkView {
    task(1, TITLE, 5)
}

/// The same task, reported.
fn reported(position: u64) -> WorkView {
    view(json!({
        "id": "exe_t1", "session_id": TASK, "title": TITLE, "position": position,
        "state": "done", "ended_at_ms": T0 - 1_000,
        "attention": {"level": "idle", "label": "done", "since_ms": T0},
        "report": {"outcome": "done", "first_line": "the channel is sounded"}
    }))
}

fn node(sid: &str, pos: u64, kind: &str, text: &str, turn: &str) -> Value {
    json!({
        "node_id": format!("nod_{pos}"), "kind": kind, "session_id": sid, "position": pos,
        "at_unix_ms": T0 - 1000, "text": text, "detail": null, "bytes": 0, "turn_id": turn,
    })
}

/// A daemon with the conversation, one running task, and a history for each.
/// `ended`: a task that refuses a line as one that has reported.
fn daemon(ended: bool) -> Script {
    let base = script();
    let snapshot = board(&[conversation(), running()]);
    Arc::new(move |m, p| -> Answer {
        let sid = p["session_id"].as_str().unwrap_or_default();
        match m {
            "work.watch" => Ok(snapshot.clone()),
            "session.open" => base(m, p).map(|mut v| {
                v["execution_id"] = json!("exe_conv");
                v
            }),
            "session.history" => {
                let (title, nodes) = if sid == TASK {
                    (
                        TITLE,
                        vec![
                            node(TASK, 10, "user_message", "sound the channel", "trn_t1"),
                            node(
                                TASK,
                                11,
                                "assistant_message",
                                "I have read the chart",
                                "trn_t1",
                            ),
                        ],
                    )
                } else {
                    (
                        "Regatta prep",
                        vec![node(SID, 1, "user_message", "start the survey", "trn_m1")],
                    )
                };
                Ok(json!({
                    "session": {
                        "session_id": sid, "kind": "conversation", "label": null, "title": title,
                        "created_at_unix_ms": T0, "turns": 1,
                    },
                    "nodes": nodes, "pending_confirms": [],
                }))
            }
            "turn.submit" if ended && sid == TASK => Err((NOT_FOUND, "the task has ended".into())),
            "turn.inspect" => Ok(json!({"url": "https://cockpit.example/turn/trn_t1"})),
            _ => base(m, p),
        }
    })
}

async fn ready(ended: bool) -> Rig {
    let mut r = Rig::with(100, 40, daemon(ended)).await;
    r.ready().await;
    r.asked("work.watch", 1).await;
    r.settle().await;
    r
}

/// Open the picker and inspect the one task.
async fn inspecting(r: &mut Rig) {
    r.ctrl('t').await;
    r.press(KeyCode::Enter).await;
    r.shows("read-only").await;
}

/// Open the picker and join the one task.
async fn joined(r: &mut Rig) {
    r.ctrl('t').await;
    r.type_text("j").await;
    r.asked("session.history", 1).await;
    r.shows(&format!("{SHORT} ›")).await;
}

fn submits(r: &Rig) -> Vec<Value> {
    r.daemon.asked("turn.submit")
}

// ------------------------------------------------------------ the picker

#[tokio::test(start_paused = true)]
async fn ctrl_t_opens_a_picker_and_esc_closes_it_without_asking_anything() {
    let mut r = ready(false).await;
    r.ctrl('t').await;
    r.shows("↑↓ move · enter inspect · j join · c cancel · a answer · esc close")
        .await;
    assert!(
        r.text().contains(&format!("{SHORT} {TITLE}")),
        "{}",
        r.text()
    );
    r.press(KeyCode::Esc).await;
    assert!(!r.text().contains("↑↓ move"));
    assert!(r.daemon.asked("session.history").is_empty());
    assert!(submits(&r).is_empty());
}

#[tokio::test(start_paused = true)]
async fn enter_in_the_picker_inspects_the_task_read_only() {
    let mut r = ready(false).await;
    inspecting(&mut r).await;
    r.asked("session.history", 1).await;
    let h = &r.daemon.asked("session.history")[0];
    assert_eq!(h["session_id"], TASK);
    r.asked("session.watch", 2).await;
    assert_eq!(r.daemon.asked("session.watch")[1]["session_id"], TASK);
    r.shows("I have read the chart").await;
    // The header names the task, what it does, and the keys.
    r.shows(&format!("── ⚙ task {SHORT} · {TITLE} · running"))
        .await;
    r.shows("j join · i inspector · Esc back to main").await;
    // Nothing typed reaches the task: the line is kept, and the screen says why.
    r.type_text("hello").await;
    r.press(KeyCode::Enter).await;
    r.shows("read-only here").await;
    assert!(submits(&r).is_empty());
}

#[tokio::test(start_paused = true)]
async fn j_joins_from_the_picker_and_a_line_goes_to_the_tasks_session() {
    let mut r = ready(false).await;
    joined(&mut r).await;
    r.type_text("turn north first").await;
    r.press(KeyCode::Enter).await;
    r.asked("turn.submit", 1).await;
    let s = &submits(&r)[0];
    assert_eq!(s["session_id"], TASK);
    assert_eq!(s["input"], "turn north first");
    assert_eq!(s["author"], "theseus chat");
    assert!(
        s.get("profile").is_none(),
        "the conversation's profile stays behind"
    );
    r.shows("↪ the task reads this at its next step").await;
}

/// Joined, the line is for typing: a message that starts with an inspect
/// key (`f`, `i`, `j`) is the message, never the command.
#[tokio::test(start_paused = true)]
async fn a_joined_line_that_starts_with_an_inspect_key_is_the_message() {
    let mut r = ready(false).await;
    joined(&mut r).await;
    r.type_text("fix the jib first").await;
    r.press(KeyCode::Enter).await;
    r.asked("turn.submit", 1).await;
    assert_eq!(submits(&r)[0]["input"], "fix the jib first");
    assert!(r.daemon.asked("task.retry").is_empty());
}

#[tokio::test(start_paused = true)]
async fn a_line_to_a_joined_task_is_sent_while_its_turn_runs_never_queued() {
    let mut r = ready(false).await;
    joined(&mut r).await;
    r.tell(theseus_protocol::Event::TurnStarted(
        theseus_protocol::TurnStarted {
            session_id: TASK.into(),
            turn_id: "trn_t1".into(),
            ..Default::default()
        },
    ));
    r.shows("thinking").await;
    r.type_text("mind the shoal").await;
    r.press(KeyCode::Enter).await;
    r.asked("turn.submit", 1).await;
    assert_eq!(submits(&r)[0]["session_id"], TASK);
    assert!(!r.text().contains("queued"), "{}", r.text());
}

#[tokio::test(start_paused = true)]
async fn j_on_an_empty_line_joins_a_task_that_was_only_inspected() {
    let mut r = ready(false).await;
    inspecting(&mut r).await;
    r.type_text("j").await;
    r.shows(&format!("{SHORT} ›")).await;
    r.type_text("go").await;
    r.press(KeyCode::Enter).await;
    r.asked("turn.submit", 1).await;
    assert_eq!(submits(&r)[0]["session_id"], TASK);
}

// ------------------------------------------------------------ returning

#[tokio::test(start_paused = true)]
async fn esc_goes_back_and_the_next_line_goes_to_the_conversation() {
    let mut r = ready(false).await;
    joined(&mut r).await;
    r.press(KeyCode::Esc).await;
    // The conversation is read again, and the prompt is its own.
    r.until("main's history", |r| {
        r.daemon
            .asked("session.history")
            .iter()
            .any(|h| h["session_id"] == SID)
    })
    .await;
    r.shows("── back in main").await;
    assert!(!r.text().contains(&format!("{SHORT} ›")), "{}", r.text());
    r.type_text("and the tide table").await;
    r.press(KeyCode::Enter).await;
    r.asked("turn.submit", 1).await;
    assert_eq!(submits(&r)[0]["session_id"], SID);
    assert_eq!(submits(&r)[0]["input"], "and the tide table");
}

#[tokio::test(start_paused = true)]
async fn the_return_line_says_what_main_did_while_you_were_away() {
    let mut r = ready(false).await;
    inspecting(&mut r).await;
    // The conversation answers while the task is on screen: counted, not printed.
    r.starts_turn("trn_m2");
    r.answers("trn_m2", "the survey is done");
    r.ends_turn("trn_m2", "the survey is done");
    r.settle().await;
    assert!(!r.text().contains("the survey is done"), "{}", r.text());
    r.press(KeyCode::Esc).await;
    r.shows(&format!(
        "── back in main · {SHORT} still running · main: 1 reply while you were away ──"
    ))
    .await;
}

#[tokio::test(start_paused = true)]
async fn esc_in_a_task_never_stops_anything_and_slash_stop_asks_twice() {
    let mut r = ready(false).await;
    joined(&mut r).await;
    r.tell(theseus_protocol::Event::TurnStarted(
        theseus_protocol::TurnStarted {
            session_id: TASK.into(),
            turn_id: "trn_t1".into(),
            ..Default::default()
        },
    ));
    r.shows("thinking").await;
    r.type_text("x").await;
    r.press(KeyCode::Esc).await;
    r.settle().await;
    assert!(r.daemon.asked("execution.stop").is_empty());
    assert!(
        r.text().contains(&format!("{SHORT} ›")),
        "a line is not an empty line"
    );
    // /stop asks first; the second cancels the task, by its short id.
    r.ctrl('u').await;
    r.type_text("/stop").await;
    r.press(KeyCode::Enter).await;
    r.shows("cancel task").await;
    assert!(r.daemon.asked("task.cancel").is_empty());
    r.type_text("/stop").await;
    r.press(KeyCode::Enter).await;
    r.asked("task.cancel", 1).await;
    assert_eq!(r.daemon.asked("task.cancel")[0]["task"], SHORT);
    assert!(r.daemon.asked("execution.stop").is_empty());
}

#[tokio::test(start_paused = true)]
async fn back_and_the_task_commands_work_from_the_line() {
    let mut r = ready(false).await;
    r.type_text(&format!("/join {SHORT}")).await;
    r.press(KeyCode::Enter).await;
    r.shows(&format!("{SHORT} ›")).await;
    r.type_text("/back").await;
    r.press(KeyCode::Enter).await;
    r.shows("── back in main").await;
    r.type_text(&format!("/task {SHORT}")).await;
    r.press(KeyCode::Enter).await;
    r.shows("read-only").await;
    r.type_text("/inspect").await;
    r.press(KeyCode::Enter).await;
    r.shows("inspect https://cockpit.example/turn/trn_t1").await;
    assert_eq!(r.daemon.asked("turn.inspect")[0]["turn_id"], "trn_t1");
    // A name too short, or none, is said and nothing opens.
    r.press(KeyCode::Esc).await;
    r.type_text("/task 1a").await;
    r.press(KeyCode::Enter).await;
    r.shows("too short").await;
}

// ------------------------------------------------------------ the end

#[tokio::test(start_paused = true)]
async fn a_task_that_ends_while_joined_sends_the_next_line_to_main_with_the_shared_line() {
    let mut r = ready(false).await;
    joined(&mut r).await;
    push(&r, 20, vec![reported(20)]);
    r.shows(&format!("🏁 {SHORT} reported")).await;
    r.shows("main ›").await;
    r.type_text("thanks, now the tide table").await;
    r.press(KeyCode::Enter).await;
    r.asked("turn.submit", 1).await;
    let s = &submits(&r)[0];
    assert_eq!(s["session_id"], SID);
    assert_eq!(
        s["input"],
        format!(
            "[Written to the task “{TITLE}” (…{SHORT}), and the task is over] thanks, now the tide table"
        )
    );
    r.shows("sent to the conversation that started it").await;
}

#[tokio::test(start_paused = true)]
async fn a_line_that_races_the_end_is_not_lost_it_goes_to_main() {
    // The daemon refuses the line as the task's end landed: the view had not heard.
    let mut r = ready(true).await;
    joined(&mut r).await;
    r.type_text("one more thing").await;
    r.press(KeyCode::Enter).await;
    r.asked("turn.submit", 2).await;
    let s = submits(&r);
    assert_eq!(s[0]["session_id"], TASK);
    assert_eq!(s[1]["session_id"], SID);
    assert!(s[1]["input"]
        .as_str()
        .unwrap()
        .ends_with("and the task is over] one more thing"));
}

#[test]
fn only_a_task_gone_by_class_is_redirected_and_any_other_refusal_is_said() {
    let err = |class: &str| -> theseus_protocol::RpcError {
        serde_json::from_value(
            json!({"code": -32000, "message": "refused", "data": {"class": class}}),
        )
        .unwrap()
    };
    // The kernel's own words for an ended execution.
    let gone = err("execution_complete");
    assert!(gone.data["class"]
        .as_str()
        .unwrap()
        .starts_with("execution_"));
    assert!(!err("budget").data["class"]
        .as_str()
        .unwrap()
        .starts_with("execution_"));
}

#[tokio::test(start_paused = true)]
async fn f_offers_a_follow_up_for_an_ended_task_and_only_then() {
    let mut r = ready(false).await;
    inspecting(&mut r).await;
    r.type_text("f").await;
    r.shows("the task has not ended").await;
    assert!(r.daemon.asked("task.retry").is_empty());
    push(&r, 20, vec![reported(20)]);
    r.shows("f follow up · i inspector · Esc back to main")
        .await;
    r.type_text("f").await;
    r.asked("task.retry", 1).await;
    assert_eq!(r.daemon.asked("task.retry")[0]["execution_id"], "exe_t1");
}

// ------------------------------------------------------------ notices

#[tokio::test(start_paused = true)]
async fn the_task_on_screen_rings_nothing_and_the_conversations_question_still_does() {
    let mut r = ready(false).await;
    inspecting(&mut r).await;
    let notice = |work: Value, position: u64| {
        json!({
            "urgency": "interrupt", "why": "asked", "work": work,
            "root": {"id": "exe_conv", "kind": "conversation", "session_id": SID, "title": ""},
            "line": "x", "position": position, "at_ms": T0
        })
    };
    let task_work = json!({"id": "exe_t1", "kind": "task", "session_id": TASK, "title": TITLE});
    let main_work =
        json!({"id": "exe_conv", "kind": "conversation", "session_id": SID, "title": ""});
    let mark = r.bytes();
    r.daemon.notify("work.notice", notice(task_work, 30));
    r.settle().await;
    assert!(
        !r.written_since(mark).contains('\x07'),
        "the task on screen rang"
    );
    let mark = r.bytes();
    r.daemon.notify("work.notice", notice(main_work, 31));
    r.settle().await;
    assert_eq!(r.written_since(mark).matches('\x07').count(), 1);
}

// ------------------------------------------------------------ the picker's other keys

#[tokio::test(start_paused = true)]
async fn c_in_the_picker_cancels_on_the_second_press_only() {
    let mut r = ready(false).await;
    r.ctrl('t').await;
    r.type_text("c").await;
    r.shows("c again cancels").await;
    assert!(r.daemon.asked("task.cancel").is_empty());
    r.type_text("c").await;
    r.asked("task.cancel", 1).await;
    assert_eq!(r.daemon.asked("task.cancel")[0]["task"], SHORT);
    // Any other key between the two keeps the task.
    let mut r = ready(false).await;
    r.ctrl('t').await;
    r.type_text("c").await;
    r.press(KeyCode::Down).await;
    r.type_text("c").await;
    r.settle().await;
    assert!(r.daemon.asked("task.cancel").is_empty());
}

// ------------------------------------------------------------ cold open

fn listing() -> Value {
    json!({"sessions": [
        {
            "session_id": SID, "kind": "conversation", "label": null, "title": "Regatta prep",
            "created_at_unix_ms": T0, "turns": 2, "execution_id": "exe_conv", "cost_usd": 1.5,
        },
        {
            "session_id": TASK, "kind": "task", "label": "task", "title": TITLE,
            "created_at_unix_ms": T0, "turns": 1, "execution_id": "exe_t1",
            "parent_session_id": SID,
        },
    ]})
}

async fn cold(given: &str, join: bool) -> Rig {
    let base = daemon(false);
    let script: Script = Arc::new(move |m, p| match m {
        "session.list" => Ok(listing()),
        _ => base(m, p),
    });
    let given = given.to_string();
    let mut r = Rig::with_cfg(100, 40, script, move |c| c.task = Some((given, join))).await;
    r.shows(TITLE).await;
    r
}

#[tokio::test(start_paused = true)]
async fn chat_task_opens_the_task_joined_with_its_conversation_behind_it() {
    let mut r = cold(SHORT, true).await;
    r.shows(&format!("{SHORT} ›")).await;
    // No conversation of its own was opened: the task's parent is the main.
    assert!(r.daemon.asked("session.open").is_empty());
    let history = r.daemon.asked("session.history");
    assert!(
        history.iter().all(|h| h["session_id"] == TASK),
        "{history:?}"
    );
    r.press(KeyCode::Esc).await;
    r.shows("── back in main").await;
    assert!(r
        .daemon
        .asked("session.history")
        .iter()
        .any(|h| h["session_id"] == SID));
}

#[tokio::test(start_paused = true)]
async fn chat_task_inspect_is_read_only_and_an_unknown_task_is_said() {
    let mut r = cold("bc1f", false).await;
    r.shows("read-only").await;
    r.type_text("hi").await;
    r.press(KeyCode::Enter).await;
    r.settle().await;
    assert!(submits(&r).is_empty());
    let mut r = Rig::with_cfg(
        100,
        40,
        {
            let base = daemon(false);
            Arc::new(move |m, p| match m {
                "session.list" => Ok(listing()),
                _ => base(m, p),
            })
        },
        |c| c.task = Some(("nothere".into(), true)),
    )
    .await;
    r.shows("no task `nothere`").await;
}

// ------------------------------------------------------------ the header

fn golden(name: &str, got: &str) {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/golden")
        .join(name);
    if std::env::var("THESEUS_GOLDEN").as_deref() == Ok("write") {
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, got).unwrap();
    }
    let want = std::fs::read_to_string(&path)
        .unwrap_or_else(|_| panic!("no golden {name}; THESEUS_GOLDEN=write writes it"));
    assert_eq!(
        got, want,
        "{name} differs; THESEUS_GOLDEN=write rewrites it"
    );
}

/// The dock's rows: the header, the prompt and the status line.
fn dock(r: &Rig) -> String {
    let rows = r.screen();
    let at = rows
        .iter()
        .position(|l| l.starts_with("── ⚙ task"))
        .unwrap();
    rows[at..].join("\n")
}

#[tokio::test(start_paused = true)]
async fn the_header_is_a_golden_inspecting_joined_and_ended() {
    let mut r = ready(false).await;
    inspecting(&mut r).await;
    let inspect = dock(&r);
    r.type_text("j").await;
    let join = dock(&r);
    push(&r, 20, vec![reported(20)]);
    r.shows("🏁").await;
    let ended = dock(&r);
    golden(
        "chat_task_view_100.txt",
        &format!("{inspect}\n\n{join}\n\n{ended}\n"),
    );
}

/// The header carries the one progress line's words (theseus-ypnxt.2): a
/// paused task says `paused` first, and the step, dollars and time left follow.
#[tokio::test(start_paused = true)]
async fn the_header_says_paused_and_what_the_progress_line_says() {
    let mut r = ready(false).await;
    inspecting(&mut r).await;
    let mut v = running();
    v.position = 30;
    v.paused = true;
    v.progress =
        serde_json::from_value(json!({"steps_done": 2, "steps_total": 5, "at_ms": 1})).ok();
    v.cost = serde_json::from_value(json!({"spent_usd": 0.4, "limit_usd": 2.0})).ok();
    push(&r, 30, vec![v]);
    r.shows("paused · running").await;
    let head = dock(&r).lines().next().unwrap().to_string();
    assert!(head.contains("step 3 of 5 · $0.40 of $2"), "{head}");
}
