//! The chat's questions, its stop and its queue (theseus-wy6v.1), over the
//! scripted daemon of `tests.rs`: a question shows its hunk numbered and
//! marked, one key answers it, a message that starts with "y" is a message,
//! Esc stops a turn and says what ended, a message typed meanwhile waits in
//! the dock, and an edit's diff stays in the transcript.

use std::sync::Arc;

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use serde_json::{json, Value};
use theseus_protocol::{
    CancelVerdict, ConfirmPreview, ConfirmRequest, Event, ExecutionInfo, ExecutionStopResult,
    ToolDiff, ToolEnded, ToolProposed, ToolStarted,
};

use super::tests::{Rig, SID, T0};
use crate::tests::{Answer, Script};

const EXE: &str = "exe_0123456789ab";

const UNIFIED: &str = "--- tides.py\n+++ tides.py\n@@ -48,3 +48,3 @@\n     highest = max(heights)\n-    return round(lowest - highest, 2)\n+    return round(highest - lowest, 2)\n \n";

fn execution() -> ExecutionInfo {
    serde_json::from_value(json!({
        "execution_id": EXE, "session_id": SID, "kind": "conversation", "state": "running",
        "turns": 1, "interrupted": 0, "outstanding": 0, "queued_results": 0,
        "budget": {"limit_usd": 1.0, "spent_usd": 0.0, "reserved_usd": 0.0,
                   "held_unknown_usd": 0.0, "available_usd": 1.0},
        "created_at_ms": T0, "updated_at_ms": T0,
    }))
    .unwrap()
}

fn stop_result(killed: Option<u32>) -> Value {
    let verdicts = killed
        .map(|n| CancelVerdict {
            correlation_id: "act_1".into(),
            tool: "proc.run".into(),
            state: "termination_verified".into(),
            verified_by: "pidns".into(),
            killed: Some(n),
            survivors: Some(0),
            ..Default::default()
        })
        .into_iter()
        .collect();
    serde_json::to_value(ExecutionStopResult {
        execution: execution(),
        stopped: true,
        stopped_actions: vec![],
        verdicts,
        declined: vec![],
        turn_running: true,
        tasks_running: 0,
        wakes_pending: 0,
        cancelled: vec![],
    })
    .unwrap()
}

/// The quiet daemon of `tests.rs`, whose session has an execution and whose
/// stop answers with two processes ended.
fn script() -> Script {
    Arc::new(|m, _p| -> Answer {
        Ok(match m {
            "health" => json!({"name": "theseusd", "model": "claude-sonnet-5-5", "profile": "f10"}),
            "session.open" => json!({
                "session_id": SID, "kind": "conversation", "label": null,
                "created_at_unix_ms": T0, "turns": 0, "execution_id": EXE,
            }),
            "session.list" => json!({"sessions": []}),
            "session.watch" => json!({"watching": SID, "watchers": 1}),
            "execution.stop" => stop_result(Some(2)),
            _ => json!({}),
        })
    })
}

fn edit_question(unified: &str) -> ConfirmRequest {
    ConfirmRequest {
        correlation_id: "act_88".into(),
        session_id: SID.into(),
        tool: "fs.edit".into(),
        input: json!({
            "path": "/home/ada/work/harbour-tides/tides.py",
            "old_string": "return round(lowest - highest, 2)",
            "new_string": "return round(highest - lowest, 2)",
        }),
        reason: "write under /home/ada/work/harbour-tides".into(),
        by: "the model".into(),
        requested_at_ms: T0,
        expires_at_ms: T0 + 300_000,
        preview: Some(ConfirmPreview {
            kind: ConfirmPreview::DIFF.into(),
            body: unified.into(),
        }),
        ..Default::default()
    }
}

async fn asked(r: &mut Rig, q: ConfirmRequest) {
    r.starts_turn("trn_1");
    r.tell(Event::ConfirmRequested(q));
    r.shows("y yes").await;
}

fn lines_of(r: &Rig) -> Vec<String> {
    r.screen()
}

