//! The look's cuts stop at a known value's edge: a message's line, a task's title and why it ended, the end
//! of an id in a place's name, each part and the whole answer, and a played turn's stored result and the
//! request that carries it. "No part of
//! it" means no four characters in a row of the value outside a whole copy (`tests_cut_sites`). All values
//! invented.

use serde_json::json;

use super::tasks::task_line;
use super::*;
use crate::node::{Body, Node, ResultStatus};
use crate::tests_cut_sites::whole_or_absent;
use crate::view::text::kept::sample::{cuts as knowing, BROKEN, VALUE};

/// A person's message, a reply and a subagent's answer, each saying `text`.
fn messages(text: &str) -> [Node; 3] {
    let reply = Node::assistant(
        "ses_look",
        "trn_1",
        0,
        Body::AssistantMessage {
            blocks: vec![json!({"type": "text", "text": text})],
            model: "m".into(),
            provider: "p".into(),
            stop_reason: None,
            usage: Default::default(),
            cost_usd: None,
            catalog_version: None,
            request_id: None,
            correlation_id: None,
            compilation_id: None,
            request_digest: None,
            effort: None,
        },
    );
    let answer = theseus_protocol::SubagentAnswer {
        profile: "scout".into(),
        model: "m".into(),
        text: text.into(),
        ..Default::default()
    };
    let subagent = Node::tool_result(
        "ses_look",
        Some("trn_1"),
        Some(0),
        Body::ToolResult {
            tool_use_id: "tu_1".into(),
            tool: "profile.ask".into(),
            status: ResultStatus::Ok,
            is_error: false,
            content: String::new(),
            correlation_id: None,
            bytes_total: 0,
            truncated: false,
            full_ref: None,
            duration_ms: None,
            late: false,
            meta: json!({ "subagent": answer }),
            image: None,
            external: None,
            payload: None,
        },
    );
    [
        Node::user("ses_look", Some("trn_1"), "cli", text),
        reply,
        subagent,
    ]
}

/// A message's line (a person's, a reply's, a subagent's answer, each 600 characters): a known value across
/// the cap leaves none of it, the line stopping before it; a two-line value inside a shorter text leaves
/// neither half; one at the start leaves an ellipsis alone, and the message shows none of its words.
#[test]
fn a_messages_line_stops_at_a_known_values_edge() {
    let cuts = knowing(&[VALUE, BROKEN]);
    let across = format!("{}{VALUE} after", "x".repeat(590));
    for n in messages(&across) {
        let (_, words, some) = history::line_of(&n, &cuts).unwrap();
        whole_or_absent("a message's line", &words, VALUE);
        assert_eq!(words, format!("{}…", "x".repeat(590)));
        assert!(some);
    }
    for n in messages(&format!("chart {BROKEN} reef")) {
        let (_, words, some) = history::line_of(&n, &cuts).unwrap();
        whole_or_absent("a message's line", &words, BROKEN);
        assert_eq!(words, "chart…");
        assert!(some);
    }
    for n in messages(&format!("{BROKEN} reef")) {
        let (_, words, some) = history::line_of(&n, &cuts).unwrap();
        assert_eq!(words, "…");
        assert!(!some, "none of its words is shown");
    }
    // A text with no known value in it reads as it did.
    for n in messages("the  tide\nturns") {
        let (_, words, some) = history::line_of(&n, &cuts).unwrap();
        assert_eq!(words, "the tide turns");
        assert!(some);
    }
}

/// A task's title (100) and why it ended (120): a known value across either cap leaves none of it, and a
/// two-line value inside a shorter text leaves neither half.
#[test]
fn a_tasks_title_and_why_it_ended_stop_at_a_known_values_edge() {
    let cuts = knowing(&[VALUE, BROKEN]);
    let mut t = theseus_protocol::TaskInfo {
        short: "a1b2c3".into(),
        title: Some(format!("{}{VALUE} then report", "x".repeat(90))),
        state: "failed".into(),
        ended_reason: Some(format!("{}{VALUE} was refused", "y".repeat(110))),
        ..Default::default()
    };
    let line = task_line(&t, &cuts);
    whole_or_absent("a task's line", &line, VALUE);
    assert!(
        line.contains(&format!(" · {}… · ", "x".repeat(90))),
        "{line}"
    );
    assert!(
        line.ends_with(&format!(" · {}…\n", "y".repeat(110))),
        "{line}"
    );
    t.title = Some(format!("chart {BROKEN} reef"));
    t.ended_reason = Some(format!("stopped at {BROKEN} here"));
    let line = task_line(&t, &cuts);
    whole_or_absent("a task's line", &line, BROKEN);
    assert!(line.contains(" · chart… · "), "{line}");
    assert!(line.ends_with(" · stopped at…\n"), "{line}");
}

