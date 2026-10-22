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

/// `stop` cancels a task by its session id and halts a conversation by its session; a pane that is
/// not Theseus's stops nothing.
#[test]
fn stop_is_cancel_for_a_task_and_halt_for_a_conversation() {
    assert_eq!(
        stop_for(&json!({"kind": "task", "session": "ses_a", "task": "[walkthrough] drift"})),
        Some(Stop::Cancel("ses_a".into()))
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
        .unwrap()
        .join("herdr-plugin.toml")
        .is_file());
}

/// A directory is named only when it holds the plugin: with none, the lines say where they looked.
#[test]
fn plugin_dir_names_only_a_directory_that_holds_the_plugin() {
    if let Some(dir) = plugin_dir(std::path::Path::new("/nowhere/bin/theseus")) {
        assert!(dir.join("herdr-plugin.toml").is_file(), "{dir:?}");
    }
    let candidates = plugin_candidates(std::path::Path::new("/nowhere/bin/theseus"));
    assert!(candidates.iter().any(|c| c.starts_with("/nowhere")));
    // The same lines, when no candidate holds the plugin.
    let lines = plugin_lines_for(None, &candidates);
    assert!(lines[0].contains("found no plugin directory"), "{lines:?}");
    assert!(
        lines[1].contains("/nowhere/bin/contrib/herdr/theseus"),
        "{lines:?}"
    );
    assert!(
        lines.iter().all(|l| !l.starts_with("link it once")),
        "{lines:?}"
    );
}

/// prefix+i reads the focused pane's task, and nothing else's.
#[test]
fn inspect_takes_a_tasks_session_and_no_other_pane() {
    let task = json!({"kind": "task", "session": "ses_task9f2", "task": "9f2"});
    assert_eq!(inspect_for(&task).as_deref(), Some("ses_task9f2"));
    assert_eq!(
        inspect_for(&json!({"kind": "conversation", "session": "ses_c"})),
        None
    );
    assert_eq!(inspect_for(&json!({"kind": "main"})), None);
    assert_eq!(
        inspect_for(&json!({"kind": "task", "session": "a b;rm"})),
        None
    );
}

fn task_view(session: &str, state: &str, paused: bool) -> WorkView {
    serde_json::from_value(json!({
        "id": "exe_t1", "kind": "task", "position": 9, "at_ms": 9, "root": "exe_main",
        "session_id": session, "title": "Sound the north channel", "state": state, "paused": paused,
        "attention": {"level": "working", "label": "turn 3", "since_ms": 5},
        "started_at_ms": 1, "state_since_ms": 5
    }))
    .unwrap()
}

/// prefix+p pauses a task that works and resumes one the board says is held, for the task the
/// pane shows and no other.
#[test]
fn pause_toggles_by_what_the_board_says_of_the_panes_task() {
    let pane = json!({"kind": "task", "session": "ses_task9f2", "task": "9f2"});
    let running = [
        task_view("ses_other", "running", true),
        task_view("ses_task9f2", "running", false),
    ];
    let held = [task_view("ses_task9f2", "waiting", true)];
    let go = hold_for(&pane, &running).unwrap();
    assert_eq!(
        (go.method, go.task.as_str(), go.name.as_str()),
        ("task.pause", "ses_task9f2", "Sound the north channel")
    );
    assert_eq!(hold_for(&pane, &held).unwrap().method, "task.resume");
}

/// A pane that shows no task, a task the board lacks and one that is over have nothing to hold.
#[test]
fn pause_holds_nothing_that_is_not_a_task_at_work() {
    let pane = json!({"kind": "task", "session": "ses_task9f2"});
    let words = |e: anyhow::Error| e.to_string();
    assert!(
        hold_for(&json!({"kind": "conversation", "session": "ses_c"}), &[])
            .map_err(words)
            .unwrap_err()
            .contains("does not show a task")
    );
    assert!(hold_for(&pane, &[task_view("ses_other", "running", false)])
        .map_err(words)
        .unwrap_err()
        .contains("no task ses_task9f2"));
    assert!(hold_for(&pane, &[task_view("ses_task9f2", "done", false)])
        .map_err(words)
        .unwrap_err()
        .contains("is over"));
}