#[tokio::test(start_paused = true)]
async fn a_question_shows_the_hunk_numbered_with_who_asks_and_when_it_expires() {
    for width in [80, 120] {
        let mut r = Rig::with(width, 30, script()).await;
        r.ready().await;
        asked(&mut r, edit_question(UNIFIED)).await;
        let text = r.text();
        for want in [
            "? Edit tides.py · replace 1 line",
            "tides.py:49  (+1 −1)",
            "@@ -48,3 +48,3 @@",
            "48       highest = max(heights)",
            "49 -     return round(lowest - highest, 2)",
            "49 +     return round(highest - lowest, 2)",
            "Theseus asks · expires in 5:00",
            "y yes · n no, say what instead · v whole call · esc type",
        ] {
            assert!(
                text.contains(want),
                "{width} columns: {want:?} not in\n{text}"
            );
        }
        assert!(
            lines_of(&r)
                .iter()
                .all(|l| l.chars().count() <= usize::from(width)),
            "{text}"
        );
        // The question stands in the input's place.
        assert!(!r.screen().iter().any(|l| l == "›"), "{text}");
    }
}

#[tokio::test(start_paused = true)]
async fn the_changed_words_are_marked_in_colour() {
    let mut r = Rig::with(100, 30, script()).await;
    r.runner.chat.cfg.color = true;
    r.ready().await;
    asked(&mut r, edit_question(UNIFIED)).await;
    let raw = String::from_utf8_lossy(&r.term.lock().unwrap().raw).into_owned();
    // The removed line is red and its words `lowest`, `highest` reversed on red;
    // the added line green with its own marks.
    assert!(raw.contains("\x1b[1;97;41mlowest\x1b[0m"), "{raw:?}");
    assert!(raw.contains("\x1b[1;97;42mhighest\x1b[0m"), "{raw:?}");
    assert!(raw.contains("\x1b[31m"), "red for the removed line");
    assert!(raw.contains("\x1b[32m"), "green for the added line");
}

#[tokio::test(start_paused = true)]
async fn a_long_diff_line_continues_behind_an_arrow_at_80_columns() {
    let long = "x".repeat(30);
    let unified =
        format!("--- a.py\n+++ a.py\n@@ -1 +1 @@\n-name = 1\n+name = \"{long} {long} {long}\"\n");
    let mut r = Rig::with(80, 30, script()).await;
    r.ready().await;
    asked(&mut r, edit_question(&unified)).await;
    let text = r.text();
    assert!(text.contains("↪ "), "{text}");
    assert!(!text.contains('…') || text.contains("more rows"), "{text}");
    let shown: usize = text
        .lines()
        .filter(|l| l.contains("+ name") || l.contains("↪ "))
        .map(|l| l.matches('x').count())
        .sum();
    assert_eq!(shown, 90, "no character is cut:\n{text}");
}

#[tokio::test(start_paused = true)]
async fn y_confirms_with_the_author_theseus_chat_and_the_dock_paints_in_one_frame() {
    let mut r = Rig::with(80, 24, script()).await;
    r.ready().await;
    r.starts_turn("trn_1");
    r.settle().await;
    let frames = r.runner.frames;
    r.tell(Event::ConfirmRequested(edit_question(UNIFIED)));
    r.settle().await;
    assert!(r.text().contains("y yes"), "{}", r.text());
    assert!(r.runner.frames - frames <= 1, "one frame for the question");
    r.press(KeyCode::Char('y')).await;
    r.asked("action.confirm", 1).await;
    let a = &r.daemon.asked("action.confirm")[0];
    assert_eq!(a["correlation_id"], "act_88");
    assert_eq!(a["approve"], true);
    assert_eq!(a["author"], "theseus chat");
    // A second y before the daemon has answered is not a second answer.
    r.press(KeyCode::Char('y')).await;
    assert_eq!(r.daemon.asked("action.confirm").len(), 1);
    assert!(r.text().contains("answering…"), "{}", r.text());
}

#[tokio::test(start_paused = true)]
async fn a_message_that_starts_with_y_is_a_message() {
    let mut r = Rig::with(80, 24, script()).await;
    r.ready().await;
    asked(&mut r, edit_question(UNIFIED)).await;
    // Esc goes back to typing: the question stays, one row high.
    r.press(KeyCode::Esc).await;
    assert!(r.text().contains("? Edit tides.py"), "{}", r.text());
    assert!(r.text().contains("tab: answer it"), "{}", r.text());
    r.type_text("yes, but keep the rounding").await;
    r.press(KeyCode::Enter).await;
    assert!(
        r.daemon.asked("action.confirm").is_empty(),
        "the y was text"
    );
    // The turn waits on the question, so the message waits in the dock; it
    // goes as a message when the turn ends.
    r.shows("queued › yes, but keep the rounding").await;
    r.ends_turn("trn_1", "");
    r.asked("turn.submit", 1).await;
    assert_eq!(
        r.daemon.asked("turn.submit")[0]["input"],
        "yes, but keep the rounding"
    );
}