/// A part's cut at 8,000 characters and the answer's at 24,000 would each fall inside a known value: each
/// stops before it, and a message whose words begin where a cut now stops is no copy; one before it still is.
#[test]
fn each_cut_of_the_answer_stops_at_a_known_values_edge() {
    let cuts = knowing(&[VALUE]);
    let first = format!("{}{VALUE}{}", "a".repeat(7_990), "b".repeat(100));
    // The first three parts take 23,856 characters (23,866 had the first part's cut fallen at 8,000), so the
    // answer's cut falls inside the value at 125 characters into the last.
    let last = format!("{}{VALUE}{}", "f".repeat(125), "g".repeat(100));
    let (said, shown) = assemble(
        vec![
            (
                first,
                vec![
                    ("msg_a".to_string(), 0, 7_990),
                    ("msg_value".to_string(), 7_990, 8_017),
                ],
            ),
            ("c".repeat(7_900), vec![("msg_c".to_string(), 0, 50)]),
            ("d".repeat(7_900), vec![]),
            (last, vec![("msg_last".to_string(), 125, 152)]),
        ],
        &cuts,
    );
    assert!(said.contains("[cut at 8000 characters"), "the part is cut");
    assert!(
        said.contains("[cut at 24000 characters"),
        "the answer is cut"
    );
    whole_or_absent("the answer", &said, VALUE);
    assert!(
        said.starts_with(&format!("{}…\n[cut at 8000", "a".repeat(7_990))),
        "the part stops before the value"
    );
    let ids: Vec<&str> = shown.iter().map(|(id, _, _)| id.as_str()).collect();
    assert_eq!(ids, ["msg_a", "msg_c"], "{shown:?}");
    assert_eq!(shown[0], ("msg_a".to_string(), 0, 7_990));
}

/// Through a played turn: the owner's message holds a known value across the look's 600-character cut, the
/// model calls the work look, and the result stored for that call, and the request then sent with it, hold no
/// part of the value; the line stops before it.
#[tokio::test]
async fn a_played_looks_stored_result_holds_no_part_of_a_known_value() {
    use crate::provider::Scripted;
    use crate::tests_cut_sites::{knowing_core, tool_results, V};
    use crate::tests_places::{answers_a_call, result_of, session, turn, OWNER};
    let dir = crate::scratch::tempdir();
    let file = dir.path().join("theseus.toml");
    std::fs::write(&file, "").unwrap();
    let r = knowing_core(
        V,
        |c| c.config_file = Some(file.clone()),
        |req| match answers_a_call(req) {
            true => Scripted::text("Done."),
            false => Scripted::tools("", &[("t_look", "look_work", json!({"what": ["history"]}))]),
        },
    );
    let sid = session(&r.core, Some(&format!("dm:{OWNER}")));
    turn(&r.core, &sid, &format!("{}{V} and more", "x".repeat(595))).await;
    let stored = result_of(&r.core, &sid, "t_look");
    assert!(
        stored.contains(&format!("- the owner: {}…\n", "x".repeat(595))),
        "{stored}"
    );
    whole_or_absent("the stored result", &stored, V);
    let sent: Vec<_> = r.requests().into_iter().filter(answers_a_call).collect();
    assert!(!sent.is_empty(), "the result was sent");
    for q in &sent {
        for t in tool_results(q) {
            whole_or_absent("a result sent", &t, V);
        }
        let whole = serde_json::to_string(&q.messages).unwrap();
        whole_or_absent("the request sent", &whole, V);
    }
}

/// A message whose words run on past where a part's cut now stops is copied up to that edge, not up to 8,000.
#[test]
fn a_copied_span_ends_where_the_cut_fell() {
    let cuts = knowing(&[VALUE]);
    let part = format!("{}{VALUE}{}", "a".repeat(7_990), "b".repeat(100));
    let spans = vec![("msg_a".to_string(), 7_000, 8_090)];
    let (said, shown) = assemble(vec![(part, spans)], &cuts);
    whole_or_absent("the answer", &said, VALUE);
    assert_eq!(shown, [("msg_a".to_string(), 7_000, 7_990)]);
}

