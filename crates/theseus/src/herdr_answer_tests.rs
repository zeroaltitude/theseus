use serde_json::json;

use super::*;

fn q(cid: &str, session: &str, asked: u64, extra: Value) -> ConfirmRequest {
    let mut v = json!({
        "correlation_id": cid, "session_id": session, "execution_id": "exe_q7f3k2",
        "tool": "fs.write", "input": {"path": "log.md"}, "reason": "write under /tide",
        "by": "policy", "requested_at_ms": asked, "expires_at_ms": 0,
    });
    for (k, x) in extra.as_object().cloned().unwrap_or_default() {
        v[k] = x;
    }
    serde_json::from_value(v).unwrap()
}

fn three() -> Vec<ConfirmRequest> {
    vec![
        q("act_0000bbbb22", "ses_aaaa11", 2_000, json!({})),
        q("act_0000aaaa11", "ses_bbbb22", 1_000, json!({})),
        q("act_0000cccc33", "ses_cccc33", 3_000, json!({})),
    ]
}

/// Which question the card opens on: the named one, else the focused pane's, else the one that has
/// waited longest, else none.
#[test]
fn the_card_opens_on_the_named_then_the_focused_then_the_oldest() {
    let qs = three();
    let want = |cid: Option<&str>, focused: Option<&str>| Want {
        cid: cid.map(Into::into),
        focused: focused.map(Into::into),
    };
    // Nothing named, nothing focused: the oldest (asked at 1,000).
    assert_eq!(pick(&qs, &want(None, None)), Pick::Found(1));
    // The focused pane's question, whole or by its end.
    assert_eq!(
        pick(&qs, &want(None, Some("act_0000cccc33"))),
        Pick::Found(2)
    );
    assert_eq!(pick(&qs, &want(None, Some("cc33"))), Pick::Found(2));
    // A focused token that names nothing (answered meanwhile) falls to the oldest.
    assert_eq!(pick(&qs, &want(None, Some("act_gone0000"))), Pick::Found(1));
    // A named question beats the focused one.
    assert_eq!(
        pick(&qs, &want(Some("bbbb22"), Some("cc33"))),
        Pick::Found(0)
    );
    // A name that matches nothing is gone, and never another question.
    assert_eq!(
        pick(&qs, &want(Some("zzzz99"), None)),
        Pick::Gone("zzzz99".into())
    );
    // An end shorter than four characters names nothing.
    assert_eq!(pick(&qs, &want(Some("22"), None)), Pick::Gone("22".into()));
    assert_eq!(pick(&[], &want(None, None)), Pick::Nothing);
    assert_eq!(queue(&qs), vec![1, 0, 2]);
}

/// The keys come from the question's own options; a daemon that sends none gets the three a call has.
#[test]
fn the_keys_are_the_questions_own() {
    let call = q("act_1", "ses_a", 1, json!({}));
    let ks: Vec<(char, String)> = keys(&call).into_iter().map(|k| (k.key, k.option)).collect();
    assert_eq!(ks, [('y', "approve".into()), ('n', "decline".into())]);
    let ask = q(
        "act_3",
        "ses_a",
        1,
        json!({"options": [
            {"id": "choice:0", "label": "North quay", "style": "plain", "takes_note": false, "effect": ""},
            {"id": "choice:1", "label": "South quay", "style": "plain", "takes_note": false, "effect": ""},
            {"id": "decline", "label": "Decline", "style": "plain", "takes_note": true, "effect": ""}]}),
    );
    let ks: Vec<(char, String)> = keys(&ask).into_iter().map(|k| (k.key, k.option)).collect();
    assert_eq!(
        ks,
        [
            ('1', "choice:0".into()),
            ('2', "choice:1".into()),
            ('n', "decline".into())
        ]
    );
}

/// A line is a key, a key and a note, the next question, a close, or a decline with its words; an
/// empty line answers nothing.
#[test]
fn a_line_is_an_answer_a_note_or_nothing() {
    let call = q("act_2", "ses_a", 1, json!({}));
    let ks = keys(&call);
    let ans = |option: &str, note: Option<&str>| Decision::Answer {
        option: option.into(),
        note: note.map(Into::into),
    };
    assert_eq!(decide("y", &ks), ans("approve", None));
    assert_eq!(decide(" Y \n", &ks), ans("approve", None));
    assert_eq!(decide("n", &ks), ans("decline", None));
    assert_eq!(
        decide("n not today", &ks),
        ans("decline", Some("not today"))
    );
    // The note path: other words decline with themselves as the note.
    assert_eq!(
        decide("use the staging port", &ks),
        ans("decline", Some("use the staging port"))
    );
    assert_eq!(decide("tab", &ks), Decision::Next);
    assert_eq!(decide("\t", &ks), Decision::Next);
    assert_eq!(decide("q", &ks), Decision::Quit);
    assert_eq!(decide("   ", &ks), Decision::Nothing);
    assert_eq!(decide("", &ks), Decision::Nothing);
    // A key the question does not have is a note, not an answer: `t` on a plain call declines.
    let plain = keys(&q("act_1", "ses_a", 1, json!({})));
    assert_eq!(decide("t", &plain), ans("decline", Some("t")));
}

