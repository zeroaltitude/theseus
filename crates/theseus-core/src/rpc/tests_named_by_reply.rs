//! A thread is named by the words the turn's message brings, not by the
//! reply before it (theseus-n6mkd). The recall's query is the message plus
//! the start of the previous reply, so it can hold another thread's title or
//! working-set word the assistant itself said; read as the message, that
//! waived the other thread's distance, and its notes came in although the
//! asking thread's own best note outscored them (theseus-stwkb). Through the
//! whole turn, with the index answering fixed scores.

use serde_json::json;
use theseus_protocol::Usage;

use super::tests_thread_recall::{canary, new_thread, say, scored, thread_session};
use super::Core;
use crate::node::{Body, Node};
use crate::tests_recall::turn;

/// The assistant's reply in `sid`, the turn before the one under test.
fn reply(c: &Core, sid: &str, text: &str) {
    let n = Node::assistant(
        sid,
        "trn_prev",
        0,
        Body::AssistantMessage {
            blocks: vec![json!({"type": "text", "text": text})],
            model: "test".into(),
            provider: "test".into(),
            stop_reason: Some("end_turn".into()),
            usage: Usage::default(),
            cost_usd: None,
            catalog_version: None,
            request_id: None,
            correlation_id: None,
            compilation_id: None,
            request_digest: None,
            effort: None,
        },
    );
    c.store.append(&[n.record().unwrap()]).unwrap();
}

/// The sessions whose notes the turn's recall node carries.
fn recalled_sessions(c: &Core, sid: &str) -> Vec<String> {
    c.store
        .session_nodes(sid)
        .unwrap()
        .into_iter()
        .filter_map(|(_, n)| match n.body {
            Body::Recall { items, .. } => Some(items),
            _ => None,
        })
        .flatten()
        .map(|i| i.session_id)
        .collect()
}

/// The harbour thread holds its own best note (in context, 0.116); the other
/// thread's note is at 0.100, past the bar's share (0.86) but, once it has
/// paid, under the own note. The previous reply says the other thread's
/// title; the new message does not.
#[tokio::test]
async fn a_thread_the_previous_reply_named_is_not_waived() {
    let r = canary();
    let c = &r.core;
    new_thread(c, "harbour", "Harbour log notes");
    new_thread(c, "docs-census", "Docs word census");
    let harbour = thread_session(c, "harbour");
    say(c, &harbour, "the ferry left at 06:40 by the harbour summary");
    reply(
        c,
        &harbour,
        "Done. The Docs word census thread counted 32,307 words.",
    );
    let docs = thread_session(c, "docs-census");
    say(c, &docs, "one doc mentions a ferry, but ferries are not counted");
    c.runner.memory.set_ask(scored(
        c,
        vec![(harbour.clone(), 0, 0.116), (docs.clone(), 0, 0.100)],
    ));
    turn(c, &harbour, "When did the ferry leave?").await;
    assert_eq!(
        recalled_sessions(c, &harbour),
        Vec::<String>::new(),
        "the other thread's note is no better than the thread's own: it drops `thread_own`"
    );
}

/// The same scores, the message itself naming the other thread: its distance
/// is waived and its note comes in.
#[tokio::test]
async fn a_thread_the_message_names_is_still_waived() {
    let r = canary();
    let c = &r.core;
    new_thread(c, "harbour", "Harbour log notes");
    new_thread(c, "docs-census", "Docs word census");
    let harbour = thread_session(c, "harbour");
    say(c, &harbour, "the ferry left at 06:40 by the harbour summary");
    reply(c, &harbour, "Done.");
    let docs = thread_session(c, "docs-census");
    say(c, &docs, "one doc mentions a ferry, but ferries are not counted");
    c.runner.memory.set_ask(scored(
        c,
        vec![(harbour.clone(), 0, 0.116), (docs.clone(), 0, 0.100)],
    ));
    turn(
        c,
        &harbour,
        "What did the docs word census say about the ferry?",
    )
    .await;
    assert_eq!(recalled_sessions(c, &harbour), vec![docs]);
}
