//! The router's question and its placing, in the chat (theseus-wapgp,
//! theseus-0rfjx). A message the router cannot place waits in the daemon for
//! the owner's word, `[threads] ask_wait_secs` (900 s) at most, and the chat
//! once said nothing of the question: the message was echoed and then held
//! for 15 minutes, with every later message behind it. The chat now shows the
//! question and answers it by its key; a turn the router placed in a thread
//! says so.

use std::sync::Arc;

use crossterm::event::KeyCode;
use serde_json::json;
use theseus_protocol::thread_route::{AskOption, RouteHow, ThreadAsk, ThreadTag};
use theseus_protocol::{Event, ThreadPlaced, TurnStarted, TurnSubmitResult};

use super::tests::{Rig, SID, T0};
use crate::tests::Answer;

/// A message of the length that hung (159 characters and up).
fn long() -> String {
    "Start two tasks for me, one to count the words in every markdown file under sample and one to \
     time the slow load script, then tell me when both are done."
        .to_string()
}

fn ask(id: &str, session: &str, expires_ms: u64) -> ThreadAsk {
    let opt = |key: &str, label: &str| AskOption {
        key: key.into(),
        label: label.into(),
    };
    ThreadAsk {
        ask_id: id.into(),
        session_id: session.into(),
        place: None,
        question: "Is this about the security fix or the pr round?".into(),
        options: vec![
            opt("1", "pr round"),
            opt("2", "security fix"),
            opt("home", "home"),
        ],
        message: "Start two tasks for me, one to count the words".into(),
        expires_ms,
        others: Vec::new(),
    }
}

/// The chat typed `long()` and the router asked about it.
async fn asked_rig(expires_in_ms: u64) -> Rig {
    let mut r = Rig::new(100, 40).await;
    r.ready().await;
    r.paste(&long()).await;
    r.press(KeyCode::Enter).await;
    r.asked("turn.submit", 1).await;
    r.tell(Event::ThreadAsked(ask("ask_1", SID, T0 + expires_in_ms)));
    r
}

#[tokio::test(start_paused = true)]
async fn the_routers_question_is_shown_and_its_key_answers_it() {
    assert!(long().len() > 150, "the length the issue names");
    let mut r = asked_rig(900_000).await;
    r.shows("Is this about the security fix or the pr round?")
        .await;
    let text = r.text();
    assert!(text.contains("[2] security fix"), "{text}");
    assert!(text.contains("[h]ome"), "{text}");
    r.type_text("2").await;
    r.press(KeyCode::Enter).await;
    r.asked("thread.answer", 1).await;
    let sent = &r.daemon.asked("thread.answer")[0];
    assert_eq!(
        (&sent["ask_id"], &sent["choice"]),
        (&json!("ask_1"), &json!("2"))
    );
    // The key answered the question; it was not a message.
    assert_eq!(r.daemon.asked("turn.submit").len(), 1);
}

#[tokio::test(start_paused = true)]
async fn a_line_that_is_no_option_is_a_message_and_the_question_stays() {
    let mut r = asked_rig(900_000).await;
    r.shows("security fix").await;
    r.type_text("and what is the weather").await;
    r.press(KeyCode::Enter).await;
    r.asked("turn.submit", 2).await;
    assert!(r.daemon.asked("thread.answer").is_empty());
    // The question still takes its key.
    r.type_text("h").await;
    r.press(KeyCode::Enter).await;
    r.asked("thread.answer", 1).await;
    assert_eq!(r.daemon.asked("thread.answer")[0]["choice"], json!("home"));
}

#[tokio::test(start_paused = true)]
async fn an_expired_question_takes_no_keys_and_another_sessions_is_not_shown() {
    let mut r = asked_rig(60_000).await;
    r.shows("security fix").await;
    r.advance(61_000).await;
    r.type_text("2").await;
    r.press(KeyCode::Enter).await;
    r.asked("turn.submit", 2).await;
    assert!(r.daemon.asked("thread.answer").is_empty());
    let mut other = Rig::new(100, 40).await;
    other.ready().await;
    other.tell(Event::ThreadAsked(ask(
        "ask_9",
        "ses_someone_else",
        T0 + 900_000,
    )));
    other.advance(100).await;
    assert!(!other.text().contains("security fix"), "{}", other.text());
}