/// The answer goes out through `work.answer` with author `herdr`.
#[test]
fn the_answer_carries_the_author_herdr() {
    let p = answer_params("act_0000aaaa11", "decline", Some("not now".into()));
    let v = serde_json::to_value(&p).unwrap();
    assert_eq!(
        v,
        json!({"question_id": "act_0000aaaa11", "option": "decline", "note": "not now",
               "author": "herdr"})
    );
    assert_eq!(AUTHOR, "herdr");
}

/// The card: the task's title and short id, where it came from, the model and the spend against its
/// limit, when it was asked and expires, the call, why, and its input; the keys; the others.
#[test]
fn the_card_says_the_question_whole() {
    theseus_client::render::time::pin_for_tests();
    let c = q(
        "act_0000aaaa11",
        "ses_q7f3k2",
        1_000_000,
        json!({"task": {"task_id": "tsk_a1b2c3d4", "short": "a1b2c3", "title": "Survey the north quay"},
               "expires_at_ms": 1_000_000 + 240_000}),
    );
    let session: SessionInfo = serde_json::from_value(json!({
        "session_id": "ses_q7f3k2", "kind": "task", "created_at_unix_ms": 0, "turns": 3,
        "usage": {"input_tokens": 0, "output_tokens": 0}, "last_active_ms": 0, "cost_usd": 0.42, "tool_calls": 2,
        "model": "glm-5", "limit_usd": 10.0, "pending_confirms": 1,
    }))
    .unwrap_or_else(|e| panic!("{e}"));
    let ctx = Context {
        session: Some(session),
        parent: Some("Tide notes".into()),
        others: vec!["d4e5f6 proc.run".into(), "ses_x fs.read".into()],
    };
    let lines = card(&c, &ctx, 1_000_000 + 180_000);
    assert_eq!(lines[0], "Survey the north quay  ·  a1b2c3");
    assert_eq!(
        lines[1],
        "from Tide notes · glm-5 · $0.42 of $10.00 · asked 3 min ago · expires in 60 s · at 17:20"
            .replace(
                "17:20",
                &theseus_client::render::time::fmt_hm(1_000_000 + 240_000)
            )
    );
    let text = lines.join("\n");
    assert!(text.contains("? fs.write waits for you"), "{text}");
    assert!(text.contains("why: write under /tide"), "{text}");
    assert!(text.contains(r#"input: {"path":"log.md"}"#), "{text}");
    assert!(text.contains("[y] approve  [n] decline"), "{text}");
    assert!(text.contains("any other words decline"), "{text}");
    assert!(
        text.ends_with("2 more wait: d4e5f6 proc.run, ses_x fs.read [tab] next"),
        "{text}"
    );
}

/// Esc closes the card; an Esc among other keys is neither an answer nor a decline.
#[test]
fn esc_closes_the_answer_card() {
    let ks = keys(&q("act_2", "ses_a", 1, json!({})));
    assert_eq!(decide("\x1b\n", &ks), Decision::Quit);
    assert_eq!(decide("\x1b\x1b \r\n", &ks), Decision::Quit);
    assert_eq!(decide("\x1by\n", &ks), Decision::Unknown("y".into()));
    assert_eq!(
        decide("note \x1b[A", &ks),
        Decision::Unknown("note [A".into())
    );
}

/// The card lists the other questions in the order `tab` walks them, the next one first.
#[test]
fn the_listed_waiting_order_is_the_tab_order() {
    let qs = three();
    let ids = |shown: usize| -> Vec<String> {
        others_after(&qs, &qs[shown])
            .iter()
            .map(|q| q.correlation_id.clone())
            .collect()
    };
    // Raw order would list aaaa11 before cccc33 here; tab goes to cccc33.
    assert_eq!(ids(0), ["act_0000cccc33", "act_0000aaaa11"]);
    assert_eq!(ids(1), ["act_0000bbbb22", "act_0000cccc33"]);
    assert_eq!(ids(2), ["act_0000aaaa11", "act_0000bbbb22"]);
    assert!(others_after(&qs[..1], &qs[0]).is_empty());
}
