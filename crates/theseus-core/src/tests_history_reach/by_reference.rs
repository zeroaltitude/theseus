//! A node is shown with what has been said about it since. A node's words do not change once written (erase
//! is the one exception), and what is said about it later is a record of its own that points at it. So a
//! conversation that read a node's words by a look back (`context.history`, `look.work`) is told of its
//! retraction once, as a conversation that laid it any other way is.

use theseus_protocol::retract::FactRetractResult;
use theseus_protocol::SessionKind;

use super::*;
use crate::graph::{Edge, EdgeKind, VIA_HISTORY, VIA_LOOK};
use crate::session::{SessionRecord, TaskOf};

/// Why the line said is retracted.
const WHY: &str = "the heron nests by the south weir";

fn retract(core: &Core, node: &str) -> FactRetractResult {
    core.fact_retract(
        &FactRetractParams {
            node: node.into(),
            why: WHY.into(),
            session_id: None,
        },
        "cli",
    )
    .unwrap()
}

/// The correction notes stored in `sid`.
fn notes_in(core: &Core, sid: &str) -> usize {
    (core.store.session_nodes(sid).unwrap().into_iter())
        .filter(|(_, n)| n.author.as_deref() == Some(crate::retract::NOTE_AUTHOR))
        .count()
}

/// A new session (a task's, when `task`) holding a result of `tool` that showed `of`'s words, written by hand
/// as a completion frame writes it (the result, and its edge by the tool's route): never run. Its id and the
/// result's.
fn read_by_hand(core: &Core, task: bool, (tool, via): (&str, &str), of: &str) -> (String, String) {
    let mut rec = SessionRecord::new(
        if task {
            SessionKind::Task
        } else {
            SessionKind::Conversation
        },
        None,
    );
    if task {
        rec.title = Some("count the herons".into());
        rec.task = Some(TaskOf {
            parent_session: session(core, None),
            parent_execution: "exe_none".into(),
            by: "call_none".into(),
            target: None,
            arrangement: None,
            check: None,
            notify_done: false,
        });
    }
    core.store.put_session(&rec.session_id, &rec).unwrap();
    let mut n = crate::compiler::tests_view::result("turn_read", "h9", SAID);
    n.session_id = rec.session_id.clone();
    if let Body::ToolResult { tool: t, .. } = &mut n.body {
        *t = tool.into();
    }
    let e = Edge::new(EdgeKind::DerivedFrom, &n.id, of, via);
    core.store
        .append(&[n.record().unwrap(), e.record().unwrap()])
        .unwrap();
    (rec.session_id, n.id)
}

/// The exposure `out` lists for `sid`.
fn of_session<'a>(
    out: &'a FactRetractResult,
    sid: &str,
) -> &'a theseus_protocol::retract::RetractExposure {
    (out.exposures.iter())
        .find(|e| e.session_id == sid)
        .unwrap_or_else(|| panic!("the retraction does not list {sid}: {out:#?}"))
}

/// A conversation that read the line through the history tool is told: its exposure is a session's, its
/// next turn holds one correction note, and a second retraction writes none.
#[tokio::test]
async fn a_retraction_tells_the_conversation_that_read_it_by_history() {
    let r = rig();
    let said = say(&r.core).await;
    let other = session(&r.core, Some("channel:lab"));
    let (copy, _) = read_in(&r.core, &other, json!({ "open": said.turn })).await;
    let out = retract(&r.core, &said.node);
    let e = of_session(&out, &other);
    assert_eq!(
        (e.kind.as_str(), e.node_id.as_str()),
        (kind::SESSION, copy.as_str()),
        "{e:?}"
    );
    assert!(e.what.contains("history"), "{e:?}");
    turn(&r.core, &other, "and the weir?").await;
    assert_eq!(notes_in(&r.core, &other), 1);
    let last = r.requests().pop().expect("a request");
    let sent = serde_json::to_string(&last.messages).unwrap();
    assert!(sent.contains(WHY), "the note is in the request: {sent}");
    assert!(retract(&r.core, &said.node).already);
    turn(&r.core, &other, "and at dawn?").await;
    assert_eq!(notes_in(&r.core, &other), 1, "told once");
}

/// The same for a result of `look.work`: its conversation is a session the retraction tells.
#[tokio::test]
async fn a_retraction_tells_the_conversation_that_read_it_by_look() {
    let r = rig();
    let said = say(&r.core).await;
    let (reader, copy) = read_by_hand(&r.core, false, (crate::look::WORK, VIA_LOOK), &said.node);
    let out = retract(&r.core, &said.node);
    let e = of_session(&out, &reader);
    assert_eq!(
        (e.kind.as_str(), e.node_id.as_str()),
        (kind::SESSION, copy.as_str()),
        "{e:?}"
    );
    turn(&r.core, &reader, "and the weir?").await;
    assert_eq!(notes_in(&r.core, &reader), 1);
}