#[tokio::test(start_paused = true)]
async fn a_character_that_is_no_answer_starts_the_message_and_tab_gives_the_focus_back() {
    let mut r = Rig::with(80, 24, script()).await;
    r.ready().await;
    asked(&mut r, edit_question(UNIFIED)).await;
    r.type_text("w").await;
    r.type_text("y").await;
    assert!(r.daemon.asked("action.confirm").is_empty());
    assert!(r.text().contains("› wy"), "{}", r.text());
    r.ctrl('u').await;
    r.press(KeyCode::Esc).await;
    r.press(KeyCode::Tab).await;
    assert!(r.text().contains("y yes"), "{}", r.text());
    r.press(KeyCode::Char('y')).await;
    r.asked("action.confirm", 1).await;
}

#[tokio::test(start_paused = true)]
async fn n_takes_a_note_and_enter_sends_the_decline_with_it() {
    let mut r = Rig::with(80, 24, script()).await;
    r.ready().await;
    asked(&mut r, edit_question(UNIFIED)).await;
    r.press(KeyCode::Char('n')).await;
    r.shows("say what to do instead").await;
    assert!(
        r.daemon.asked("action.confirm").is_empty(),
        "n waits for Enter"
    );
    r.type_text("use min and max").await;
    r.press(KeyCode::Enter).await;
    r.asked("action.confirm", 1).await;
    let a = &r.daemon.asked("action.confirm")[0];
    assert_eq!(a["approve"], false);
    assert_eq!(a["note"], "use min and max");
    assert_eq!(a["author"], "theseus chat");
    assert!(
        r.daemon.asked("turn.submit").is_empty(),
        "a note is not a message"
    );
}

#[tokio::test(start_paused = true)]
async fn n_then_enter_alone_declines_with_no_note_and_esc_goes_back() {
    let mut r = Rig::with(80, 24, script()).await;
    r.ready().await;
    asked(&mut r, edit_question(UNIFIED)).await;
    r.press(KeyCode::Char('n')).await;
    r.press(KeyCode::Esc).await;
    assert!(r.text().contains("y yes"), "{}", r.text());
    r.press(KeyCode::Char('n')).await;
    r.press(KeyCode::Enter).await;
    r.asked("action.confirm", 1).await;
    let a = &r.daemon.asked("action.confirm")[0];
    assert_eq!((a["approve"].as_bool(), a.get("note")), (Some(false), None));
}

#[tokio::test(start_paused = true)]
async fn v_writes_the_whole_call_into_the_transcript() {
    let mut r = Rig::with(80, 40, script()).await;
    r.ready().await;
    asked(&mut r, edit_question(UNIFIED)).await;
    r.press(KeyCode::Char('v')).await;
    r.shows("the whole call · Edit tides.py").await;
    assert!(r.daemon.asked("action.confirm").is_empty());
    assert!(
        r.text().contains("y yes"),
        "the question stays: {}",
        r.text()
    );
}

#[tokio::test(start_paused = true)]
async fn the_grant_keys_follow_the_offer() {
    let mut q = edit_question(UNIFIED);
    q.grant_offer = Some(theseus_protocol::grants::GrantOffer::mode_edits());
    let mut r = Rig::with(100, 30, script()).await;
    r.ready().await;
    asked(&mut r, q).await;
    assert!(
        r.text()
            .contains("e  yes, and move this conversation to edits mode"),
        "{}",
        r.text()
    );
    // `a` is no key of this offer: it offers edits mode.
    r.press(KeyCode::Char('a')).await;
    assert!(r.daemon.asked("action.confirm").is_empty());
    assert!(
        r.text().contains("› a"),
        "an unoffered key is text:\n{}",
        r.text()
    );
    r.ctrl('u').await;
    r.press(KeyCode::Char('e')).await;
    r.asked("action.confirm", 1).await;
    let a = &r.daemon.asked("action.confirm")[0];
    assert_eq!(a["approve"], true);
    assert_eq!(a["grant"]["pattern"], "edits");
}

