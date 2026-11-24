//! `look.work`'s input, its lines and its bounds, on the facts alone. The played turns, through the
//! whole core, are `config_change/tests_work.rs`.

use serde_json::json;

use super::tasks::{question_line, task_line};
use super::*;
use crate::scrub::Cuts;

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
        task_line(&t, &Cuts::none()),
        "- a1b2c3 · Chart the reef · waiting (waiting on confirm) · 2 turns · $0.12 of $2.00 · asks the owner 1\n"
    );
    let q = theseus_protocol::ConfirmRequest {
        correlation_id: "act_01abcdef123456".into(),
        tool: "proc.run".into(),
        reason: "it   touches\nthe floor".into(),
        ..Default::default()
    };
    assert_eq!(
        question_line(&q, &Cuts::none()),
        "- 123456 · proc.run · it touches the floor\n"
    );
}

/// Every line is one line and a long one is cut; the whole answer is cut and says so.
#[test]
fn a_line_is_clipped_and_the_whole_answer_is_cut_and_says_so() {
    assert_eq!(clip("a  b\n c", 10, &Cuts::none()), "a b c");
    assert_eq!(
        clip(&"x".repeat(700), 600, &Cuts::none()).chars().count(),
        601
    );
    let long = "line\n".repeat(5_000);
    let shown = cut(long, TOTAL_CHARS, &Cuts::none());
    assert!(shown.chars().count() < TOTAL_CHARS + 120, "{}", shown.len());
    assert!(shown.contains("[cut at 8000 characters"), "{shown}");
    assert_eq!(cut("short".into(), 100, &Cuts::none()), "short");
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
    assert_eq!(clip("a\u{1b}[31mb\u{0}c", 20, &Cuts::none()), "a [31mb c");
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
    let none = Cuts::none();
    assert!(place_words(&long, &none).chars().count() <= FIELD_CHARS + 1);
    let cli = place_words(&format!("{long} on the CLI or the web UI"), &none);
    assert!(
        cli.starts_with("the CLI or the web UI (…") && room(&cli),
        "{cli}"
    );
    // The id's own end, not the end of a clipped id.
    assert!(
        place_words(&format!("ses_{long}abcdef on the CLI or the web UI"), &none)
            .ends_with("(…abcdef)"),
        "the id's own end"
    );
    // A question's tool name.
    let q = theseus_protocol::ConfirmRequest {
        correlation_id: "act_01abcdef123456".into(),
        tool: long.clone(),
        reason: "why".into(),
        ..Default::default()
    };
    assert!(
        room(&question_line(&q, &none)),
        "{}",
        question_line(&q, &none)
    );
    // A task's state and what it waits on.
    let t = theseus_protocol::TaskInfo {
        short: "a1b2c3".into(),
        state: long.clone(),
        waiting_on: Some(long.clone()),
        ..Default::default()
    };
    assert!(room(&task_line(&t, &none)), "{}", task_line(&t, &none));
    // A routine's series, a wake's span and a project's state.
    let routine: theseus_protocol::routines::RoutineInfo = serde_json::from_value(json!({
        "id": "ses_x", "short": "x", "title": "t", "schedule": "s", "series": long, "brief": "b",
        "paused": false, "run_usd": 0.0, "month_usd": 0.0, "month_spent_usd": 0.0,
        "deliver_to": "dm", "catch_up": "auto", "speak": "always", "carry": "none", "runs": 0
    }))
    .unwrap();
    assert!(room(&routine_row(&routine, "t", None, &none)));
    let wake = theseus_protocol::WakeInfo {
        short: long.clone(),
        due_local: long.clone(),
        every: Some(long.clone()),
        ..Default::default()
    };
    assert!(room(&wake_row(&wake, "", "note", &none)));
    let project = theseus_protocol::projects::ProjectInfo {
        id: "tsk_x".into(),
        state: long,
        ..Default::default()
    };
    assert!(room(&project_line(&project, "t", None, &none)));
}

/// A project's line carries its tasks as a count (issue 37), a few words and no list.
#[test]
fn a_projects_line_counts_its_tasks_and_names_none() {
    use super::horizon::project_line;
    use theseus_protocol::projects::ProjectInfo;
    let mut p = ProjectInfo {
        id: "tsk_00harbour".into(),
        state: "accepted".into(),
        ..Default::default()
    };
    let none = Cuts::none();
    let bare = project_line(&p, "Harbour survey", None, &none);
    assert!(!bare.contains("task"), "{bare}");
    p.tasks = 7;
    p.tasks_running = 1;
    let with = project_line(&p, "Harbour survey", None, &none);
    assert!(with.ends_with(" · 7 tasks, 1 running\n"), "{with}");
    p.tasks = 1;
    p.tasks_running = 0;
    assert!(project_line(&p, "Harbour survey", None, &none).ends_with(" · 1 task\n"));
    // What it costs the look: under thirty characters, about eight tokens.
    assert!(
        with.len() - bare.len() < 30,
        "{} chars",
        with.len() - bare.len()
    );
}

