//! `look.work`'s input, its lines and its bounds, on the facts alone. The played turns, through the
//! whole core, are `config_change/tests_work.rs`.

use serde_json::json;

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
