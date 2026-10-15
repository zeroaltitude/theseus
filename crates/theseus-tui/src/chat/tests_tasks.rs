//! The chat's tasks (theseus-mnd9g) over a scripted daemon: the work board's
//! snapshot and pushes become a footer above the input line and a titled
//! line for each event; a task's question is answered with one key through
//! `work.answer` as `theseus chat`; and nothing repaints on a timer.

use std::sync::Arc;

use crossterm::event::KeyCode;
use serde_json::{json, Value};
use theseus_protocol::work::WorkView;
use theseus_protocol::{Event, WorkChanged};

use super::tests::{script, Rig, SID, T0};
use crate::tests::Script;

fn view(v: Value) -> WorkView {
    let mut base = json!({
        "id": "exe_1", "kind": "task", "position": 9, "at_ms": T0, "root": "exe_conv",
        "parent": "exe_conv", "title": "", "state": "running",
        "attention": {"level": "working", "label": "turn 1", "since_ms": T0 - 20_000},
        "started_at_ms": T0 - 20_000, "state_since_ms": T0 - 20_000,
        "doing": {"what": "thinking", "label": "turn 1", "since_ms": T0 - 20_000, "turn": 1, "loop_index": 0}
    });
    for (k, x) in v.as_object().unwrap() {
        base[k] = x.clone();
    }
    serde_json::from_value(base).unwrap()
}

fn conversation() -> WorkView {
    view(json!({
        "id": "exe_conv", "kind": "conversation", "session_id": SID, "parent": null,
        "root": "exe_conv", "title": "Regatta prep", "state": "waiting", "position": 1,
        "attention": {"level": "working", "label": "waiting on tasks", "since_ms": T0 - 90_000},
        "doing": null
    }))
}

fn task(n: usize, title: &str, position: u64) -> WorkView {
    view(json!({
        "id": format!("exe_t{n}"), "session_id": format!("ses_task{n}abc{n}f"),
        "title": title, "position": position
    }))
}

fn options(ask: bool) -> Value {
    if ask {
        json!([
            {"id": "choice:0", "label": "Red", "style": "plain", "takes_note": false, "effect": "x"},
            {"id": "choice:1", "label": "Blue", "style": "plain", "takes_note": false, "effect": "x"},
            {"id": "choice:other", "label": "Something else", "style": "plain", "takes_note": true, "effect": "x"}
        ])
    } else {
        json!([
            {"id": "approve", "label": "Approve", "style": "primary", "takes_note": false, "effect": "the call runs"},
            {"id": "decline", "label": "Decline", "style": "plain", "takes_note": true, "effect": "the call does not run"}
        ])
    }
}

/// Paint log asks to write a file (or, with `ask`, which hull colour).
fn asking(position: u64, ask: bool) -> WorkView {
    let (kind, prompt) = if ask {
        ("ask", "which hull colour?")
    } else {
        ("approval", "write regatta/paint-log.md (37 B)")
    };
    view(json!({
        "id": "exe_paint", "session_id": "ses_paint2a329e", "title": "Paint log",
        "state": "needs_you", "position": position,
        "attention": {"level": "needs_you", "label": "confirm fs.write", "since_ms": T0},
        "question": {
            "id": "act_x2a329e", "kind": kind, "tool": "fs.write", "prompt": prompt,
            "asked_by": {"id": "exe_paint", "kind": "task", "title": "Paint log"},
            "options": options(ask), "choices": if ask { json!(["Red", "Blue"]) } else { json!([]) },
            "floor": false, "may_answer": true, "asked_at_ms": T0,
            "input": {"path": "regatta/paint-log.md", "content": "the hull is red"}
        }
    }))
}

fn board(views: &[WorkView]) -> Value {
    json!({"position": 5, "views": views, "total": 1})
}

fn script_with(snapshot: Value) -> Script {
    let base = script();
    Arc::new(move |m, p| {
        match m {
            "work.watch" => Ok(snapshot.clone()),
            // The conversation's execution, which Esc and `/stop` stop.
            "session.open" => base(m, p).map(|mut v| {
                v["execution_id"] = json!("exe_conv");
                v
            }),
            _ => base(m, p),
        }
    })
}