/// A task that read the line through the history tool is named on the card, as a task that used it any
/// other way is; it gets no note.
#[tokio::test]
async fn a_task_that_read_it_by_history_is_named_on_the_card() {
    let r = rig();
    let said = say(&r.core).await;
    let tool = (crate::view::history::HISTORY_TOOL, VIA_HISTORY);
    let (task, copy) = read_by_hand(&r.core, true, tool, &said.node);
    let out = retract(&r.core, &said.node);
    let e = of_session(&out, &task);
    assert_eq!(
        (e.kind.as_str(), e.node_id.as_str()),
        (kind::TASK, copy.as_str()),
        "{e:?}"
    );
    assert!(
        !out.exposures.iter().any(|e| e.kind == kind::SEEN),
        "{out:#?}"
    );
}

/// What `memory.search` admits for the line, over `sessions`' nodes.
async fn recalled(
    core: &Arc<Core>,
    sessions: Vec<String>,
) -> theseus_protocol::memory::RecallManifest {
    core.runner
        .memory
        .set_ask(crate::tests_recall::index_of(core, sessions));
    core.memory_search(MemorySearchParams {
        query: "zircon heron weir".into(),
        session_id: None,
        k: None,
        arm: None,
        as_of: None,
    })
    .await
    .unwrap()
}

/// Recall never serves a look back's result as its own words: before a retraction the original is served
/// and the result is not; after it, neither.
#[tokio::test]
async fn recall_never_serves_a_history_result_as_its_own_words() {
    let r = rig();
    let said = say(&r.core).await;
    let other = session(&r.core, Some("channel:lab"));
    let (copy, _) = read_in(&r.core, &other, json!({ "open": said.turn })).await;
    let both = vec![said.session.clone(), other.clone()];
    let m = recalled(&r.core, both.clone()).await;
    assert!(m.admitted.iter().any(|i| i.node_id == said.node), "{m:?}");
    assert!(!m.admitted.iter().any(|i| i.node_id == copy), "{m:?}");
    retract(&r.core, &said.node);
    let m = recalled(&r.core, both).await;
    assert!(!m.admitted.iter().any(|i| i.node_id == said.node), "{m:?}");
    assert!(!m.admitted.iter().any(|i| i.node_id == copy), "{m:?}");
}

/// An erased import's words are not recalled through a history result that showed them.
#[tokio::test]
async fn an_erased_import_is_not_recalled_through_a_history_result() {
    use crate::tests_imports::{episode, import};
    let r = rig();
    let ids = import(
        &r.core,
        &[episode(7, "personal", "2026-03-03T18:00:00Z", SAID)],
    );
    let other = session(&r.core, None);
    let (copy, text) = read_in(&r.core, &other, json!({ "open": ids[0] })).await;
    assert!(text.contains(WORD), "{text}");
    crate::import::write::erase(&r.core.store, "kelp-2026", Some("a test"), "cli").unwrap();
    let m = recalled(&r.core, vec![ids[0].clone(), other.clone()]).await;
    assert!(!m.admitted.iter().any(|i| i.node_id == copy), "{m:?}");
    // Nothing of the reader's holds the words. The import's own nodes are not this test's: the stand-in
    // index keeps every record of a session, where the real one drops a node once it is erased.
    let shown = |i: &theseus_protocol::memory::RecallItem| {
        i.session_id == other && i.text.as_deref().is_some_and(|t| t.contains(WORD))
    };
    assert!(!m.admitted.iter().any(shown), "{m:?}");
}

/// The message of the request the model last got that holds the result of call `h1`, and that block.
fn laid_result(r: &Rig) -> (Value, Value) {
    let last = r.requests().pop().expect("a request");
    for m in &last.messages {
        let blocks = m["content"].as_array().cloned().unwrap_or_default();
        if let Some(b) = blocks
            .into_iter()
            .find(|b| b["type"] == "tool_result" && b["tool_use_id"] == "h1")
        {
            return (m.clone(), b);
        }
    }
    panic!("no result of h1 is laid: {:#?}", last.messages)
}

/// The next request of the conversation that read the line lays its history result with what was said
/// since: its words as they were, and the line after them.
#[tokio::test]
async fn a_window_lays_a_history_result_with_the_correction_since() {
    let r = rig();
    let said = say(&r.core).await;
    let other = session(&r.core, Some("channel:lab"));
    read_in(&r.core, &other, json!({ "open": said.turn })).await;
    turn(&r.core, &other, "and the tide?").await;
    let (_, before) = laid_result(&r);
    assert!(!before.to_string().contains("Since this was"), "{before}");
    retract(&r.core, &said.node);
    turn(&r.core, &other, "and the weir?").await;
    let (_, after) = laid_result(&r);
    let text = after.to_string();
    assert!(text.contains(WORD), "{text}");
    assert!(text.contains("[Since this was read: "), "{text}");
    assert!(text.contains(WHY), "{text}");
}

