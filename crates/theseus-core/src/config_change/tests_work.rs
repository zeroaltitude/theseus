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

/// `texts` as the owner's messages in `sid`'s transcript, in order: their node ids.
fn seed(r: &super::tests::Rig, sid: &str, texts: &[String]) -> Vec<String> {
    let nodes: Vec<crate::node::Node> = (texts.iter())
        .map(|t| crate::node::Node::user(sid, Some("t_seed"), "cli", t))
        .collect();
    let records: Vec<_> = nodes.iter().map(|n| n.record().unwrap()).collect();
    r.core.store.append(&records).unwrap();
    nodes.into_iter().map(|n| n.id).collect()
}

/// The look's result in `sid`: its node id and the words it shows.
fn look_result(r: &super::tests::Rig, sid: &str) -> (String, String) {
    (r.core.store.session_nodes(sid).unwrap().into_iter())
        .find_map(|(_, n)| match &n.body {
            crate::node::Body::ToolResult { content, tool, .. } if tool == crate::look::WORK => {
                Some((n.id.clone(), content.clone()))
            }
            _ => None,
        })
        .expect("the look's result")
}

/// Whether `node.reach` of `from` lists `copy` as its copy.
fn reaches(r: &super::tests::Rig, from: &str, copy: &str) -> bool {
    let walk = crate::reach::reach(&r.core.store, from, None)
        .unwrap()
        .unwrap();
    walk.descendants.iter().any(|d| d.node_id == copy)
}

/// The messages whose tag (`m07:`) the look's words hold: wholly (its whole text), in part, or not at all.
fn sorted(shown: &str, texts: &[String]) -> (Vec<usize>, Vec<usize>, Vec<usize>) {
    let tag = |t: &str| t.split(':').next().unwrap().to_string() + ":";
    let (mut whole, mut part, mut none) = (Vec::new(), Vec::new(), Vec::new());
    for (i, t) in texts.iter().enumerate() {
        match (shown.contains(t.as_str()), shown.contains(&tag(t))) {
            (true, _) => whole.push(i),
            (false, true) => part.push(i),
            (false, false) => none.push(i),
        }
    }
    (whole, part, none)
}

/// **Its cuts decide its copies** (D6 round 6). Twenty messages of 600 characters pass the history part's
/// 8,000: the part is cut, so the newest of them are not in the result at all. A message cut wholly out is
/// no copy (`node.reach` does not list the result); one cut in its words, and each kept whole, is.
#[tokio::test]
async fn a_message_its_cut_leaves_out_has_no_edge_and_one_it_keeps_part_of_has_one() {
    let r = rig();
    let sid = r.session();
    let texts: Vec<String> = (0..25)
        .map(|i| format!("m{i:02}:{}", "w".repeat(596)))
        .collect();
    let ids = seed(&r, &sid, &texts);
    r.turn_with(
        "look_work",
        &sid,
        json!({"what": ["history"], "messages": 20}),
    )
    .await;
    let (copy, shown) = look_result(&r, &sid);
    assert!(shown.contains("[cut at 8000 characters"), "{shown}");
    let (whole, part, none) = sorted(&shown, &texts);
    assert!(
        !whole.is_empty() && part.len() == 1 && none.len() > 5,
        "whole {whole:?}, part {part:?}, none {none:?}"
    );
    for i in none {
        assert!(
            !reaches(&r, &ids[i], &copy),
            "m{i:02} is not in the result, yet node.reach lists it"
        );
    }
    for i in whole.iter().chain(&part) {
        assert!(
            reaches(&r, &ids[*i], &copy),
            "m{i:02}'s words are in the result, yet node.reach does not list it"
        );
    }
}