fn push(r: &Rig, position: u64, views: Vec<WorkView>) {
    r.tell(Event::WorkChanged(WorkChanged {
        position,
        at_ms: T0,
        views,
        gone: vec![],
        threads: vec![],
    }));
}

async fn ready(w: u16, h: u16, snapshot: &[WorkView]) -> Rig {
    let mut r = Rig::with(w, h, script_with(board(snapshot))).await;
    r.ready().await;
    r.asked("work.watch", 1).await;
    r.settle().await;
    r
}

// ------------------------------------------------------------ the watch

#[tokio::test(start_paused = true)]
async fn the_chat_watches_its_own_tree_as_a_screen_a_person_looks_at() {
    let r = ready(80, 24, &[conversation()]).await;
    let w = &r.daemon.asked("work.watch")[0];
    assert_eq!(w["roots"], json!([SID]));
    assert_eq!(w["interactive"], json!(true));
}

#[tokio::test(start_paused = true)]
async fn five_tasks_print_five_titled_started_lines_and_a_footer() {
    let mut r = ready(80, 30, &[conversation()]).await;
    let names = [
        "Paint log",
        "Buoy survey",
        "Tide tables",
        "Rigging check",
        "Weather brief",
    ];
    for (i, n) in names.iter().enumerate() {
        push(&r, 10 + i as u64, vec![task(i, n, 10 + i as u64)]);
    }
    r.shows("◐ tasks · 5 working").await;
    let text = r.text();
    for n in names {
        assert!(
            text.contains(&format!("◐ started: {n} (")),
            "a started line for {n}:\n{text}"
        );
    }
    // The footer: the summary and a row each, the status line counts them.
    assert_eq!(text.matches("model <1m").count(), 5, "{text}");
    assert!(text.contains("◐ 5 tasks"), "{text}");
    // A second push of the same views says and draws nothing new.
    let mark = r.bytes();
    push(&r, 10, vec![task(0, "Paint log", 10)]);
    r.settle().await;
    assert_eq!(text.matches("started: Paint log").count(), 1);
    assert!(!r.written_since(mark).contains("started"), "said twice");
}

#[tokio::test(start_paused = true)]
async fn the_footer_shows_five_tasks_then_how_many_more() {
    let mut r = ready(80, 40, &[conversation()]).await;
    for i in 0..8 {
        push(
            &r,
            10 + i,
            vec![task(i as usize, &format!("Leg {i}"), 10 + i)],
        );
    }
    r.shows("+3 more · ctrl-t").await;
    let rows = r
        .screen()
        .iter()
        .filter(|l| l.contains("model <1m"))
        .count();
    assert_eq!(rows, 5, "{}", r.text());
}

#[tokio::test(start_paused = true)]
async fn a_snapshot_with_running_tasks_prints_no_started_lines() {
    let mut r = ready(80, 24, &[conversation(), task(1, "Buoy survey", 3)]).await;
    r.shows("◐ tasks · 1 working").await;
    assert!(!r.text().contains("started:"), "{}", r.text());
}

// ------------------------------------------------------------ one-key answers

#[tokio::test(start_paused = true)]
async fn a_task_asks_by_name_and_y_answers_it_as_theseus_chat() {
    let mut r = ready(100, 30, &[conversation()]).await;
    push(&r, 10, vec![asking(10, false)]);
    r.shows("[y] approve · [n] decline · [v] view").await;
    let text = r.text();
    assert!(
        text.contains("Paint log asks: write regatta/paint-log.md (37 B)?"),
        "{text}"
    );
    assert!(
        text.contains("● 2a329e  Paint log") || text.contains("2a329e"),
        "{text}"
    );
    r.press(KeyCode::Char('y')).await;
    r.asked("work.answer", 1).await;
    let a = &r.daemon.asked("work.answer")[0];
    assert_eq!(
        (
            a["question_id"].as_str(),
            a["option"].as_str(),
            a["author"].as_str()
        ),
        (Some("act_x2a329e"), Some("approve"), Some("theseus chat"))
    );
    r.shows("Paint log: approved (approve)").await;
}