/// Two turns with no correction between them lay the read exchange in the same bytes, the line included.
#[tokio::test]
async fn two_turns_with_nothing_said_between_lay_the_same_bytes() {
    let r = rig();
    let said = say(&r.core).await;
    let other = session(&r.core, Some("channel:lab"));
    read_in(&r.core, &other, json!({ "open": said.turn })).await;
    retract(&r.core, &said.node);
    turn(&r.core, &other, "and the weir?").await;
    let first = laid_result(&r);
    turn(&r.core, &other, "and at dawn?").await;
    let second = laid_result(&r);
    assert!(first.1.to_string().contains("[Since this was read: "));
    assert_eq!(first, second);
}

/// An import erased after a history result showed it: the result is laid as one line, none of its words.
#[tokio::test]
async fn a_window_lays_no_words_of_an_erased_source() {
    use crate::tests_imports::{episode, import};
    let r = rig();
    let ids = import(
        &r.core,
        &[episode(7, "personal", "2026-03-03T18:00:00Z", SAID)],
    );
    let other = session(&r.core, None);
    read_in(&r.core, &other, json!({ "open": ids[0] })).await;
    crate::import::write::erase(&r.core.store, "kelp-2026", Some("a test"), "cli").unwrap();
    turn(&r.core, &other, "and the weir?").await;
    let (_, laid) = laid_result(&r);
    assert!(
        laid.to_string()
            .contains(crate::copies::since::ERASED_RESULT),
        "{laid}"
    );
    let last = r.requests().pop().expect("a request");
    let sent = serde_json::to_string(&last.messages).unwrap();
    assert!(!sent.contains(WORD), "{sent}");
}

/// A model that answers the line said with a call, so its exchange holds tool traffic and the window lays it
/// whole (an exchange with none is laid spoken, and the line is laid only beside an exchange laid whole), and
/// anything else with a word. The correction note quotes the line, so it is told apart by its own words.
fn calls_on_the_line(req: &crate::provider::ProviderRequest) -> Scripted {
    let said = last_user(req);
    if answers_a_call(req) {
        Scripted::text("Read.")
    } else if said.contains(SAID) && !said.contains("Correction:") {
        Scripted::tools("Reading.", &[("h1", "context_history", json!({}))])
    } else {
        Scripted::text("Noted.")
    }
}

/// The retracted message itself, laid whole in its own conversation's window, carries the mark after its
/// words.
#[tokio::test]
async fn a_retracted_message_in_the_window_carries_the_mark() {
    let r = rig_scripted(|_| {}, calls_on_the_line);
    let sid = session(&r.core, None);
    turn(&r.core, &sid, SAID).await;
    let node = (r.core.store.session_nodes(&sid).unwrap().into_iter())
        .map(|(_, n)| n)
        .find(|n| matches!(&n.body, Body::UserMessage { text, .. } if text.contains(WORD)))
        .expect("the person's message")
        .id;
    retract(&r.core, &node);
    turn(&r.core, &sid, "and the weir?").await;
    let last = r.requests().pop().expect("a request");
    let ask = (last.messages.iter())
        .map(|m| m.to_string())
        .find(|m| m.contains(WORD) && !m.contains("Correction:"))
        .unwrap_or_else(|| panic!("the message is laid: {:#?}", last.messages));
    assert!(
        ask.contains("[Since this was said: it was marked wrong on "),
        "{ask}"
    );
    let (_, stored) = r.core.store.get_node(&node).unwrap().unwrap();
    assert!(
        matches!(&stored.body, Body::UserMessage { text, .. } if text == SAID),
        "the stored message is as said: {stored:?}"
    );
}

/// A replay of a turn whose window laid a history result is refused once a source the result showed is
/// erased: the turn's kept copy would send the erased words again.
#[tokio::test]
async fn a_replay_refuses_a_window_that_showed_erased_words_by_history() {
    use crate::tests_imports::{episode, import};
    let r = rig();
    let ids = import(
        &r.core,
        &[episode(7, "personal", "2026-03-03T18:00:00Z", SAID)],
    );
    let other = session(&r.core, None);
    read_in(&r.core, &other, json!({ "open": ids[0] })).await;
    let t = turn(&r.core, &other, "and the weir?").await.turn_id;
    if let Err(e) = r.core.rebuild_first(&other, &t) {
        panic!("the turn is kept and rebuilds before the erase: {e}");
    }
    crate::import::write::erase(&r.core.store, "kelp-2026", Some("a test"), "cli").unwrap();
    let Err(e) = r.core.rebuild_first(&other, &t) else {
        panic!("a turn whose window showed erased words was rebuilt");
    };
    assert_eq!(
        (e.part, e.why.as_str()),
        ("view", crate::own_bench::rebuild::VIEW_ERASED)
    );
}