// ------------------------------------------------------------ the stop

#[tokio::test(start_paused = true)]
async fn esc_during_a_turn_stops_it_and_asks_what_to_do_instead() {
    let mut r = Rig::with(100, 24, script()).await;
    r.ready().await;
    r.type_text("explain how tides are predicted, at length")
        .await;
    r.press(KeyCode::Enter).await;
    r.starts_turn("trn_1");
    r.shows("◐ thinking").await;
    r.press(KeyCode::Esc).await;
    r.asked("execution.stop", 1).await;
    let s = &r.daemon.asked("execution.stop")[0];
    assert_eq!(s["execution_id"], EXE);
    assert_eq!(s["author"], "theseus chat");
    assert_eq!(s["keep_jobs"], true, "Esc stops the turn, not the jobs");
    r.shows("stopped by you (esc) · verified: 2 processes ended")
        .await;
    assert!(
        r.text()
            .contains("What should it do instead? (enter: nothing · ↑: edit your last message)"),
        "{}",
        r.text()
    );
    // ↑ brings the last message back.
    r.ends_turn("trn_1", "");
    r.press(KeyCode::Up).await;
    assert!(r.text().contains("› explain how tides"), "{}", r.text());
}

#[tokio::test(start_paused = true)]
async fn esc_with_no_turn_running_stops_nothing() {
    let mut r = Rig::with(80, 24, script()).await;
    r.ready().await;
    // A job the last turn left in the background is not a turn.
    r.tell(Event::ToolProposed(ToolProposed {
        session_id: SID.into(),
        tool_use_id: "tu_1".into(),
        tool: "proc.run".into(),
        input: json!({"argv": ["python3", "slow_survey.py"]}),
        ..Default::default()
    }));
    r.tell(Event::ToolStarted(ToolStarted {
        session_id: SID.into(),
        tool_use_id: "tu_1".into(),
        tool: "proc.run".into(),
        ..Default::default()
    }));
    r.shows("slow_survey.py").await;
    r.press(KeyCode::Esc).await;
    r.press(KeyCode::Esc).await;
    r.settle().await;
    assert!(
        r.daemon.asked("execution.stop").is_empty(),
        "Esc stopped the background job"
    );
    assert!(r.text().contains("slow_survey.py"), "{}", r.text());
}

#[tokio::test(start_paused = true)]
async fn esc_on_a_question_goes_back_to_typing_before_it_stops_anything() {
    let mut r = Rig::with(80, 24, script()).await;
    r.ready().await;
    asked(&mut r, edit_question(UNIFIED)).await;
    r.press(KeyCode::Esc).await;
    assert!(r.daemon.asked("execution.stop").is_empty());
    r.press(KeyCode::Esc).await;
    r.asked("execution.stop", 1).await;
}

// ------------------------------------------------------------ the queue

#[tokio::test(start_paused = true)]
async fn a_message_typed_while_a_turn_runs_waits_in_the_dock_and_goes_when_it_ends() {
    let mut r = Rig::with(80, 24, script()).await;
    r.ready().await;
    r.type_text("first").await;
    r.press(KeyCode::Enter).await;
    r.asked("turn.submit", 1).await;
    r.starts_turn("trn_1");
    r.shows("◐ thinking").await;
    r.type_text("and then the second").await;
    r.press(KeyCode::Enter).await;
    r.shows("queued › and then the second").await;
    assert_eq!(r.daemon.asked("turn.submit").len(), 1, "it waits");
    assert!(
        !r.screen().iter().any(|l| l.starts_with("› and then")),
        "not in the transcript yet:\n{}",
        r.text()
    );
    r.answers("trn_1", "One.");
    r.ends_turn("trn_1", "One.");
    r.asked("turn.submit", 2).await;
    assert_eq!(
        r.daemon.asked("turn.submit")[1]["input"],
        "and then the second"
    );
    assert!(!r.text().contains("queued"), "{}", r.text());
    assert!(r.text().contains("› and then the second"), "{}", r.text());
}