/// **What the answer's cuts leave decides what it copies** (D6 round 6). With every part asked, history comes
/// last, and the parts before it can fill the answer's 24,000 characters: a message the answer's cut leaves
/// out is no copy. The same message where the cut keeps it is one, with where its words are in the answer.
#[test]
fn a_message_the_answer_cut_leaves_out_is_no_copy() {
    let big = "x".repeat(TOTAL_CHARS + 100);
    let history = || "- the owner: hello\n".to_string();
    let hello = || vec![("msg_1".to_string(), 13, 18)];
    let (said, shown) = assemble(
        vec![
            (big.clone(), vec![]),
            (big.clone(), vec![]),
            (big.clone(), vec![]),
            (history(), hello()),
        ],
        &Cuts::none(),
    );
    assert!(
        said.contains("[cut at 24000 characters"),
        "the answer is cut"
    );
    assert!(!said.contains("hello"), "the history part is cut away");
    assert!(shown.is_empty(), "no copy: {shown:?}");
    let (said, shown) = assemble(vec![(history(), hello()), (big, vec![])], &Cuts::none());
    assert!(said.contains("hello"));
    assert_eq!(shown, hello());
}

/// `clip` as it was written before it said what it kept: the oracle its line is held to.
fn clip_as_it_was(s: &str, max: usize) -> String {
    let plain: String = s
        .chars()
        .map(|c| if c.is_control() { ' ' } else { c })
        .collect();
    let one: String = plain.split_whitespace().collect::<Vec<_>>().join(" ");
    match one.chars().count() > max {
        true => format!("{}…", one.chars().take(max).collect::<String>()),
        false => one,
    }
}

/// Over random text (characters of one to four bytes, control characters, runs of blanks, a line shorter
/// than, as long as and longer than the cap): the line is today's, and it is Clip1 of the bytes the clip says
/// it kept, then `…` exactly when words are left after them; those bytes split no character. Knowing no value,
/// the clip falls where it always fell. Knowing the values drawn into the text, the bytes it keeps split none
/// of them and hold none that has a blank in it (the rewrite would change it), the line is still Clip1 of them
/// and an ellipsis, and with no value in the text the line is the one it is knowing none.
#[test]
fn clip_reports_the_bytes_it_kept() {
    use crate::view::text::kept::sample::{cuts, verbatim, whole, Draw};
    let mut draw = Draw::new(0x2c1b_3c6d_6a09_e667);
    let none = Cuts::none();
    let blank = |c: char| c.is_control() || c.is_whitespace();
    for case in 0..1_500 {
        let max = [0, 1, 3, 8, 20, 80][draw.below(6)];
        let (text, known) = draw.case(max, 2 * max + 12);
        let (shown, kept) = clip_kept(&text, max, &none);
        assert_eq!(shown, clip_as_it_was(&text, max), "case {case}: {text:?}");
        assert_eq!(clip(&text, max, &none), shown);
        assert!(kept.start <= kept.end && kept.end <= text.len());
        assert!(text.is_char_boundary(kept.start) && text.is_char_boundary(kept.end));
        let left = text[kept.end..]
            .chars()
            .any(|c| !(c.is_control() || c.is_whitespace()));
        let ellipsis = if left { "…" } else { "" };
        assert_eq!(
            format!("{}{ellipsis}", clip1(&text[kept.clone()])),
            shown,
            "case {case}: {text:?} at {max}, kept {kept:?}"
        );
        let (said, at) = clip_kept(&text, max, &cuts(&known));
        whole(&text, &at, &known);
        for (a, b) in verbatim(&text, &known) {
            assert!(
                !(at.start <= a && b <= at.end && text[a..b].contains(blank)),
                "case {case}: {at:?} keeps a value the rewrite changes, of {text:?}"
            );
        }
        let left = text[at.end..].chars().any(|c| !blank(c));
        let ellipsis = if left { "…" } else { "" };
        assert_eq!(
            format!("{}{ellipsis}", clip1(&text[at.clone()])),
            said,
            "case {case}: {text:?} at {max}, kept {at:?}"
        );
        if verbatim(&text, &known).is_empty() {
            assert_eq!(said, shown, "case {case}: no value, the same line");
        }
    }
    assert_eq!(
        clip_kept(" a\u{0}\n[b] c ", 3, &none),
        ("a […".into(), 1..5)
    );
}