#[tokio::test(start_paused = true)]
async fn a_digit_answers_an_ask_question_with_its_choice() {
    let mut r = ready(100, 30, &[conversation()]).await;
    push(&r, 10, vec![asking(10, true)]);
    r.shows("[1] red · [2] blue").await;
    r.press(KeyCode::Char('2')).await;
    r.asked("work.answer", 1).await;
    let a = &r.daemon.asked("work.answer")[0];
    assert_eq!(a["option"], json!("choice:1"));
    assert_eq!(a["author"], json!("theseus chat"));
    // `y` is no answer to a question with choices.
    let mut r2 = ready(100, 30, &[conversation()]).await;
    push(&r2, 10, vec![asking(10, true)]);
    r2.shows("[1] red").await;
    r2.press(KeyCode::Char('y')).await;
    assert!(r2.daemon.asked("work.answer").is_empty());
}

#[tokio::test(start_paused = true)]
async fn n_takes_a_note_and_esc_goes_back_to_the_question() {
    let mut r = ready(100, 30, &[conversation()]).await;
    push(&r, 10, vec![asking(10, false)]);
    r.shows("[n] decline").await;
    r.press(KeyCode::Char('n')).await;
    r.shows("no · say what to do instead").await;
    // Nothing is sent until the note is.
    assert!(r.daemon.asked("work.answer").is_empty());
    r.type_text("use the old log").await;
    r.press(KeyCode::Enter).await;
    r.asked("work.answer", 1).await;
    let a = &r.daemon.asked("work.answer")[0];
    assert_eq!(a["option"], json!("decline"));
    assert_eq!(a["note"], json!("use the old log"));
    // Enter with no note declines without one.
    let mut r = ready(100, 30, &[conversation()]).await;
    push(&r, 10, vec![asking(10, false)]);
    r.shows("[n] decline").await;
    r.press(KeyCode::Char('n')).await;
    r.press(KeyCode::Enter).await;
    r.asked("work.answer", 1).await;
    let a = &r.daemon.asked("work.answer")[0];
    assert_eq!(a["option"], json!("decline"));
    assert!(a.get("note").is_none(), "{a}");
    // Esc is chat-ask's: back to the question, nothing sent.
    let mut r = ready(100, 30, &[conversation()]).await;
    push(&r, 10, vec![asking(10, false)]);
    r.shows("[n] decline").await;
    r.press(KeyCode::Char('n')).await;
    r.press(KeyCode::Esc).await;
    r.shows("[y] approve").await;
    assert!(r.daemon.asked("work.answer").is_empty());
}

#[tokio::test(start_paused = true)]
async fn a_message_that_starts_with_y_never_meets_a_one_key_answer() {
    // With the line holding text, `y` is text.
    let mut r = ready(100, 30, &[conversation()]).await;
    push(&r, 10, vec![asking(10, false)]);
    r.shows("[y] approve").await;
    r.type_text("so ").await;
    r.type_text("yes").await;
    assert!(r.daemon.asked("work.answer").is_empty());
    assert!(r.text().contains("› so yes"), "{}", r.text());
    // With esc, the line is for typing: the first `y` is text too, and the
    // question stays in the dock, one row high.
    let mut r = ready(100, 30, &[conversation()]).await;
    push(&r, 10, vec![asking(10, false)]);
    r.shows("[y] approve").await;
    r.press(KeyCode::Esc).await;
    r.shows("tab: answer it").await;
    r.type_text("yes, but call it log2").await;
    r.press(KeyCode::Enter).await;
    r.asked("turn.submit", 1).await;
    assert_eq!(
        r.daemon.asked("turn.submit")[0]["input"],
        json!("yes, but call it log2")
    );
    assert!(
        r.daemon.asked("work.answer").is_empty(),
        "a message answered"
    );
    // Tab gives the question its keys again.
    r.press(KeyCode::Tab).await;
    r.shows("[y] approve").await;
}