#[tokio::test(start_paused = true)]
async fn up_pulls_a_queued_message_back_to_edit() {
    let mut r = Rig::with(80, 24, script()).await;
    r.ready().await;
    r.starts_turn("trn_1");
    r.type_text("second thoughts").await;
    r.press(KeyCode::Enter).await;
    r.shows("queued › second thoughts").await;
    r.press(KeyCode::Up).await;
    assert!(!r.text().contains("queued"), "{}", r.text());
    assert!(r.text().contains("› second thoughts"), "{}", r.text());
    r.type_text(" and more").await;
    r.press(KeyCode::Enter).await;
    r.shows("queued › second thoughts and more").await;
    r.ends_turn("trn_1", "");
    r.asked("turn.submit", 1).await;
    assert_eq!(
        r.daemon.asked("turn.submit")[0]["input"],
        "second thoughts and more"
    );
}

#[tokio::test(start_paused = true)]
async fn ctrl_enter_stops_the_turn_and_sends_the_message_now() {
    let mut r = Rig::with(100, 24, script()).await;
    r.ready().await;
    r.starts_turn("trn_1");
    r.type_text("stop that and do this").await;
    r.key(KeyEvent::new(KeyCode::Enter, KeyModifiers::CONTROL))
        .await;
    r.asked("execution.stop", 1).await;
    assert!(
        r.daemon.asked("turn.submit").is_empty(),
        "it goes when the turn ends"
    );
    r.shows("your queued message goes next").await;
    r.ends_turn("trn_1", "");
    r.asked("turn.submit", 1).await;
    assert_eq!(
        r.daemon.asked("turn.submit")[0]["input"],
        "stop that and do this"
    );
    assert!(
        !r.text().contains("What should it do instead?"),
        "the queued message answers it:\n{}",
        r.text()
    );
}

// ------------------------------------------------------------ after it

#[tokio::test(start_paused = true)]
async fn the_finished_edits_diff_stays_in_the_transcript() {
    let mut r = Rig::with(100, 30, script()).await;
    r.ready().await;
    r.starts_turn("trn_1");
    r.tell(Event::ToolProposed(ToolProposed {
        session_id: SID.into(),
        turn_id: "trn_1".into(),
        tool_use_id: "tu_1".into(),
        tool: "fs.edit".into(),
        input: json!({"path": "/home/ada/work/harbour-tides/tides.py"}),
        ..Default::default()
    }));
    r.tell(Event::ToolEnded(ToolEnded {
        session_id: SID.into(),
        turn_id: "trn_1".into(),
        tool_use_id: "tu_1".into(),
        tool: "fs.edit".into(),
        status: "ok".into(),
        duration_ms: Some(40),
        diff: Some(ToolDiff {
            path: "tides.py".into(),
            added: 1,
            removed: 1,
            unified: UNIFIED.into(),
            truncated: false,
        }),
        ..Default::default()
    }));
    r.shows("● Edit tides.py  +1 −1").await;
    let text = r.text();
    assert!(
        text.contains("49 -     return round(lowest - highest, 2)"),
        "{text}"
    );
    assert!(
        text.contains("49 +     return round(highest - lowest, 2)"),
        "{text}"
    );
    r.ends_turn("trn_1", "Fixed.");
    r.shows("1.2 s").await;
    assert!(
        r.text().contains("49 + "),
        "it stays after the turn: {}",
        r.text()
    );
}

#[tokio::test(start_paused = true)]
async fn a_commands_result_lines_stay_under_its_line_behind_a_gutter() {
    let mut r = Rig::with(100, 30, script()).await;
    r.ready().await;
    r.starts_turn("trn_1");
    r.tell(Event::ToolProposed(ToolProposed {
        session_id: SID.into(),
        turn_id: "trn_1".into(),
        tool_use_id: "tu_1".into(),
        tool: "proc.run".into(),
        input: json!({"argv": ["python3", "-m", "unittest", "-v", "test_tides"]}),
        ..Default::default()
    }));
    r.tell(Event::ToolEnded(ToolEnded {
        session_id: SID.into(),
        turn_id: "trn_1".into(),
        tool_use_id: "tu_1".into(),
        tool: "proc.run".into(),
        status: "ok".into(),
        exit_code: Some(1),
        duration_ms: Some(900),
        preview: "[exit code 1 · in /app]\ntest_tidal_range (test_tides.T) ... FAIL\nAssertionError: -2.8 != 2.8\n".into(),
        ..Default::default()
    }));
    r.shows("AssertionError: -2.8 != 2.8").await;
    assert!(r.text().contains("⎿ test_tidal_range"), "{}", r.text());
}