/// What a place's name ends in when it is the CLI's: the session's id, then these words.
const CLI: &str = " on the CLI or the web UI";

/// A place's name that ends as the CLI's does: the end of the id it shows starts after a known value its start
/// would fall inside, and ends before one that runs over where those words were taken off, both found on the
/// whole name; when nothing is left the brackets hold an ellipsis alone. A name with no known value reads as it
/// did, and a session's own id shows its last six characters.
#[test]
fn a_place_names_tail_stops_at_a_known_values_edge() {
    use super::reader::place_words;
    let cuts = knowing(&[VALUE]);
    let words = place_words(&format!("#{VALUE}{CLI}"), &cuts);
    whole_or_absent("a place's name", &words, VALUE);
    assert_eq!(words, "the CLI or the web UI (…)");
    // The tail would start inside the value: it starts at the value's end.
    assert_eq!(
        place_words(&format!("#{VALUE}xyz{CLI}"), &cuts),
        "the CLI or the web UI (…xyz)"
    );
    // A value that runs on into those words: the tail ends where the value starts.
    let across = knowing(&["kq81 on the CLI"]);
    assert_eq!(
        place_words(&format!("ses_0123456789abkq81{CLI}"), &across),
        "the CLI or the web UI (…ab)"
    );
    // No known value in the name: as it reads with no value known.
    for name in [
        format!("ses_0123456789abcdef{CLI}"),
        format!("#harbour{CLI}"),
        format!("ab{CLI}"),
        "#harbour".to_string(),
    ] {
        let none = crate::scrub::Cuts::none();
        assert_eq!(place_words(&name, &cuts), place_words(&name, &none), "{name}");
    }
    assert_eq!(
        place_words(&format!("ses_0123456789abcdef{CLI}"), &cuts),
        "the CLI or the web UI (…abcdef)"
    );
    // A session's own id, as a session no place runs on is named.
    let sid = crate::session::SessionRecord::new(theseus_protocol::SessionKind::Conversation, None)
        .session_id;
    assert_eq!(
        place_words(&crate::recall::render::unplaced(&sid), &cuts),
        format!("the CLI or the web UI (…{})", crate::task::short(&sid))
    );
}

/// Through played turns: a conversation in a place whose bound name is a known value and then the CLI's words
/// sets a reminder; the owner's look at what is coming says where it was set, and the result stored for that
/// call holds no part of the value.
#[tokio::test]
async fn a_played_looks_stored_place_holds_no_part_of_a_known_value() {
    use crate::provider::Scripted;
    use crate::tests_cut_sites::{knowing_core, V};
    use crate::tests_places::{answers_a_call, last_user, result_of, session, turn, LAB, OWNER};
    let dir = crate::scratch::tempdir();
    let file = dir.path().join("theseus.toml");
    std::fs::write(&file, "").unwrap();
    let r = knowing_core(
        V,
        |c| c.config_file = Some(file.clone()),
        |req| match (answers_a_call(req), last_user(req).contains("remind me")) {
            (true, _) => Scripted::text("Done."),
            (false, true) => Scripted::tools(
                "",
                &[(
                    "t_wake",
                    "wake_at",
                    json!({"after": "2h", "note": "water the ferns", "for": "owner", "say_only": true}),
                )],
            ),
            (false, false) => {
                Scripted::tools("", &[("t_look", "look_work", json!({"what": ["coming"]}))])
            }
        },
    );
    r.core.bind_places(vec![crate::places::BoundPlace {
        target: format!("discord:channel:{LAB}"),
        name: format!("#{V}{CLI}"),
        private: true,
        ..Default::default()
    }]);
    let there = session(&r.core, Some(&format!("channel:{LAB}")));
    turn(&r.core, &there, "remind me to water the ferns").await;
    let here = session(&r.core, Some(&format!("dm:{OWNER}")));
    turn(&r.core, &here, "what is coming").await;
    let stored = result_of(&r.core, &here, "t_look");
    assert!(
        stored.contains(" · in the CLI or the web UI (…) · water the ferns\n"),
        "{stored}"
    );
    whole_or_absent("the stored result", &stored, V);
}