/// The daemon's script, `turn.submit` answered as a turn the router placed in
/// the thread `security-fix` (its own session).
fn placed_script() -> crate::tests::Script {
    let base = super::tests::script();
    Arc::new(move |m, p| -> Answer {
        if m != "turn.submit" {
            return base(m, p);
        }
        let placed = TurnSubmitResult {
            session_id: "ses_thread_security".into(),
            turn_id: "turn_t1".into(),
            output: "Looked at the vault.".into(),
            model: "claude-sonnet-5-5".into(),
            elapsed_ms: 900,
            stop_reason: "done".into(),
            thread: Some(ThreadTag {
                slug: Some("security-fix".into()),
                title: "security fix".into(),
                how: RouteHow::Quiet,
                confidence: 0.95,
                movable: false,
            }),
            ..Default::default()
        };
        Ok(serde_json::to_value(placed).unwrap())
    })
}

#[tokio::test(start_paused = true)]
async fn a_message_the_router_placed_in_a_thread_says_so() {
    let mut r = Rig::with(100, 40, placed_script()).await;
    r.ready().await;
    r.type_text("why is the vault build red").await;
    r.press(KeyCode::Enter).await;
    r.shows("Looked at the vault.").await;
    let text = r.text();
    assert!(text.contains("→ thread security fix"), "{text}");
    assert!(text.contains("ses_thread_security"), "{text}");
}

const PLACED: &str = "ses_thread_security";

fn placed_tag() -> ThreadTag {
    ThreadTag {
        slug: Some("security-fix".into()),
        title: "security fix".into(),
        how: RouteHow::Quiet,
        confidence: 0.95,
        movable: false,
    }
}

/// The daemon's notification that the router placed this chat's message.
fn placed(turn: &str) -> Event {
    Event::ThreadPlaced(ThreadPlaced {
        session_id: SID.into(),
        placed: PLACED.into(),
        turn_id: turn.into(),
        tag: placed_tag(),
    })
}

fn started(session: &str, turn: &str) -> Event {
    Event::TurnStarted(TurnStarted {
        session_id: session.into(),
        turn_id: turn.into(),
        ..Default::default()
    })
}

#[tokio::test(start_paused = true)]
async fn a_placed_turns_task_shows_in_the_chats_footer() {
    let mut r = Rig::with(100, 40, placed_script()).await;
    r.ready().await;
    r.tell(placed("turn_t1"));
    r.settle().await;
    // The chat watches the board again with the placed session among its
    // roots (a second watch replaces the first's), so its tasks reach it.
    let watches = r.daemon.asked("work.watch");
    assert_eq!(watches.len(), 2, "{watches:?}");
    assert_eq!(watches[1]["roots"], json!([SID, PLACED]));
    super::tests_tasks::push(
        &r,
        10,
        vec![super::tests_tasks::task(1, "Audit the vault", 10)],
    );
    r.advance(100).await;
    assert!(r.text().contains("Audit the vault"), "{}", r.text());
}

#[tokio::test(start_paused = true)]
async fn the_placement_is_told_when_it_happens() {
    let mut r = Rig::with(100, 40, placed_script()).await;
    r.ready().await;
    r.tell(placed("turn_t1"));
    r.advance(100).await;
    let said = |t: &str| t.matches("→ thread security fix").count();
    let text = r.text();
    assert_eq!(said(&text), 1, "{text}");
    assert!(text.contains("go there: theseus chat ses_thread_security"));
    r.type_text("why is the vault build red").await;
    r.press(KeyCode::Enter).await;
    r.shows("Looked at the vault.").await;
    let text = r.text();
    assert_eq!(said(&text), 1, "the result tells it no second time: {text}");
    assert_eq!(text.matches("go there:").count(), 1, "{text}");
}

#[tokio::test(start_paused = true)]
async fn another_turn_of_the_thread_is_not_the_chats() {
    let mut r = Rig::with(100, 40, placed_script()).await;
    r.ready().await;
    r.tell(placed("turn_t1"));
    // A turn sent to the thread's session from elsewhere starts no live line.
    r.tell(started(PLACED, "turn_other"));
    r.advance(300).await;
    assert!(!r.text().contains("thinking"), "{}", r.text());
    // The placed turn's own start does.
    r.tell(started(PLACED, "turn_t1"));
    r.advance(300).await;
    assert!(r.text().contains("thinking"), "{}", r.text());
}

fn asking(turn: Option<&str>) -> Event {
    Event::ConfirmRequested(theseus_protocol::ConfirmRequest {
        correlation_id: format!("act_{}", turn.unwrap_or("none")),
        session_id: PLACED.into(),
        turn_id: turn.map(str::to_string),
        tool: "proc.run".into(),
        reason: "run the vault tests".into(),
        input: json!({"argv": ["cargo", "test"]}),
        ..Default::default()
    })
}