#[tokio::test(start_paused = true)]
async fn inside_a_job_a_key_answer_is_refused_and_sends_nothing() {
    let mut r = ready(100, 30, &[conversation()]).await;
    r.runner.chat.job_session = Some("ses_job0123456789".into());
    push(&r, 10, vec![asking(10, false)]);
    r.shows("[y] approve").await;
    r.press(KeyCode::Char('y')).await;
    r.shows("theseus answer refused").await;
    r.settle().await;
    assert!(r.daemon.asked("work.answer").is_empty());
}

#[tokio::test(start_paused = true)]
async fn tab_moves_to_the_next_question_and_v_shows_the_call() {
    let mut r = ready(100, 36, &[conversation()]).await;
    let second = view(json!({
        "id": "exe_buoy", "session_id": "ses_buoy5a877c", "title": "Buoy survey",
        "state": "needs_you", "position": 11,
        "attention": {"level": "needs_you", "label": "confirm", "since_ms": T0},
        "question": {"id": "act_y5a877c", "kind": "approval", "tool": "proc.run",
            "prompt": "run cargo build", "asked_by": {"id": "exe_buoy", "kind": "task", "title": "Buoy survey"},
            "options": options(false), "floor": false, "may_answer": true, "asked_at_ms": T0 + 1000}
    }));
    push(&r, 10, vec![asking(10, false)]);
    push(&r, 11, vec![second]);
    r.shows("tab next (1 more)").await;
    assert!(r.text().contains("? Paint log asks:"), "{}", r.text());
    r.press(KeyCode::Tab).await;
    r.shows("? Buoy survey asks: run cargo build?").await;
    r.press(KeyCode::Char('y')).await;
    r.asked("work.answer", 1).await;
    assert_eq!(
        r.daemon.asked("work.answer")[0]["question_id"],
        json!("act_y5a877c")
    );
    // The call in full.
    r.press(KeyCode::Tab).await;
    r.press(KeyCode::Char('v')).await;
    r.shows("\"content\": \"the hull is red\"").await;
}

// ------------------------------------------------------------ commands

#[tokio::test(start_paused = true)]
async fn tasks_and_ctrl_t_draw_the_tree_in_place() {
    let mut r = ready(100, 30, &[conversation(), task(1, "Buoy survey", 3)]).await;
    r.type_text("/tasks").await;
    r.press(KeyCode::Enter).await;
    r.shows("1 working").await;
    assert!(r.text().contains("Buoy survey"), "{}", r.text());
    let rows = r
        .screen()
        .iter()
        .filter(|l| l.contains("Regatta prep"))
        .count();
    assert_eq!(rows, 1);
    // Ctrl-T the same, again below the first.
    r.ctrl('t').await;
    let n = r.text().matches("1 working").count();
    assert!(n >= 2, "{}", r.text());
}

#[tokio::test(start_paused = true)]
async fn cancel_and_stop_are_one_call_each() {
    let mut r = ready(100, 30, &[conversation(), task(1, "Buoy survey", 3)]).await;
    r.type_text("/cancel abc1f").await;
    r.press(KeyCode::Enter).await;
    r.asked("task.cancel", 1).await;
    assert_eq!(r.daemon.asked("task.cancel")[0]["task"], json!("abc1f"));
    r.type_text("/cancel ab").await;
    r.press(KeyCode::Enter).await;
    r.shows("at least four characters").await;
    r.type_text("/stop --tasks").await;
    r.press(KeyCode::Enter).await;
    r.asked("execution.stop", 1).await;
    let s = &r.daemon.asked("execution.stop")[0];
    // One command: Esc's stop (jobs that went to the background run on), and
    // with `--tasks` the conversation's tasks as well.
    assert_eq!(
        (
            s["execution_id"].as_str(),
            s["tasks"].as_bool(),
            s["keep_jobs"].as_bool()
        ),
        (Some("exe_conv"), Some(true), Some(true))
    );
}

