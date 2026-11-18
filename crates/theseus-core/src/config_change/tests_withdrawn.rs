//! A settings question for an owner's message withdraws the capture question
//! still open for that message (issue 22): the owner's "set X to Y" was offered
//! as a standing order to keep, beside the card that asks the change.

use std::sync::Arc;

use serde_json::json;
use theseus_protocol::{StandingAsk, StandingKind, StandingScope};

use crate::capture::state::Open;
use crate::config_change::tests::{change, rig, Rig};
use crate::node::{Body, ResultStatus};
use crate::Core;

/// The node of the session's input message, once its turn has written it.
fn input_node(core: &Core, sid: &str) -> String {
    core.store
        .session_nodes(sid)
        .unwrap()
        .into_iter()
        .find_map(|(_, n)| matches!(n.body, Body::UserMessage { .. }).then_some(n.id))
        .expect("the turn wrote its input")
}

/// A capture question open for `node`, as the judge leaves one.
fn open_ask(core: &Core, sid: &str, node: &str, text: &str) {
    let mut held = core.runner.standing.capture.lock(&core.store).unwrap();
    let mut next = (*held).clone();
    next.asks.push(Open {
        ask: StandingAsk {
            id: format!("ask_{}", next.asks.len() + 1),
            kind: StandingKind::Order,
            text: text.into(),
            scope: StandingScope::Everywhere,
            why: "order".into(),
            node: node.into(),
            at_ms: theseus_protocol::now_unix_ms(),
        },
        judgment: "jdg_x".into(),
        session: sid.into(),
    });
    core.store.append(&[next.row().unwrap()]).unwrap();
    held.set(next);
}

fn open_asks(r: &Rig) -> Vec<String> {
    r.core
        .capture_asks(true)
        .unwrap()
        .into_iter()
        .map(|a| a.text)
        .collect()
}

/// The turn's input is the words the capture question is about, so the card
/// that asks the change is the only question: the other is withdrawn, and the
/// ledger says so.
#[tokio::test]
async fn a_settings_question_withdraws_the_capture_question_for_the_same_message() {
    let r = rig();
    let sid = r.session();
    let core = r.core.clone();
    let id = sid.clone();
    r.when_the_turn_asks_the_model(move || {
        let node = input_node(&core, &id);
        open_ask(&core, &id, &node, "set self-improvement install to auto");
    });
    r.turn(&sid, change("import.notes.enabled", json!(false)))
        .await;
    let (status, text) = r.result(&sid);
    assert_eq!(status, ResultStatus::Ok, "{text}");
    assert_eq!(r.asked().len(), 1, "the settings question is asked");
    assert!(open_asks(&r).is_empty(), "{:?}", open_asks(&r));
    let rows = r.rows("standing.answered");
    assert_eq!(rows.len(), 1, "{rows:?}");
    assert_eq!(rows[0]["answer"], "withdrawn");
    assert_eq!(rows[0]["record"], json!(null));
}

/// Only the question for this message goes: an ask for an earlier message of
/// the session, and one for another session's, stay open.
#[tokio::test]
async fn a_capture_question_for_another_message_is_left_open() {
    let r = rig();
    let sid = r.session();
    let other = r.session();
    open_ask(
        &r.core,
        &sid,
        "msg_earlier",
        "always ask before you restart yourself",
    );
    open_ask(&r.core, &other, "msg_elsewhere", "never run it on a Friday");
    let (core, id) = (r.core.clone(), sid.clone());
    r.when_the_turn_asks_the_model(move || {
        let node = input_node(&core, &id);
        open_ask(&core, &id, &node, "set it to auto");
    });
    r.turn(&sid, change("import.notes.enabled", json!(false)))
        .await;
    assert_eq!(
        open_asks(&r),
        vec![
            "always ask before you restart yourself".to_string(),
            "never run it on a Friday".to_string()
        ]
    );
}

/// A turn that asks no settings question leaves a capture question for its
/// own message open: a standing order said in the same breath is still
/// offered (the two orders here are what a real one sounds like).
#[tokio::test]
async fn a_turn_that_proposes_nothing_leaves_the_capture_question_open() {
    let r = rig();
    let sid = r.session();
    let (core, id) = (Arc::clone(&r.core), sid.clone());
    r.when_the_turn_asks_the_model(move || {
        let node = input_node(&core, &id);
        open_ask(&core, &id, &node, "from now on set my reminders for 9am");
        open_ask(&core, &id, &node, "always ask before you restart yourself");
    });
    r.turn_with("config_show", &sid, json!({})).await;
    assert!(r.asked().is_empty());
    assert_eq!(
        open_asks(&r),
        vec![
            "from now on set my reminders for 9am".to_string(),
            "always ask before you restart yourself".to_string()
        ]
    );
    assert!(r.rows("standing.answered").is_empty());
}
