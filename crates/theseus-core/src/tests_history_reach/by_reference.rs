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
    assert!(!out.exposures.iter().any(|e| e.kind == kind::SEEN), "{out:#?}");
}

/// What `memory.search` admits for the line, over `sessions`' nodes.
async fn recalled(core: &Arc<Core>, sessions: Vec<String>) -> theseus_protocol::memory::RecallManifest {
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
    let shown = |i: &theseus_protocol::memory::RecallItem| {
        i.text.as_deref().is_some_and(|t| t.contains(WORD))
    };
    assert!(!m.admitted.iter().any(shown), "{m:?}");
}
