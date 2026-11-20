//! `look.work`'s input, its lines and its bounds, on the facts alone. The played turns, through the
//! whole core, are `config_change/tests_work.rs`.

use serde_json::json;

use super::tasks::{question_line, task_line};
use super::*;

/// No part named is all of them, in order; a part named once, whatever order; a part that is not one,
/// a field that is not the tool's, and a count out of range are refused or clamped.
#[test]
fn the_input_names_parts_once_in_order_and_bounds_the_messages() {
    let all = ask_of(&json!({})).unwrap();
    assert_eq!(all.parts, PARTS);
    assert_eq!(all.messages, MESSAGES);
    let some = ask_of(&json!({"what": ["history", "tasks", "history"], "messages": 3})).unwrap();
    assert_eq!(
        (some.parts.as_slice(), some.messages),
        (&["tasks", "history"][..], 3)
    );
    assert_eq!(ask_of(&json!({"messages": 0})).unwrap().messages, 1);
    assert_eq!(
        ask_of(&json!({"messages": 500})).unwrap().messages,
        MESSAGES_MAX
    );
    for bad in [
        json!({"what": ["vault"]}),
        json!({"session": "ses_other"}),
        json!({"what": ["tasks"], "place": "dm:1"}),
        json!({"messages": -1}),
    ] {
        assert!(ask_of(&bad).is_err(), "{bad}");
    }
}

/// A task is one line with what a person asks of it: its id, title, state, what it waits on, turns,
/// spend and what it asks; a question is one line and offers no answer.
#[test]
fn a_task_and_a_question_are_each_one_line() {
    let t = theseus_protocol::TaskInfo {
        short: "a1b2c3".into(),
        title: Some("Chart the reef".into()),
        state: "waiting".into(),
        waiting_on: Some("confirm".into()),
        turns: 2,
        spent_usd: 0.1234,
        limit_usd: 2.0,
        pending_confirms: 1,
        ..Default::default()
    };
    assert_eq!(
        task_line(&t),
        "- a1b2c3 · Chart the reef · waiting (waiting on confirm) · 2 turns · $0.12 of $2.00 · asks the owner 1\n"
    );
    let q = theseus_protocol::ConfirmRequest {
        correlation_id: "act_01abcdef123456".into(),
        tool: "proc.run".into(),
        reason: "it   touches\nthe floor".into(),
        ..Default::default()
    };
    assert_eq!(
        question_line(&q),
        "- 123456 · proc.run · it touches the floor\n"
    );
}

/// Every line is one line and a long one is cut; the whole answer is cut and says so.
#[test]
fn a_line_is_clipped_and_the_whole_answer_is_cut_and_says_so() {
    assert_eq!(clip("a  b\n c", 10), "a b c");
    assert_eq!(clip(&"x".repeat(700), 600).chars().count(), 601);
    let long = "line\n".repeat(5_000);
    let shown = cut(long, TOTAL_CHARS);
    assert!(shown.chars().count() < TOTAL_CHARS + 120, "{}", shown.len());
    assert!(shown.contains("[cut at 8000 characters"), "{shown}");
    assert_eq!(cut("short".into(), 100), "short");
}

/// A caller may name a scope: the default is the whole of Theseus's work, `here` is this conversation's, and
/// the list of parts is six, history last.
#[test]
fn the_scope_is_global_unless_it_says_here() {
    assert!(!ask_of(&json!({})).unwrap().here);
    assert!(!ask_of(&json!({"scope": "all"})).unwrap().here);
    let here = ask_of(&json!({"scope": " here "})).unwrap();
    assert!(here.here);
    assert_eq!(
        here.summary(),
        format!("{} (this conversation)", PARTS.join(", "))
    );
    for bad in [json!({"scope": "elsewhere"}), json!({"scope": ["here"]})] {
        assert!(ask_of(&bad).is_err(), "{bad}");
    }
    assert_eq!(PARTS.last(), Some(&"history"));
}