// ------------------------------------------------------------ notices

#[tokio::test(start_paused = true)]
async fn only_an_interrupt_rings_the_bell() {
    let mut r = ready(100, 30, &[conversation()]).await;
    let notice = |urgency: &str, why: &str, position: u64| {
        json!({
            "urgency": urgency, "why": why,
            "work": {"id": "exe_paint", "kind": "task", "session_id": "ses_paint2a329e", "title": "Paint log"},
            "root": {"id": "exe_conv", "kind": "conversation", "session_id": SID, "title": ""},
            "line": "x", "position": position, "at_ms": T0
        })
    };
    let mark = r.bytes();
    r.daemon
        .notify("work.notice", notice("inform", "reported", 20));
    r.settle().await;
    assert!(!r.written_since(mark).contains('\x07'), "an inform rang");
    r.daemon
        .notify("work.notice", notice("interrupt", "asked", 21));
    r.settle().await;
    assert_eq!(r.written_since(mark).matches('\x07').count(), 1);
    // The conversation's own, on screen, rings nothing.
    let mut own = notice("interrupt", "asked", 22);
    own["work"] = json!({"id": "exe_conv", "kind": "conversation", "session_id": SID, "title": ""});
    let mark = r.bytes();
    r.daemon.notify("work.notice", own);
    r.settle().await;
    assert!(!r.written_since(mark).contains('\x07'));
}

// ------------------------------------------------------------ quiet by construction

#[tokio::test(start_paused = true)]
async fn the_footer_repaints_on_the_minute_and_on_no_timer() {
    let mut r = ready(100, 30, &[conversation()]).await;
    // Started 20 s before T0: its clock turns over 40 s from now.
    push(&r, 10, vec![task(1, "Buoy survey", 10)]);
    r.shows("◐ tasks · 1 working").await;
    r.settle().await;
    let frames = r.runner.frames;
    r.advance(30_000).await;
    assert_eq!(r.runner.frames, frames, "the footer repainted on a timer");
    r.advance(15_000).await;
    assert_eq!(r.runner.frames, frames + 1, "one repaint on the minute");
    assert!(r.text().contains("1m"), "{}", r.text());
    // And it goes quiet again until the next.
    let frames = r.runner.frames;
    r.advance(30_000).await;
    assert_eq!(r.runner.frames, frames);
}

#[tokio::test(start_paused = true)]
async fn nothing_waits_on_the_clock_when_no_task_works() {
    let mut r = ready(100, 30, &[conversation()]).await;
    let done = view(json!({
        "id": "exe_t1", "session_id": "ses_task1abc1f", "title": "Buoy survey", "position": 10,
        "state": "done", "attention": {"level": "idle", "label": "complete", "since_ms": T0},
        "report": {"outcome": "done", "first_line": "Charted."}
    }));
    push(&r, 10, vec![done]);
    r.shows("✓ Buoy survey reported: Charted.").await;
    r.settle().await;
    assert_eq!(
        r.runner.chat.deadline(),
        None,
        "a deadline with nothing running"
    );
    assert!(!r.text().contains("◐ tasks"), "{}", r.text());
}

// ------------------------------------------------- one question model

fn own_question() -> theseus_protocol::ConfirmRequest {
    theseus_protocol::ConfirmRequest {
        correlation_id: "act_own1".into(),
        session_id: SID.into(),
        tool: "proc.run".into(),
        input: json!({"argv": ["make", "tides"]}),
        reason: "build the tide tables".into(),
        by: "the model".into(),
        requested_at_ms: T0,
        expires_at_ms: T0 + 300_000,
        ..Default::default()
    }
}

fn resolved(id: &str) -> Event {
    Event::ConfirmResolved(theseus_protocol::ConfirmResolved {
        correlation_id: id.into(),
        session_id: SID.into(),
        option: Some("approve".into()),
        ..Default::default()
    })
}

