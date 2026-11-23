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
    JobKept, ToolDiff, ToolEnded, ToolProposed, ToolStarted,
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
        agents_running: vec![],
        cancelled: vec![],
        left_ended: vec![],
        left_kept: vec![],
    })
    .unwrap()
}

/// The quiet daemon of `tests.rs`, whose session has an execution and whose
/// stop answers with two processes ended.
fn script() -> Script {
    script_stopping(stop_result(Some(2)))
}

/// The same daemon, its stop answering `stop`.
fn script_stopping(stop: Value) -> Script {
    Arc::new(move |m, _p| -> Answer {
        Ok(match m {
            "health" => json!({"name": "theseusd", "model": "claude-sonnet-5-5", "profile": "f10"}),
            "session.open" => json!({
                "session_id": SID, "kind": "conversation", "label": null,
                "created_at_unix_ms": T0, "turns": 0, "execution_id": EXE,
            }),
            "session.list" => json!({"sessions": []}),
            "session.watch" => json!({"watching": SID, "watchers": 1}),
            "execution.stop" => stop.clone(),
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

/// A call's question waits past the life it would have had, held rather than
/// expired (theseus-lrc3m): the dock says how long it has waited, as the board's
/// card does, and never "expires" or "expired".
#[tokio::test(start_paused = true)]
async fn a_held_questions_dock_says_how_long_it_waited_not_that_it_expired() {
    let mut r = Rig::with(120, 30, script()).await;
    r.ready().await;
    asked(&mut r, edit_question(UNIFIED)).await;
    assert!(r.text().contains("waiting 0 s"), "{}", r.text());
    r.advance(20 * 60_000).await;
    let text = r.text();
    assert!(text.contains("Theseus asks · waiting 20 min"), "{text}");
    assert!(!text.contains("expire"), "{text}");
}

#[tokio::test(start_paused = true)]
async fn a_question_shows_the_hunk_numbered_with_who_asks_and_how_long_it_waited() {
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
            "Theseus asks · waiting 0 s",
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
    // goes as a message when the question is resolved and the turn ends.
    r.shows("queued › yes, but keep the rounding").await;
    r.daemon.notify(
        "confirm.resolved",
        json!({
            "session_id": SID, "correlation_id": "act_88", "approved": false,
            "option": "decline", "expired": false
        }),
    );
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

/// A stop that reached past the turn: two tasks cancelled, two jobs ended, and
/// two jobs kept, each on its own approver's word.
fn stop_that_reached() -> ExecutionStopResult {
    let mut r: ExecutionStopResult = serde_json::from_value(stop_result(Some(2))).unwrap();
    r.cancelled = vec!["exe_ses_aaa111".into(), "exe_ses_bbb222".into()];
    r.left_ended = vec!["j1".into(), "j2".into()];
    r.left_kept = vec![
        JobKept {
            job: "j3".into(),
            approved_by: "discord:operator".into(),
        },
        JobKept {
            job: "job_9f".into(),
            approved_by: "the TUI".into(),
        },
    ];
    r
}

/// Esc under `script_stopping(r)`, then the transcript as one run of words:
/// a long answer wraps, and a wrap is not a difference.
async fn esc_answered(r: &ExecutionStopResult) -> String {
    let mut rig = Rig::with(100, 24, script_stopping(serde_json::to_value(r).unwrap())).await;
    rig.ready().await;
    rig.type_text("explain how tides are predicted").await;
    rig.press(KeyCode::Enter).await;
    rig.starts_turn("trn_1");
    rig.shows("◐ thinking").await;
    rig.press(KeyCode::Esc).await;
    rig.asked("execution.stop", 1).await;
    rig.shows("What should it do instead?").await;
    rig.text().split_whitespace().collect::<Vec<_>>().join(" ")
}

#[tokio::test(start_paused = true)]
async fn a_stop_says_the_jobs_it_ended_and_how_many() {
    let mut r = stop_that_reached();
    (r.cancelled, r.left_kept) = (vec![], vec![]);
    let said = esc_answered(&r).await;
    assert!(
        said.contains(
            "stopped by you (esc) · verified: 2 processes ended · ended 2 job(s) that had left \
             something running: j1 j2"
        ),
        "{said}"
    );
    assert!(
        !said.contains("kept job") && !said.contains("cancelled 2"),
        "{said}"
    );
}

#[tokio::test(start_paused = true)]
async fn a_stop_names_who_approved_each_job_it_kept() {
    let mut r = stop_that_reached();
    (r.cancelled, r.left_ended) = (vec![], vec![]);
    let said = esc_answered(&r).await;
    assert!(
        said.contains(
            "verified: 2 processes ended · kept job j3 running, approved by discord:operator \
             · kept job job_9f running, approved by the TUI"
        ),
        "{said}"
    );
    assert!(
        !said.contains("ended 2 job(s)") && !said.contains("cancelled 2"),
        "{said}"
    );
}

#[tokio::test(start_paused = true)]
async fn a_stop_that_reached_nothing_past_the_turn_says_what_it_always_did() {
    let mut r = stop_that_reached();
    (r.cancelled, r.left_ended, r.left_kept) = (vec![], vec![], vec![]);
    let said = esc_answered(&r).await;
    assert!(
        said.contains(
            "stopped by you (esc) · verified: 2 processes ended What should it do instead?"
        ),
        "{said}"
    );
    for word in ["cancelled", "job(s)", "kept job"] {
        assert!(!said.contains(word), "{word}: {said}");
    }
}

/// An agent the stopped turn handed work to is said to be still working, in the one wording the CLI and the
/// other surfaces use, with its own cancel.
#[tokio::test(start_paused = true)]
async fn a_stop_says_an_agent_the_turn_handed_work_to_is_still_working() {
    let mut r = stop_that_reached();
    (r.cancelled, r.left_ended, r.left_kept) = (vec![], vec![], vec![]);
    r.agents_running = vec![theseus_protocol::StopAgent {
        profile: "haiku".into(),
        model: "claude-haiku-5-5".into(),
        task: "a1b2c3".into(),
    }];
    let said = esc_answered(&r).await;
    assert!(
        said.contains(
            "verified: 2 processes ended · The agent haiku, handed work (claude-haiku-5-5), is \
             still working: `theseus cancel a1b2c3` stops it."
        ),
        "{said}"
    );
}

/// The three surfaces that word a stop must not drift: the chat pane says the
/// reach of a stop (tasks cancelled, jobs ended, jobs kept and on whose word)
/// exactly as `theseus stop` prints it, in the same order.
#[tokio::test(start_paused = true)]
async fn the_chat_pane_and_theseus_stop_say_the_same_reach() {
    let r = stop_that_reached();
    let said = esc_answered(&r).await;
    let line = theseus_client::render::stop_line(&r);
    let (_, reach) = line
        .split_once(" · cancelled ")
        .expect("the CLI says the cancelled tasks");
    assert!(
        said.contains(&format!("cancelled {reach}")),
        "{said}\n{line}"
    );
    // The order is the CLI's: tasks, then ended, then each kept.
    let at = |w: &str| said.find(w).unwrap_or_else(|| panic!("{w}: {said}"));
    assert!(
        at("cancelled 2 task(s): aaa111 bbb222") < at("ended 2 job(s)"),
        "{said}"
    );
    assert!(at("ended 2 job(s)") < at("kept job j3"), "{said}");
    assert!(at("kept job j3") < at("kept job job_9f"), "{said}");
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

/// The daemon's card for a changed prompt file, as the daemon words it (issue 23).
const PROMPT_CARD: &str = "Change the prompt file /state/PERSONA.md?\nIt is Theseus's file of persona warm.\nBefore: 120 bytes, 4 lines, fingerprint 0123456789abcdef\nAfter: 131 bytes, 4 lines, fingerprint fedcba9876543210\n1 line added, 1 line removed.\nThe changed lines, whole (unseen characters are written \\u{..}; a backslash is doubled):\n```diff\n@@ -1,4 +1,4 @@\n You are Theseus.\n-Be brief.\n+Be brief and warm.\n Be honest.\n```\nTakes effect with the next message; no restart.\n";

/// A change to a prompt file is a settings question: the dock holds a line and
/// says to read the card first; `v` writes the daemon's card into the transcript
/// with each line as a row of its own, its marker and spaces kept, nothing run
/// together or cut; and only then is `y` taken (issue 23).
#[tokio::test(start_paused = true)]
async fn a_prompt_file_card_is_read_line_by_line_before_a_yes() {
    let mut r = Rig::with(100, 40, script()).await;
    r.ready().await;
    r.starts_turn("trn_1");
    let q = ConfirmRequest {
        correlation_id: "act_99".into(),
        session_id: SID.into(),
        tool: "config.ack".into(),
        input: json!({"whole": true, "kind": "prompt_file"}),
        reason: PROMPT_CARD.into(),
        by: "operator".into(),
        requested_at_ms: T0,
        expires_at_ms: T0 + 300_000,
        needs_proof: true,
        plain: Some(theseus_protocol::PlainWords {
            who: "Theseus".into(),
            what: "change the prompt file `/state/PERSONA.md` (1 line added, 1 line removed); no restart".into(),
            yes: "has the daemon write the file itself; it takes effect with the next message.".into(),
            no: "changes nothing.".into(),
            detail: PROMPT_CARD.into(),
            ..Default::default()
        }),
        ..Default::default()
    };
    r.tell(Event::ConfirmRequested(q));
    r.shows("v: read the whole card first").await;
    r.press(KeyCode::Char('y')).await;
    r.shows("press v to read the whole card first, then y")
        .await;
    assert!(r.daemon.asked("action.confirm").is_empty());
    r.press(KeyCode::Char('v')).await;
    r.shows("+Be brief and warm.").await;
    let screen = lines_of(&r);
    for want in PROMPT_CARD.lines() {
        assert!(
            screen
                .iter()
                .any(|l| l.trim_end() == format!("  {want}").trim_end()),
            "{want:?} is not a row of its own:\n{}",
            r.text()
        );
    }
    assert!(!r.text().contains('…'), "{}", r.text());
    r.press(KeyCode::Char('y')).await;
    r.asked("action.confirm", 1).await;
}

fn settings_question() -> ConfirmRequest {
    let mut card = String::from("Change 2 settings?\n");
    for i in 0..2 {
        card.push_str(&format!(
            "- memory.k{i}: \"{}\" → \"{}!\" (after a restart)\n",
            "a".repeat(380),
            "b".repeat(380)
        ));
    }
    ConfirmRequest {
        correlation_id: "act_77".into(),
        session_id: SID.into(),
        tool: "config.ack".into(),
        reason: card.clone(),
        by: "the model".into(),
        requested_at_ms: T0,
        expires_at_ms: T0 + 300_000,
        plain: Some(theseus_protocol::PlainWords {
            who: "Theseus".into(),
            what: "change 2 settings".into(),
            yes: "has the daemon write these settings itself.".into(),
            no: "changes nothing.".into(),
            detail: card,
            ..Default::default()
        }),
        ..Default::default()
    }
}

/// A yes on a settings question is taken only after its whole card has been
/// written into the transcript with `v`: the dock holds a line, not a card.
#[tokio::test(start_paused = true)]
async fn y_on_a_settings_question_waits_for_v() {
    let mut r = Rig::with(100, 40, script()).await;
    r.ready().await;
    r.starts_turn("trn_1");
    r.tell(Event::ConfirmRequested(settings_question()));
    r.shows("v: read the whole card first").await;
    r.press(KeyCode::Char('y')).await;
    r.shows("press v to read the whole card first, then y")
        .await;
    assert!(
        r.daemon.asked("action.confirm").is_empty(),
        "no yes under a line of the card"
    );
    r.press(KeyCode::Char('v')).await;
    r.shows("memory.k1").await;
    let text = r.text();
    assert!(
        text.matches('a').count() > 700 && text.matches('b').count() > 700,
        "every character of both values is in the transcript"
    );
    r.press(KeyCode::Char('y')).await;
    r.asked("action.confirm", 1).await;
    assert_eq!(r.daemon.asked("action.confirm")[0]["approve"], true);
}

/// A source the model gave is a row of the card `v` writes into the transcript,
/// under its setting, as one row with the care line after it as it was.
#[tokio::test(start_paused = true)]
async fn a_source_is_a_row_of_the_card_read_with_v() {
    let source = "  Theseus's source for this, in its own words (nothing here checked it): \"the docs say it\"";
    let card = format!(
        "Change 1 setting?\n- providers.lantern.api_base: not set (its default) \u{2192} \"https://lantern.example.test\" (after a restart)\n{source}\nTake care: since your last message this conversation has read text from outside (a page, a search or a server), so the request may not be only yours.\nTheseus's reason, in its own words: \"x\"\n"
    );
    let mut q = settings_question();
    q.reason = card.clone();
    if let Some(p) = q.plain.as_mut() {
        p.detail = card;
    }
    let mut r = Rig::with(140, 40, script()).await;
    r.ready().await;
    r.starts_turn("trn_1");
    r.tell(Event::ConfirmRequested(q));
    r.shows("v: read the whole card first").await;
    r.press(KeyCode::Char('v')).await;
    r.shows("nothing here checked it").await;
    let screen = lines_of(&r);
    let at = screen
        .iter()
        .position(|l| l.contains("- providers.lantern.api_base"))
        .expect("the setting's row");
    assert_eq!(screen[at + 1].trim(), source.trim(), "{}", r.text());
    assert!(
        screen[at + 2].contains("Take care: since your last message"),
        "{}",
        r.text()
    );
}