/// A question or a task of another turn of the placed thread's session (one
/// sent from Discord, say) is not shown here; the chat's own placed turn's
/// is (theseus-dos5f).
#[tokio::test(start_paused = true)]
async fn a_placed_threads_other_turn_does_not_ask_in_this_chat() {
    let mut r = Rig::with(100, 40, placed_script()).await;
    r.ready().await;
    r.tell(placed("turn_t1"));
    r.tell(asking(Some("turn_other")));
    r.advance(300).await;
    assert!(!r.text().contains("y yes"), "{}", r.text());
    r.tell(asking(Some("turn_t1")));
    r.shows("y yes").await;
}

/// A question that names no turn (an older daemon's, a background worker's)
/// still asks in this chat (theseus-fkvb0).
#[tokio::test(start_paused = true)]
async fn a_question_naming_no_turn_asks_in_this_chat() {
    let mut r = Rig::with(100, 40, placed_script()).await;
    r.ready().await;
    r.tell(placed("turn_t1"));
    r.tell(asking(None));
    r.shows("y yes").await;
}

/// A task change of another turn of the placed thread's session is not
/// counted in this chat; one with no turn (a background worker's) is.
#[tokio::test(start_paused = true)]
async fn a_placed_threads_other_turns_task_is_not_shown_but_a_workers_is() {
    use theseus_protocol::tasks::{TaskChanged, TaskRecord, TaskState};
    let task = |turn: Option<&str>| {
        Event::TaskChanged(TaskChanged {
            session_id: PLACED.into(),
            verb: "update".into(),
            turn_id: turn.map(str::to_string),
            task: TaskRecord {
                id: "tsk_1".into(),
                state: TaskState::InProgress,
                ..Default::default()
            },
        })
    };
    let mut r = Rig::with(100, 40, placed_script()).await;
    r.ready().await;
    r.tell(placed("turn_t1"));
    r.tell(task(Some("turn_other")));
    r.advance(300).await;
    assert!(!r.text().contains("1 task"), "{}", r.text());
    r.tell(task(None));
    r.until("the worker's task", |r| r.text().contains("◐ 1 task"))
        .await;
}

/// After a reconnect the work board is watched again with the placed
/// thread's session among its roots (theseus-axgln).
#[tokio::test(start_paused = true)]
async fn a_reconnect_keeps_following_the_placed_thread() {
    let mut r = Rig::with(100, 40, placed_script()).await;
    r.ready().await;
    r.tell(placed("turn_t1"));
    r.settle().await;
    r.daemon.close();
    r.until("the status line to say offline", |r| {
        r.text().contains("offline")
    })
    .await;
    r.advance(1_000).await;
    r.until("the dock to leave offline", |r| {
        !r.text().contains("offline")
    })
    .await;
    let later = r.later.lock().unwrap();
    let watches = later[0].asked("work.watch");
    assert_eq!(
        watches.last().unwrap()["roots"],
        json!([SID, PLACED]),
        "{watches:?}"
    );
}

/// A turn that ran no model: held in a shared place.
#[tokio::test(start_paused = true)]
async fn a_held_turn_names_no_model_in_the_chat() {
    let base = super::tests::script();
    let sent = Arc::new(std::sync::atomic::AtomicU32::new(0));
    let script: crate::tests::Script = Arc::new(move |m, p| -> Answer {
        if m != "turn.submit" {
            return base(m, p);
        }
        let n = sent.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        let (stop, model) = match n {
            0 => ("held", "claude-opus-5-5"),
            _ => ("end_turn", "claude-sonnet-5-5"),
        };
        let r = TurnSubmitResult {
            session_id: SID.into(),
            turn_id: format!("turn_h{n}"),
            output: "I don't share that here.".into(),
            model: model.into(),
            elapsed_ms: 900,
            stop_reason: stop.into(),
            ..Default::default()
        };
        Ok(serde_json::to_value(r).unwrap())
    });
    let mut r = Rig::with(100, 40, script).await;
    r.ready().await;
    r.type_text("what is the owner's address").await;
    r.press(KeyCode::Enter).await;
    r.shows("held: no model ran").await;
    let text = r.text();
    assert!(!text.contains("Opus"), "{text}");
    assert!(
        text.contains("Sonnet 5.5"),
        "the status line keeps the last model: {text}"
    );
    // Every other turn's line is as it was.
    r.type_text("and the weather").await;
    r.press(KeyCode::Enter).await;
    r.shows("900 ms · Sonnet 5.5").await;
}