/// The conversation's own question and a task's are one question in the
/// dock, not two: the own one first, answered by `action.confirm`; the
/// task's behind it, drawn and answered when its turn comes, by
/// `work.answer`. The keys are `keys::ASK_KEYS`, once.
#[tokio::test(start_paused = true)]
async fn the_conversations_question_and_a_tasks_share_one_dock() {
    let mut r = ready(100, 36, &[conversation()]).await;
    push(&r, 10, vec![asking(10, false)]);
    r.shows("Paint log asks").await;
    r.tell(Event::ConfirmRequested(own_question()));
    r.shows("Theseus asks").await;
    let text = r.text();
    // One question has the keys: the own one, with the other behind it.
    assert_eq!(
        r.screen()
            .iter()
            .filter(|l| l.starts_with("? ") && !l.starts_with("? help"))
            .count(),
        1,
        "{text}"
    );
    assert!(text.contains("tab next (1 more)"), "{text}");
    r.press(KeyCode::Char('y')).await;
    r.asked("action.confirm", 1).await;
    assert_eq!(
        r.daemon.asked("action.confirm")[0]["correlation_id"],
        json!("act_own1")
    );
    assert!(r.daemon.asked("work.answer").is_empty());
    // A second key while the answer is in flight is no second answer.
    r.press(KeyCode::Char('y')).await;
    r.settle().await;
    assert_eq!(r.daemon.asked("action.confirm").len(), 1);
    assert!(r.daemon.asked("work.answer").is_empty());
    // The own question resolved: the task's has the keys.
    r.tell(resolved("act_own1"));
    r.shows("? Paint log asks").await;
    r.press(KeyCode::Char('y')).await;
    r.asked("work.answer", 1).await;
    assert_eq!(
        r.daemon.asked("work.answer")[0]["question_id"],
        json!("act_x2a329e")
    );
    assert_eq!(r.daemon.asked("action.confirm").len(), 1);
}

/// A digit is an `ask` question's choice; on any other question it is text,
/// so a message that starts with a number is a message.
#[tokio::test(start_paused = true)]
async fn a_digit_is_text_on_a_question_without_choices() {
    let mut r = ready(100, 30, &[conversation()]).await;
    r.tell(Event::ConfirmRequested(own_question()));
    r.shows("y yes").await;
    r.press(KeyCode::Char('2')).await;
    r.settle().await;
    assert!(r.daemon.asked("action.confirm").is_empty());
    assert!(r.text().contains("› 2"), "{}", r.text());
    // The same for a task's approval.
    let mut r = ready(100, 30, &[conversation()]).await;
    push(&r, 10, vec![asking(10, false)]);
    r.shows("[y] approve").await;
    r.press(KeyCode::Char('1')).await;
    r.settle().await;
    assert!(r.daemon.asked("work.answer").is_empty());
    assert!(r.text().contains("› 1"), "{}", r.text());
}

/// Tab goes round the questions that wait, a task's after the
/// conversation's own, and back to the first.
#[tokio::test(start_paused = true)]
async fn tab_goes_round_the_questions() {
    let mut r = ready(100, 36, &[conversation()]).await;
    push(&r, 10, vec![asking(10, false)]);
    r.tell(Event::ConfirmRequested(own_question()));
    r.shows("tab next (1 more)").await;
    r.press(KeyCode::Tab).await;
    r.shows("? Paint log asks").await;
    r.press(KeyCode::Tab).await;
    r.shows("Theseus asks").await;
}

/// A question whose words wrap in the transcript is told once, by its
/// event's line; the snapshot's announcement skips what an event said (the
/// live check found it twice when the prompt was long).
#[tokio::test(start_paused = true)]
async fn a_long_question_is_told_once() {
    let mut r = ready(60, 40, &[conversation()]).await;
    let mut v = asking(10, false);
    v.question.as_mut().unwrap().prompt =
        "write regatta/paint-log.md under the committee boat's long project directory".into();
    push(&r, 10, vec![v]);
    r.shows("[y] approve").await;
    let told = r
        .screen()
        .iter()
        .filter(|l| l.starts_with("● Paint log asks"))
        .count();
    assert_eq!(told, 1, "{}", r.text());
}
