use serde_json::json;

use super::*;

fn q(cid: &str, session: &str, asked: u64, may: Option<bool>) -> ConfirmRequest {
    let mut v = json!({
        "correlation_id": cid, "session_id": session, "execution_id": "exe_q7f3k2",
        "tool": "fs.write", "input": {}, "reason": "r", "by": "policy",
        "requested_at_ms": asked, "expires_at_ms": 0,
    });
    if let Some(m) = may {
        v["may_answer"] = json!(m);
    }
    serde_json::from_value(v).unwrap()
}

/// A link is read strictly: three paths and an id; anything else opens nothing.
#[test]
fn a_link_is_read_strictly() {
    assert_eq!(
        parse_link("theseus://answer/act_0c7e527d2a"),
        Some(Link::Answer("act_0c7e527d2a".into()))
    );
    assert_eq!(
        parse_link("theseus://history/ses_q7f3k2"),
        Some(Link::History("ses_q7f3k2".into()))
    );
    assert_eq!(
        parse_link("theseus://open/ses_q7f3k2"),
        Some(Link::Open("ses_q7f3k2".into()))
    );
    for bad in [
        "theseus://approve/act_1",
        "theseus://answer/",
        "theseus://answer/act_1/extra",
        "theseus://answer/act 1",
        "theseus://answer/act_1?y",
        "theseus://answer/act_1;rm",
        "theseus://answer/$(x)",
        "theseus://answer/../etc",
        "https://example.com/answer/act_1",
        "theseus:answer/act_1",
        "",
    ] {
        assert_eq!(parse_link(bad), None, "{bad}");
    }
    assert_eq!(
        parse_link(&format!("theseus://open/{}", "a".repeat(65))),
        None
    );
}

/// A link under a question opens a card or a view: no path of the three answers anything, and the
/// links are OSC 8 inside herdr and nothing elsewhere.
#[test]
fn links_appear_inside_herdr_only_and_never_answer() {
    let c = q("act_0c7e527d2a", "ses_q7f3k2", 1, None);
    assert_eq!(links_under(&c, None), None);
    assert_eq!(links_under(&c, Some("0")), None);
    assert_eq!(links_under(&c, Some("")), None);
    let line = links_under(&c, Some("1")).unwrap();
    assert_eq!(
        line,
        "\x1b]8;;theseus://answer/act_0c7e527d2a\x1b\\[answer]\x1b]8;;\x1b\\ \
         \x1b]8;;theseus://history/ses_q7f3k2\x1b\\[history]\x1b]8;;\x1b\\ \
         \x1b]8;;theseus://open/ses_q7f3k2\x1b\\[open]\x1b]8;;\x1b\\"
    );
    for (word, url) in card_links("act_0c7e527d2a", "ses_q7f3k2") {
        let parsed = parse_link(&url).unwrap();
        assert!(
            matches!(
                (word, parsed),
                ("answer", Link::Answer(_))
                    | ("history", Link::History(_))
                    | ("open", Link::Open(_))
            ),
            "{word}"
        );
        // No word is an approval.
        assert!(!url.contains("approve") && !url.contains("trust"), "{url}");
    }
}