/// The harness is named for its own words alone; a task's report, a wake's note and the rest carry other
/// words and say what they are (`content::harness_own`).
#[test]
fn a_message_is_named_for_what_it_is() {
    use crate::node::Origin;
    let own = crate::content::HARNESS_AUTHORS[0];
    assert_eq!(history::who(Origin::Operator, None), "the owner");
    assert_eq!(history::who(Origin::Harness, Some(own)), "the harness");
    assert_eq!(
        history::who(Origin::Harness, Some("task:ses_a1b2c3")),
        "a task's report"
    );
    assert_eq!(
        history::who(Origin::Harness, Some("wake:wak_x")),
        "a wake's note"
    );
    assert_eq!(
        history::who(Origin::Harness, Some("harness:end_note")),
        "a note that carries other words"
    );
    assert_eq!(
        history::who(Origin::Harness, None),
        "a note that carries other words"
    );
    assert_eq!(history::who(Origin::Agent, None), "a message");
}

/// A control character is no part of a line, and every part is capped: nothing else can enlarge the answer.
#[test]
fn a_line_has_no_control_characters_and_the_caps_hold_the_answer() {
    assert_eq!(clip("a\u{1b}[31mb\u{0}c", 20), "a [31mb c");
    const { assert!(TOTAL_CHARS < ANSWER_CHARS) };
    assert_eq!(ANSWER_CHARS, 24_000);
    assert_eq!(TOTAL_CHARS, 8_000);
}

/// Every short field is clipped by itself, not left to the part's cut: a place's name, a question's tool
/// name, a task's state and what it waits on, a routine's series, a wake's span, and a project's state.
#[test]
fn each_short_field_is_clipped_by_itself() {
    use super::horizon::{project_line, routine_row, wake_row};
    use super::reader::{place_words, FIELD_CHARS};
    let long = "x".repeat(5_000);
    let room = |line: &str| line.chars().count() < 1_000;
    // A place's name, bound or the CLI's.
    assert!(place_words(&long).chars().count() <= FIELD_CHARS + 1);
    let cli = place_words(&format!("{long} on the CLI or the web UI"));
    assert!(
        cli.starts_with("the CLI or the web UI (…") && room(&cli),
        "{cli}"
    );
    // A question's tool name.
    let q = theseus_protocol::ConfirmRequest {
        correlation_id: "act_01abcdef123456".into(),
        tool: long.clone(),
        reason: "why".into(),
        ..Default::default()
    };
    assert!(room(&question_line(&q)), "{}", question_line(&q));
    // A task's state and what it waits on.
    let t = theseus_protocol::TaskInfo {
        short: "a1b2c3".into(),
        state: long.clone(),
        waiting_on: Some(long.clone()),
        ..Default::default()
    };
    assert!(room(&task_line(&t)), "{}", task_line(&t));
    // A routine's series, a wake's span and a project's state.
    let routine: theseus_protocol::routines::RoutineInfo = serde_json::from_value(json!({
        "id": "ses_x", "short": "x", "title": "t", "schedule": "s", "series": long, "brief": "b",
        "paused": false, "run_usd": 0.0, "month_usd": 0.0, "month_spent_usd": 0.0,
        "deliver_to": "dm", "catch_up": "auto", "speak": "always", "carry": "none", "runs": 0
    }))
    .unwrap();
    assert!(room(&routine_row(&routine, "t", None)));
    let wake = theseus_protocol::WakeInfo {
        short: long.clone(),
        due_local: long.clone(),
        every: Some(long.clone()),
        ..Default::default()
    };
    assert!(room(&wake_row(&wake, "", "note")));
    let project = theseus_protocol::projects::ProjectInfo {
        id: "tsk_x".into(),
        state: long,
        ..Default::default()
    };
    assert!(room(&project_line(&project, "t", None)));
}