fn continuation(session: &str, turn: &str) -> Event {
    Event::TurnStarted(TurnStarted {
        session_id: session.into(),
        turn_id: turn.into(),
        continuation: true,
        ..Default::default()
    })
}

fn ended(session: &str, turn: &str, output: &str) -> Event {
    Event::TurnEnded(TurnSubmitResult {
        session_id: session.into(),
        turn_id: turn.into(),
        output: output.into(),
        model: "claude-sonnet-5-5".into(),
        stop_reason: "done".into(),
        ..Default::default()
    })
}

fn resolved(corr: &str) -> Event {
    Event::ConfirmResolved(theseus_protocol::ConfirmResolved {
        correlation_id: corr.into(),
        session_id: PLACED.into(),
        approved: true,
        option: Some("approve".into()),
        ..Default::default()
    })
}

/// The walkthrough: a message placed in a thread on a guess, the guard's
/// question, the answer, then the thread session's continuation. The chat
/// watches that session (the answer and the continuation are published to
/// its watchers alone), so the card resolves and the reply prints under it
/// (theseus-nde5n).
#[tokio::test(start_paused = true)]
async fn a_placed_threads_answered_question_resolves_and_its_reply_prints() {
    let mut r = Rig::with(100, 40, placed_script()).await;
    r.ready().await;
    r.tell(placed("turn_t1"));
    r.tell(asking(Some("turn_t1")));
    r.shows("y yes").await;
    r.settle().await;
    let watched: Vec<_> = r
        .daemon
        .asked("session.watch")
        .iter()
        .map(|p| p["session_id"].clone())
        .collect();
    assert!(watched.contains(&json!(PLACED)), "{watched:?}");
    r.press(KeyCode::Char('y')).await;
    r.asked("action.confirm", 1).await;
    assert!(r.text().contains("answering…"), "{}", r.text());
    r.tell(resolved("act_turn_t1"));
    r.tell(continuation(PLACED, "turn_c1"));
    r.tell(ended(PLACED, "turn_c1", "Wrote the harbour summary."));
    r.shows("Wrote the harbour summary.").await;
    let text = r.text();
    assert!(text.contains("approved"), "{text}");
    assert!(!text.contains("answering…"), "{text}");
}

/// Only the continuation of this chat's own question is the chat's: a turn
/// another surface sent to the thread, and a continuation nobody here asked
/// for, stay out.
#[tokio::test(start_paused = true)]
async fn another_turn_of_the_placed_session_still_does_not_print() {
    let mut r = Rig::with(100, 40, placed_script()).await;
    r.ready().await;
    r.tell(placed("turn_t1"));
    r.tell(asking(Some("turn_t1")));
    r.shows("y yes").await;
    r.tell(resolved("act_turn_t1"));
    r.tell(started(PLACED, "turn_other"));
    r.tell(ended(PLACED, "turn_other", "From the other surface."));
    r.advance(300).await;
    assert!(
        !r.text().contains("From the other surface."),
        "{}",
        r.text()
    );
    // The continuation after the answer is the chat's, and only once.
    r.tell(continuation(PLACED, "turn_c1"));
    r.tell(ended(PLACED, "turn_c1", "Done here."));
    r.shows("Done here.").await;
    r.tell(continuation(PLACED, "turn_c2"));
    r.tell(ended(PLACED, "turn_c2", "Not asked here."));
    r.advance(300).await;
    assert!(!r.text().contains("Not asked here."), "{}", r.text());
}

/// After a reconnect the placed session is watched again.
#[tokio::test(start_paused = true)]
async fn a_reconnect_watches_the_placed_session_again() {
    let mut r = Rig::with(100, 40, placed_script()).await;
    r.ready().await;
    r.tell(placed("turn_t1"));
    r.settle().await;
    r.daemon.close();
    r.until("the status line to say offline", |r| {
        r.text().contains("offline")
    })
    .await;
    r.advance(1_000).await;
    r.until("the dock to leave offline", |r| {
        !r.text().contains("offline")
    })
    .await;
    let later = r.later.lock().unwrap();
    let ids: Vec<_> = later[0]
        .asked("session.watch")
        .iter()
        .map(|p| p["session_id"].clone())
        .collect();
    assert!(ids.contains(&json!(PLACED)), "{ids:?}");
}