/// A click on the answer link opens the answer card on that question; no link answers anything.
#[test]
fn a_click_on_answer_opens_the_card_and_answers_nothing() {
    let o = opens(Link::Answer("act_0c7e527d2a".into()), Some("w1:p2".into()));
    assert_eq!(
        o,
        Opens::Popup(
            "answer",
            PopupEnv {
                pane: Some("w1:p2".into()),
                cid: Some("act_0c7e527d2a".into()),
                session: None
            }
        )
    );
    assert_eq!(
        opens(Link::History("ses_q7f3k2".into()), None),
        Opens::Popup(
            "history",
            PopupEnv {
                session: Some("ses_q7f3k2".into()),
                ..PopupEnv::default()
            }
        )
    );
    assert_eq!(
        opens(Link::Open("ses_q7f3k2".into()), None),
        Opens::Focus("ses_q7f3k2".into())
    );
    // The popup's request is an open of a pane, with ids in its environment, and nothing else.
    let p = popup_params(
        "answer",
        &PopupEnv {
            pane: Some("w1:p2".into()),
            cid: Some("act_0c7e527d2a".into()),
            session: None,
        },
    );
    assert_eq!(p["plugin_id"], "theseus");
    assert_eq!(p["entrypoint"], "answer");
    assert_eq!(p["placement"], "popup");
    assert_eq!(p["env"]["THESEUS_CID"], "act_0c7e527d2a");
    assert_eq!(p["env"]["THESEUS_PANE"], "w1:p2");
    assert!(p["env"].get("THESEUS_SESSION_ARG").is_none());
    let text = p.to_string();
    assert!(
        !text.contains("work.answer") && !text.contains("approve"),
        "{text}"
    );
}

/// What reaches a popup's environment is an id, or nothing.
#[test]
fn only_ids_reach_a_popups_environment() {
    assert_eq!(clean_id("w1:p2"), Some("w1:p2".into()));
    assert_eq!(clean_id("act_0c7e527d2a"), Some("act_0c7e527d2a".into()));
    for bad in ["", "a b", "$(x)", "a;b", "a\nb", &"a".repeat(65)] {
        assert_eq!(clean_id(bad), None, "{bad:?}");
    }
}

/// `next` goes to the question that has waited longest among those this connection may answer, and
/// its pane is the one whose `$session` token names the asking session.
#[test]
fn next_is_the_task_that_has_waited_longest() {
    let qs = vec![
        q("act_0000bbbb22", "ses_bbbb22", 2_000, None),
        q("act_0000aaaa11", "ses_aaaa11", 1_000, Some(false)),
        q("act_0000cccc33", "ses_cccc33", 1_500, Some(true)),
    ];
    // The oldest cannot be answered here: the next oldest is the one.
    assert_eq!(
        longest_waiting(&qs).unwrap().correlation_id,
        "act_0000cccc33"
    );
    assert!(longest_waiting(&[]).is_none());
    let panes = vec![
        json!({"pane_id": "w1:p1", "tokens": {"session": "ses_bbbb22"}}),
        json!({"pane_id": "w1:p2", "tokens": {"session": "ses_cccc33"}}),
        json!({"pane_id": "w1:p3", "tokens": {}}),
    ];
    assert_eq!(pane_of(&panes, "ses_cccc33").unwrap()["pane_id"], "w1:p2");
    assert!(pane_of(&panes, "ses_none").is_none());
}

/// `stop` cancels a task by its short id and halts a conversation by its session; a pane that is
/// not Theseus's stops nothing.
#[test]
fn stop_is_cancel_for_a_task_and_halt_for_a_conversation() {
    assert_eq!(
        stop_for(&json!({"kind": "task", "session": "ses_a", "task": "a1b2c3"})),
        Some(Stop::Cancel("a1b2c3".into()))
    );
    assert_eq!(
        stop_for(&json!({"kind": "conversation", "session": "ses_a"})),
        Some(Stop::Halt("ses_a".into()))
    );
    assert_eq!(stop_for(&json!({"session": "ses_a"})), None);
    assert_eq!(
        stop_for(&json!({"kind": "other", "session": "ses_a"})),
        None
    );
    assert_eq!(stop_for(&json!({})), None);
}

/// `plugin` names the directory and the one command that links it.
#[test]
fn plugin_says_where_it_is_and_how_to_link_it() {
    let lines = plugin_lines(std::path::Path::new("/nowhere/bin/theseus"));
    assert!(lines[0].starts_with("plugin: "), "{lines:?}");
    assert!(lines[0].ends_with("contrib/herdr/theseus"), "{lines:?}");
    assert!(
        lines[1].starts_with("link it once: herdr plugin link "),
        "{lines:?}"
    );
    assert!(plugin_dir(std::path::Path::new("/nowhere"))
        .join("herdr-plugin.toml")
        .is_file());
}
