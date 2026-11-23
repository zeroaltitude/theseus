//! `look.work` played through the whole core, on the settings rig (issue 28): the owner's own turn
//! reads this conversation's tasks, what waits on the owner and its last messages, asked nothing;
//! it reads this conversation and no other; and a task's session is refused it.

use serde_json::json;

use super::tests::rig;
use crate::node::ResultStatus;

/// "What is going on?": every part, with no card and no shell; a daemon with nothing going on says so.
#[tokio::test]
async fn the_owner_reads_this_conversations_work_and_is_asked_nothing() {
    let r = rig();
    let sid = r.session();
    r.turn_with("look_work", &sid, json!({})).await;
    let (status, text) = r.result(&sid);
    assert_eq!(status, ResultStatus::Ok, "{text}");
    for part in [
        "Tasks: none have been started.",
        "Waiting on the owner: nothing waits anywhere.",
        "- the owner: change a setting",
    ] {
        assert!(text.contains(part), "{part} in {text}");
    }
    assert!(r.asked().is_empty(), "no card: {:?}", r.asked());
}

/// Only this conversation's messages: another session's turn is not in it, and its count is this
/// session's own. A part asked for alone is the only part, and `messages` bounds the history.
#[tokio::test]
async fn it_reads_this_conversation_and_no_other_and_one_part_alone() {
    let r = rig();
    let other = r.session();
    r.turn_with("look_work", &other, json!({"what": ["history"]}))
        .await;
    let sid = r.session();
    r.turn_with(
        "look_work",
        &sid,
        json!({"what": ["history"], "messages": 1}),
    )
    .await;
    let (status, text) = r.result(&sid);
    assert_eq!(status, ResultStatus::Ok, "{text}");
    assert!(
        text.contains("The last 1 of this conversation's 1 messages:"),
        "{text}"
    );
    assert!(!text.contains("Tasks"), "one part alone: {text}");
    assert!(!text.contains("Waiting"), "one part alone: {text}");
    // The other session's two messages (its question and its reply) are its own.
    let (_, theirs) = r.result(&other);
    assert!(
        theirs.contains("this conversation's 1 messages"),
        "{theirs}"
    );
}

/// A task's session is refused it, in the tool's own words, and reads nothing.
#[tokio::test]
async fn a_task_is_not_given_the_look() {
    let r = rig();
    let parent = r.session();
    let mut rec =
        crate::session::SessionRecord::new(theseus_protocol::SessionKind::Conversation, None);
    rec.task = Some(crate::session::TaskOf {
        parent_session: parent,
        parent_execution: "exe_x".into(),
        by: "tool_x".into(),
        target: None,
        arrangement: None,
        check: None,
        notify_done: false,
    });
    r.core.store.put_session(&rec.session_id, &rec).unwrap();
    r.turn_with("look_work", &rec.session_id, json!({})).await;
    let (status, text) = r.result(&rec.session_id);
    assert_eq!(status, ResultStatus::Error, "{text}");
    assert!(text.contains("not for a task"), "{text}");
}

/// What it will not take: another session or place to read, and a part that is not one.
#[tokio::test]
async fn it_takes_no_session_or_place_to_read() {
    let r = rig();
    for bad in [
        json!({"session": "ses_other"}),
        json!({"place": "discord:dm:1"}),
        json!({"what": ["vault"]}),
    ] {
        let sid = r.session();
        r.turn_with("look_work", &sid, bad.clone()).await;
        let (status, text) = r.result(&sid);
        assert_eq!(status, ResultStatus::Error, "{bad}: {text}");
    }
}

/// `theseus tasks` and `status` in the owner's shell are answered with it, and the answer is on the
/// ledger as a row of the verb, the tool and the place, with no command text. The answer's words say
/// what the tool gives (Theseus's work whole) and where it gives less, and never that it is "the same
/// look".
#[tokio::test]
async fn the_shell_commands_it_replaces_are_answered_with_it_and_counted() {
    for (line, verb) in [
        ("theseus tasks", "tasks"),
        ("theseus status --json", "status"),
    ] {
        let r = rig_in_its_dir();
        let sid = r.session();
        r.turn_with("proc_run", &sid, json!({"command": line}))
            .await;
        let (status, text) = r.result(&sid);
        assert_eq!(status, ResultStatus::Declined, "{line}: {text}");
        assert!(text.contains("`look_work`: use it"), "{text}");
        assert!(
            text.contains("Theseus's work") && text.contains("the conversations running"),
            "{text}"
        );
        assert!(!text.contains("same look"), "{text}");
        let rows = r.rows("shell.answered");
        assert_eq!(rows.len(), 1, "{rows:?}");
        assert_eq!(rows[0]["verb"], json!(verb));
        assert_eq!(rows[0]["tool"], json!("look.work"));
        assert_eq!(rows[0]["place"], json!("cli"));
        assert!(
            !rows[0].to_string().contains("tail"),
            "no command text: {}",
            rows[0]
        );
    }
    // `history`, bare or of a session, is another read (the most recently active session's whole
    // transcript, or that session's), which runs.
    for line in ["theseus history", "theseus history ses_other"] {
        let r = rig_in_its_dir();
        let sid = r.session();
        r.turn_with("proc_run", &sid, json!({"command": line}))
            .await;
        assert!(r.rows("shell.answered").is_empty(), "{line}");
    }
}

/// `theseus ontology` (bare) is answered with `look_theseus`, and the answer says the ontology comes only
/// when the call names it.
#[tokio::test]
async fn the_ontology_commands_answer_says_to_name_it() {
    let r = rig_in_its_dir();
    let sid = r.session();
    r.turn_with("proc_run", &sid, json!({"command": "theseus ontology"}))
        .await;
    let (status, text) = r.result(&sid);
    assert_eq!(status, ResultStatus::Declined, "{text}");
    assert!(
        text.contains("`look_theseus`: use it") && text.contains("only when the call names it"),
        "{text}"
    );
}

fn rig_in_its_dir() -> super::tests::Rig {
    super::tests::rig_with(|cfg, state| cfg.tools.cwd = Some(state.to_string_lossy().into_owned()))
}

/// Its history part copies this conversation's messages into its result, so `node.reach` of a message it
/// shows finds the result as its copy, by its own route, as a history read's is found (D6 round 5, 3).
#[tokio::test]
async fn its_history_part_is_found_from_each_message_it_shows() {
    let r = rig();
    let sid = r.session();
    r.turn_with("look_work", &sid, json!({"what": ["history"]}))
        .await;
    let nodes = r.core.store.session_nodes(&sid).unwrap();
    let said = (nodes.iter())
        .find(|(_, n)| matches!(n.body, crate::node::Body::UserMessage { .. }))
        .map(|(_, n)| n.id.clone())
        .expect("the owner's message");
    let copy = (nodes.iter())
        .find(|(_, n)| matches!(n.body, crate::node::Body::ToolResult { .. }))
        .map(|(_, n)| n.id.clone())
        .expect("the look's result");
    let walk = crate::reach::reach(&r.core.store, &said, None)
        .unwrap()
        .unwrap();
    let d = (walk.descendants.iter())
        .find(|d| d.node_id == copy)
        .unwrap_or_else(|| panic!("node.reach does not list the look's result: {walk:#?}"));
    assert_eq!(
        (d.generation, d.route.as_str()),
        (1, crate::graph::VIA_LOOK),
        "{d:?}"
    );
}