/// **A runtime cut never loses a link; it can keep one to words it cut** (D6 round 7, the named limit). With
/// `[tools] result_max_chars` under the look's own size, the result is cut again, head and tail, after the tool
/// gave its sources. Every message the look's own answer showed stays linked: those whose words survive in the
/// head and in the tail (a lost one would be a copy a retraction misses), and those the runtime cut left out
/// (the limit: the list can include a message whose words were cut out; it never misses one).
#[tokio::test]
async fn a_runtime_cut_keeps_every_link_the_look_gave_even_to_words_it_cut() {
    let r = super::tests::rig_with(|cfg, _| cfg.tools.result_max_chars = 2_000);
    let sid = r.session();
    let texts: Vec<String> = (0..6)
        .map(|i| format!("m{i:02}:{}", "w".repeat(596)))
        .collect();
    let ids = seed(&r, &sid, &texts);
    r.turn_with(
        "look_work",
        &sid,
        json!({"what": ["history"], "messages": 7}),
    )
    .await;
    let (copy, shown) = look_result(&r, &sid);
    let cut = shown.find("not shown").expect("the runtime cap cut it");
    let at = |i: usize| shown.find(&texts[i][..4]);
    let head: Vec<usize> = (0..6).filter(|i| at(*i).is_some_and(|p| p < cut)).collect();
    let tail: Vec<usize> = (0..6).filter(|i| at(*i).is_some_and(|p| p > cut)).collect();
    let gone: Vec<usize> = (0..6).filter(|i| at(*i).is_none()).collect();
    assert!(
        !head.is_empty() && !tail.is_empty() && !gone.is_empty(),
        "head {head:?}, tail {tail:?}, cut out {gone:?}"
    );
    for i in head.iter().chain(&tail) {
        assert!(
            reaches(&r, &ids[*i], &copy),
            "m{i:02}'s words survive: its link is never lost"
        );
    }
    for i in gone {
        assert!(
            reaches(&r, &ids[i], &copy),
            "m{i:02}: the named limit, the link stays"
        );
    }
}

/// **A message whose words clip to nothing is no copy** (D6 round 6): control characters alone pass the
/// history part's test for a message, and its line shows no word of it.
#[tokio::test]
async fn a_message_of_control_characters_alone_has_no_edge() {
    let r = rig();
    let sid = r.session();
    let ids = seed(&r, &sid, &["\u{1}\u{7}".to_string()]);
    r.turn_with("look_work", &sid, json!({"what": ["history"]}))
        .await;
    let (copy, _) = look_result(&r, &sid);
    assert!(
        !reaches(&r, &ids[0], &copy),
        "a message with no word on the result is no copy"
    );
}

/// **A harness line the look shows links its node** (review section 5): the history part shows the harness's
/// own words, as the person's and Theseus's.
#[tokio::test]
async fn a_harness_line_it_shows_links_its_node() {
    let r = rig();
    let sid = r.session();
    let note = crate::node::Node::harness_body(
        "hrn",
        &sid,
        crate::turn::clock_step::CLOCK_AUTHOR,
        crate::node::Body::UserMessage {
            text: "[The harness: 9 minutes left in this turn]".into(),
            attachments: vec![],
        },
    );
    r.core.store.append(&[note.record().unwrap()]).unwrap();
    r.turn_with("look_work", &sid, json!({"what": ["history"]}))
        .await;
    let (copy, shown) = look_result(&r, &sid);
    assert!(
        shown.contains("the harness: [The harness: 9 minutes"),
        "{shown}"
    );
    assert!(reaches(&r, &note.id, &copy), "the harness line is a copy");
}

/// **A conversation that said a message and read it back by its look is told of its retraction once** (issue
/// 44). The look reads only this conversation, so the look's result is linked from the owner's message in
/// the conversation that said it; the look names no handle, so its own row is seen, and the conversation's
/// direct row is the one that acts: exactly one correction note, over two turns.
#[tokio::test]
async fn a_conversation_that_said_it_and_looked_back_is_told_once() {
    let r = rig();
    let sid = r.session();
    r.turn_with("look_work", &sid, json!({"what": ["history"]}))
        .await;
    let nodes = r.core.store.session_nodes(&sid).unwrap();
    let said = (nodes.iter())
        .find(|(_, n)| matches!(n.body, crate::node::Body::UserMessage { .. }))
        .map(|(_, n)| n.id.clone())
        .expect("the owner's message");
    let (copy, shown) = look_result(&r, &sid);
    assert!(shown.contains("- the owner: change a setting"), "{shown}");
    assert!(reaches(&r, &said, &copy), "the look's result is linked");
    let out = r
        .core
        .fact_retract(
            &theseus_protocol::retract::FactRetractParams {
                node: said,
                why: "the setting was another".into(),
                session_id: None,
            },
            "cli",
        )
        .unwrap();
    let acting = (out.exposures.iter())
        .filter(|e| {
            e.session_id == sid && e.kind == theseus_protocol::retract::exposure_kind::SESSION
        })
        .count();
    assert_eq!(acting, 1, "{out:#?}");
    r.run_turn(&sid).await;
    r.run_turn(&sid).await;
    let notes = (r.core.store.session_nodes(&sid).unwrap().into_iter())
        .filter(|(_, n)| n.author.as_deref() == Some(crate::retract::NOTE_AUTHOR))
        .count();
    assert_eq!(notes, 1);
}
