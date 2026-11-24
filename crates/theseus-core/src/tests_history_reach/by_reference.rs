//! A node is shown with what has been said about it since. A node's words do not change once written (erase
//! is the one exception), and what is said about it later is a record of its own that points at it. So a
//! conversation that read a node's words by a look back (`context.history`, `look.work`) is told of its
//! retraction once, as a conversation that laid it any other way is.

use theseus_protocol::retract::FactRetractResult;
use theseus_protocol::SessionKind;

use super::*;
use crate::graph::{Edge, EdgeKind, VIA_HISTORY, VIA_LOOK};
use crate::session::{SessionRecord, TaskOf};

mod cut;

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

/// A new session (a task's, when `task`) holding a result of `tool` whose stored text is `text` and that
/// showed `of`'s words, written by hand as a completion frame writes it (the result, and its edge by the
/// tool's route): never run. Its id and the result's.
fn read_by_hand(
    core: &Core,
    task: bool,
    (tool, via): (&str, &str),
    (of, text): (&str, &str),
) -> (String, String) {
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
    let mut n = crate::compiler::tests_view::result("turn_read", "h9", text);
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

/// A result of `look.work` names no handle (its lines are a name and words), so its kept text never shows
/// which message it holds: a conversation other than the line's own that held one is seen and gets no note.
/// The look reads only its own conversation, so no real look can be read elsewhere: this one is written by
/// hand. Its own conversation is told by its own row (`config_change::tests_work`, `…_looked_back_is_told_once`).
#[tokio::test]
async fn a_look_result_names_no_handle_so_its_reader_is_seen() {
    let r = rig();
    let said = say(&r.core).await;
    let (reader, copy) = read_by_hand(
        &r.core,
        false,
        (crate::look::WORK, VIA_LOOK),
        (&said.node, SAID),
    );
    let out = retract(&r.core, &said.node);
    let e = of_session(&out, &reader);
    assert_eq!(
        (e.kind.as_str(), e.node_id.as_str()),
        (kind::SEEN, copy.as_str()),
        "{e:?}"
    );
    turn(&r.core, &reader, "and the weir?").await;
    assert_eq!(notes_in(&r.core, &reader), 0);
}

/// A task whose history result shows the line (its stored text as a real open of the line's exchange wrote
/// it) is named on the card the owner gets, as a task that used it any other way is; no note is queued to
/// the task.
#[tokio::test]
async fn a_task_that_read_it_by_history_is_named_on_the_card() {
    let r = rig();
    r.core.bind_places(vec![crate::places::BoundPlace {
        target: "discord:dm:5150".into(),
        name: "DM @owner".into(),
        private: true,
        ..Default::default()
    }]);
    let said = say(&r.core).await;
    // The words a real open of the exchange stored, read in a conversation of its own.
    let scratch = session(&r.core, Some("channel:lab"));
    let (_, text) = read_in(&r.core, &scratch, json!({ "open": said.turn })).await;
    let tool = (crate::view::history::HISTORY_TOOL, VIA_HISTORY);
    let (task, copy) = read_by_hand(&r.core, true, tool, (&said.node, &text));
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
    assert!(out.card, "{out:#?}");
    let cards: Vec<Value> = (r
        .core
        .outbox
        .open_for(crate::outbox::OPERATOR_TARGET)
        .iter())
    .filter(|a| crate::outbox::kind_of(a) == "retraction")
    .map(|a| crate::outbox::body_of(a).clone())
    .collect();
    assert_eq!(cards.len(), 1, "{cards:?}");
    let card = cards[0]["text"].as_str().unwrap();
    assert!(
        card.contains("count the herons"),
        "the task is named: {card}"
    );
    assert!(card.contains(WHY) && card.contains("Re-run?"), "{card}");
    let queued = crate::retract::Notes::default();
    assert!(
        !queued.waiting(&r.core.store, &task),
        "no note is queued to the task"
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

/// A page cut by the runtime, as `a_runtime_cut_keeps_every_link_the_page_gave_even_to_words_it_cut` reads
/// it: eight long lines said in one conversation, read from `channel:lab` with `[tools] result_max_chars`
/// under the page's size. The rig, the reader, its result and the result's stored text, and the said
/// messages, each its node, its exchange's turn and the tag every word of it begins with.
struct CutPage {
    r: Rig,
    reader: String,
    copy: String,
    text: String,
    said: Vec<(String, String, String)>,
}

async fn cut_page() -> CutPage {
    let r = rig_scripted(|c| c.tools.result_max_chars = 2_000, script);
    let sid = session(&r.core, None);
    for k in 0..8 {
        let words: Vec<String> = (0..30).map(|j| format!("m{k:02}w{j:02}")).collect();
        turn(&r.core, &sid, &words.join(" ")).await;
    }
    let said: Vec<(String, String, String)> = (r.core.store.session_nodes(&sid).unwrap())
        .into_iter()
        .filter_map(|(_, n)| match &n.body {
            Body::UserMessage { text, .. } if text.starts_with('m') => {
                let tag = text.get(..4)?.to_string();
                Some((n.id, n.turn_id?, tag))
            }
            _ => None,
        })
        .collect();
    assert_eq!(said.len(), 8);
    let place = crate::exchange::exchange(&r.core.store, &said[0].1)
        .unwrap()
        .expect("its exchange")
        .place;
    let reader = session(&r.core, Some("channel:lab"));
    let (copy, text) = read_in(&r.core, &reader, json!({ "place": place })).await;
    assert!(text.contains("not shown"), "the runtime cap cut it: {text}");
    CutPage {
        r,
        reader,
        copy,
        text,
        said,
    }
}

impl CutPage {
    /// A said message the cut left out entirely: neither its handle nor any of its words is in the kept text.
    fn cut_out(&self) -> &str {
        (self.said.iter())
            .find(|(_, t, tag)| {
                !self.text.contains(t.as_str()) && !self.text.contains(tag.as_str())
            })
            .map(|(n, _, _)| n.as_str())
            .unwrap_or_else(|| panic!("a message the cut left out: {}", self.text))
    }

    /// A said message whose line the cut kept: a line of the kept text ends with its handle.
    fn kept(&self) -> &str {
        (self.said.iter())
            .find(|(_, t, _)| {
                (self.text.lines())
                    .any(|l| l.starts_with("- ") && l.ends_with(&format!(" \u{b7} {t}")))
            })
            .map(|(n, _, _)| n.as_str())
            .unwrap_or_else(|| panic!("a line the cut kept: {}", self.text))
    }
}

/// **A retraction of a message the runtime cut out of a page tells its reader nothing.** The page's link to
/// it stays (the conservative superset reach and erase walk), but the reader never saw its words: it is
/// listed as seen, and no note is delivered at its next turns.
#[tokio::test]
async fn a_message_the_runtime_cut_out_of_a_page_is_seen_and_no_note_is_delivered() {
    let p = cut_page().await;
    let node = p.cut_out().to_string();
    let linked = (reach(&p.r.core, &node).descendants.iter()).any(|d| d.node_id == p.copy);
    assert!(linked, "the link is kept: {}", p.text);
    let out = retract(&p.r.core, &node);
    let e = of_session(&out, &p.reader);
    assert_eq!(
        (e.kind.as_str(), e.node_id.as_str()),
        (kind::SEEN, p.copy.as_str()),
        "{e:?}"
    );
    turn(&p.r.core, &p.reader, "and the weir?").await;
    turn(&p.r.core, &p.reader, "and at dawn?").await;
    assert_eq!(notes_in(&p.r.core, &p.reader), 0);
    let last = p.r.requests().pop().expect("a request");
    let sent = serde_json::to_string(&last.messages).unwrap();
    assert!(!sent.contains(WHY), "no note is in the request: {sent}");
}

/// A retraction of a message whose line the cut page kept tells its reader: exactly one note.
#[tokio::test]
async fn a_message_the_cut_page_kept_is_told_once() {
    let p = cut_page().await;
    let node = p.kept().to_string();
    let out = retract(&p.r.core, &node);
    let e = of_session(&out, &p.reader);
    assert_eq!(
        (e.kind.as_str(), e.node_id.as_str()),
        (kind::SESSION, p.copy.as_str()),
        "{e:?}"
    );
    turn(&p.r.core, &p.reader, "and the weir?").await;
    turn(&p.r.core, &p.reader, "and at dawn?").await;
    assert_eq!(notes_in(&p.r.core, &p.reader), 1);
}

/// A conversation that said the line and then read it back by opening its own session heard it twice,
/// directly and by the look back: one row acts for it, and it gets exactly one note.
#[tokio::test]
async fn a_conversation_that_said_it_and_read_it_back_by_history_is_told_once() {
    let r = rig();
    let said = say(&r.core).await;
    let (copy, text) = read_in(&r.core, &said.session, json!({ "open": said.session })).await;
    assert!(text.contains(WORD), "the open shows the line: {text}");
    let walk = reach(&r.core, &said.node);
    assert!(
        walk.descendants.iter().any(|d| d.node_id == copy),
        "{walk:#?}"
    );
    let out = retract(&r.core, &said.node);
    let acting = (out.exposures.iter())
        .filter(|e| e.session_id == said.session && e.kind == kind::SESSION)
        .count();
    assert_eq!(acting, 1, "{out:#?}");
    turn(&r.core, &said.session, "and the weir?").await;
    turn(&r.core, &said.session, "and at dawn?").await;
    assert_eq!(notes_in(&r.core, &said.session), 1);
}

/// **A view kept before recall left copies out, which recalled a history result, is refused once the source
/// that result showed is erased.** The record's recalled nodes name the result, and the reader's view never
/// laid or folded the exchange that read it. (Only that reference metadata is written as an older build held
/// it; the saved beside blob is the one this build kept, not one holding the result's words.) New recall no longer admits a copy, so
/// the old record is written as stored data: the turn's kept view, its recalled nodes naming the result, and
/// the exchanges it laid or folded only its own (on this rig the view also folds the other conversation's
/// exchange, which the exchange check alone would refuse; a view that did not fold it held only its own).
#[tokio::test]
async fn a_replay_refuses_a_kept_recall_of_a_history_result_whose_source_was_erased() {
    use crate::tests_imports::{episode, import};
    let r = rig();
    let ids = import(
        &r.core,
        &[episode(7, "personal", "2026-03-03T18:00:00Z", SAID)],
    );
    let a = session(&r.core, None);
    let (copy, text) = read_in(&r.core, &a, json!({ "open": ids[0] })).await;
    assert!(text.contains(WORD), "{text}");
    let b = session(&r.core, Some("channel:lab"));
    let t = turn(&r.core, &b, "and the weir?").await.turn_id;
    let cid = (r.core.store.session_nodes(&b).unwrap().into_iter())
        .find_map(|(_, n)| match n.body {
            Body::AssistantMessage {
                compilation_id: Some(c),
                ..
            } if n.turn_id.as_deref() == Some(t.as_str()) => Some(c),
            _ => None,
        })
        .expect("the turn's compilation");
    let mut v = crate::view::record::read(&r.core.store, &cid)
        .unwrap()
        .expect("the turn's view");
    let kept = v.kept.as_mut().expect("the view is kept");
    let store = &r.core.store;
    kept.exchanges.retain(
        |x| matches!(crate::exchange::exchange(store, x), Ok(Some(e)) if e.session_id == b),
    );
    kept.recalled.push(copy);
    r.core
        .store
        .append(&[crate::view::record::view_record(&v).unwrap()])
        .unwrap();
    if let Err(e) = r.core.rebuild_first(&b, &t) {
        panic!("the turn rebuilds before the erase: {e}");
    }
    crate::import::write::erase(&r.core.store, "kelp-2026", Some("a test"), "cli").unwrap();
    let Err(e) = r.core.rebuild_first(&b, &t) else {
        panic!("a kept recall of a result that showed erased words was rebuilt");
    };
    assert_eq!(
        (e.part, e.why.as_str()),
        ("view", crate::own_bench::rebuild::VIEW_ERASED)
    );
}

/// **The line's two fields from the retraction, its gist and its why, leave through the provider's exit
/// substituted**, as every string of a request does: the request the window laid, sent through the exit
/// (`exits::Substituting`) as the daemon wraps every provider, carries the vault value's name in both.
#[tokio::test]
async fn a_vault_value_in_the_lines_gist_or_why_leaves_the_exit_substituted() {
    use crate::provider::Provider as _;
    const VALUE: &str = "heron-vault-value-5150";
    let r = rig();
    let sid = session(&r.core, None);
    turn(&r.core, &sid, &format!("{VALUE} {SAID}")).await;
    let node = (r.core.store.session_nodes(&sid).unwrap().into_iter())
        .map(|(_, n)| n)
        .find(|n| matches!(&n.body, Body::UserMessage { text, .. } if text.contains(VALUE)))
        .expect("the person's message");
    let reader = session(&r.core, Some("channel:lab"));
    read_in(
        &r.core,
        &reader,
        json!({ "open": node.turn_id.clone().unwrap() }),
    )
    .await;
    r.core
        .fact_retract(
            &FactRetractParams {
                node: node.id.clone(),
                why: format!("{WHY}, by {VALUE}"),
                session_id: None,
            },
            "cli",
        )
        .unwrap();
    turn(&r.core, &reader, "and the weir?").await;
    let laid = r.requests().pop().expect("a request");
    let line = |wire: &str| -> String {
        let at = wire.find("[Since this was read: ").expect(wire);
        let end = at + wire[at..].find("as they were said.]").expect(wire);
        wire[at..end].to_string()
    };
    let before = line(&serde_json::to_string(&laid.messages).unwrap());
    assert_eq!(before.matches(VALUE).count(), 2, "gist and why: {before}");
    let board =
        crate::secrets::SecretBoard::new(["heron_key".to_string()], std::time::Instant::now());
    board.publish(
        std::collections::BTreeMap::from([(
            "heron_key".to_string(),
            Ok(crate::secrets::Secret::new(VALUE.into())),
        )]),
        "fake",
    );
    let fake = Arc::new(crate::provider::FakeProvider::default());
    let exit = crate::exits::Substituting::new(
        "fake",
        fake.clone(),
        crate::exits::Exits::new(board, true),
        None,
    );
    let mut sink = |_: crate::provider::Delta<'_>| {};
    exit.stream_message(&laid, &mut sink).await.unwrap();
    let sent = serde_json::to_string(&fake.requests().pop().expect("sent").messages).unwrap();
    let after = line(&sent);
    assert!(!sent.contains(VALUE), "{sent}");
    assert_eq!(
        after.matches("[redacted:heron_key]").count(),
        2,
        "gist and why: {after}"
    );
}
