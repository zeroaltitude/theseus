//! A settings question for an owner's message puts what capture made of it to
//! rest (issue 22): the owner's "set X to Y" was offered as a standing order,
//! and "add a line to the persona" was kept as a standing preference, beside
//! the card that asks the change.

use serde_json::json;
use theseus_judge::band::Band;

use crate::capture::act::{Done, Message};
use crate::capture::{Heard, Said, Verdict};
use crate::config_change::tests::{change, rig, rig_told, Rig};
use crate::node::{Body, ResultStatus};
use crate::Core;

const SETTING: &str = "please set self-improvement install to auto";
const PERSONA: &str = "I ask for a small persona change, add a line to the helper persona: I sign \
                       off status updates with a dash";

/// A settings question that is asked whatever the tool's setting (issue 57: a
/// sensitive table keeps its card, and this one needs no code), at the default:
/// the card is asked in the turn that holds the owner's words.
fn asks_always() -> serde_json::Value {
    change("content_based_security.injection", json!("refuse"))
}

/// The node of the session's input message, once its turn has written it.
fn input_node(core: &Core, sid: &str) -> String {
    core.store
        .session_nodes(sid)
        .unwrap()
        .into_iter()
        .find_map(|(_, n)| matches!(n.body, Body::UserMessage { .. }).then_some(n.id))
        .expect("the turn wrote its input")
}

/// What Jev heard, sure of it: an order is always asked, the rest kept.
fn verdict(heard: Heard) -> Verdict {
    Verdict {
        heard,
        band: Band::Act,
        confidence: 0.95,
        p_correction: 0.0,
        said: Said::Place,
        sure_of_scope: true,
        judgment: "jdg_x".into(),
        pack: "directive.v2".into(),
    }
}

/// The judge's verdict on `text`, acted on as the inbound point acts on it.
fn hear(core: &Core, sid: &str, node: &str, text: &str, heard: Heard) -> Done {
    let m = Message {
        session: sid.into(),
        node: node.into(),
        text: text.into(),
        place_kind: "cli (private)".into(),
    };
    core.capture_now(&m, &verdict(heard)).unwrap()
}

/// The live words capture holds: asks open, and records kept and not ended.
fn held(r: &Rig) -> (Vec<String>, Vec<String>) {
    let asks = r.core.capture_asks(true).unwrap();
    let all = r.core.runner.standing.all(&r.core.store).unwrap();
    let live = crate::standing::live(&all, None, theseus_protocol::now_unix_ms());
    (
        asks.into_iter().map(|a| a.text).collect(),
        live.into_iter()
            .filter(|x| x.source.starts_with("capture:"))
            .map(|x| x.text.clone())
            .collect(),
    )
}

/// The turn's input is the words capture asked about: the card that asks the
/// change is the only question, the other is withdrawn, and the ledger says so.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_settings_question_withdraws_the_standing_order_question_for_the_same_message() {
    let r = rig_told();
    let sid = r.session();
    let core = r.core.clone();
    let id = sid.clone();
    r.when_the_turn_asks_the_model(move || {
        let node = input_node(&core, &id);
        assert!(matches!(
            hear(&core, &id, &node, SETTING, Heard::Order),
            Done::Asked(_)
        ));
    });
    r.turn(&sid, asks_always()).await;
    let (status, text) = r.result(&sid);
    assert_eq!(status, ResultStatus::Ok, "{text}");
    assert_eq!(r.asked().len(), 1, "the settings question is asked");
    assert_eq!(held(&r), (vec![], vec![]));
    let rows = r.rows("standing.answered");
    assert_eq!(rows.len(), 1, "{rows:?}");
    assert_eq!(rows[0]["answer"], "withdrawn");
}

/// The same words kept: a standing preference of the place, which rides every
/// prompt of it. The turn's settings question ends the record, and says so.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_settings_question_ends_the_record_kept_from_the_same_message() {
    let r = rig_told();
    let sid = r.session();
    let core = r.core.clone();
    let id = sid.clone();
    r.when_the_turn_asks_the_model(move || {
        let node = input_node(&core, &id);
        assert!(matches!(
            hear(&core, &id, &node, PERSONA, Heard::Preference),
            Done::Kept(_)
        ));
    });
    r.turn(&sid, asks_always()).await;
    assert_eq!(r.asked().len(), 1);
    assert_eq!(held(&r), (vec![], vec![]));
    let rows = r.rows("standing.revoked");
    assert_eq!(rows.len(), 1, "{rows:?}");
    assert_eq!(rows[0]["who"], "theseus");
    // The pack is not blamed for it: no label, and the kept note is gone.
    assert!(r.rows("judge.labeled").is_empty());
    assert!(r
        .core
        .runner
        .standing
        .capture
        .peek(&r.core.store)
        .unwrap()
        .kept
        .is_empty());
}

/// The judge answers after the turn has asked: its verdict on those words
/// makes no ask and no record, and the same verdict on another message does.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_verdict_that_comes_after_the_settings_question_makes_nothing() {
    let r = rig_told();
    let sid = r.session();
    r.turn(&sid, asks_always()).await;
    let node = input_node(&r.core, &sid);
    assert_eq!(
        hear(&r.core, &sid, &node, SETTING, Heard::Order),
        Done::Nothing
    );
    assert_eq!(
        hear(&r.core, &sid, &node, PERSONA, Heard::Preference),
        Done::Nothing
    );
    assert_eq!(held(&r), (vec![], vec![]));
    let other = hear(&r.core, &sid, "msg_later", PERSONA, Heard::Preference);
    assert!(matches!(other, Done::Kept(_)), "{other:?}");
}

/// Only this message's words go: what capture made of an earlier message, and
/// of another session's, stays.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn what_capture_made_of_another_message_is_left_alone() {
    let r = rig_told();
    let sid = r.session();
    let other = r.session();
    hear(
        &r.core,
        &sid,
        "msg_earlier",
        "never run it on a Friday",
        Heard::Order,
    );
    hear(
        &r.core,
        &sid,
        "msg_earlier2",
        "answers short, please",
        Heard::Preference,
    );
    hear(
        &r.core,
        &other,
        "msg_elsewhere",
        "only on weekdays",
        Heard::Order,
    );
    let (core, id) = (r.core.clone(), sid.clone());
    r.when_the_turn_asks_the_model(move || {
        let node = input_node(&core, &id);
        hear(&core, &id, &node, SETTING, Heard::Order);
    });
    r.turn(&sid, asks_always()).await;
    let (asks, kept) = held(&r);
    assert_eq!(
        asks,
        vec![
            "never run it on a Friday".to_string(),
            "only on weekdays".to_string()
        ]
    );
    assert_eq!(kept, vec!["answers short, please".to_string()]);
}

/// A turn that asks no settings question leaves what capture made of its words:
/// a real standing order is still asked, a real preference still kept.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_turn_that_proposes_nothing_leaves_the_capture_of_its_words() {
    let r = rig();
    let sid = r.session();
    let (core, id) = (r.core.clone(), sid.clone());
    r.when_the_turn_asks_the_model(move || {
        let node = input_node(&core, &id);
        let order = "from now on set my reminders for 9am";
        assert!(matches!(
            hear(&core, &id, &node, order, Heard::Order),
            Done::Asked(_)
        ));
        let ask = "always ask before you restart yourself";
        assert!(matches!(
            hear(&core, &id, &node, ask, Heard::Preference),
            Done::Kept(_)
        ));
    });
    r.turn_with("config_show", &sid, json!({})).await;
    assert!(r.asked().is_empty());
    assert_eq!(
        held(&r),
        (
            vec!["from now on set my reminders for 9am".to_string()],
            vec!["always ask before you restart yourself".to_string()]
        )
    );
    assert!(r.rows("standing.answered").is_empty());
    assert!(r.rows("standing.revoked").is_empty());
}

/// The turn names its own input as it writes it: the node a settings question
/// puts to rest is the owner's message, found by the turn's id, whatever else the
/// turn read before it.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn the_turn_names_the_owners_message_as_its_input() {
    let r = rig();
    let sid = r.session();
    let (core, id) = (r.core.clone(), sid.clone());
    r.when_the_turn_asks_the_model(move || {
        let node = input_node(&core, &id);
        let turn = core
            .store
            .session_nodes(&id)
            .unwrap()
            .into_iter()
            .find_map(|(_, n)| (n.id == node).then_some(n.turn_id))
            .flatten()
            .expect("the input has a turn");
        assert_eq!(core.turn_input(&turn), Some(node));
        assert_eq!(core.turn_input("turn_other"), None);
    });
    r.turn(&sid, change("import.notes.enabled", json!(false)))
        .await;
}

/// A record the owner chose (Keep on an ask) is his answer, not capture's: a
/// settings question for the same message leaves it standing. (The answer marks
/// it his own: `tests_capture`'s answer test.)
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_record_the_owner_chose_is_left_standing() {
    let r = rig();
    let sid = r.session();
    let (core, id) = (r.core.clone(), sid.clone());
    r.when_the_turn_asks_the_model(move || {
        let node = input_node(&core, &id);
        let words = "from now on never install without asking me";
        assert!(matches!(
            hear(&core, &id, &node, words, Heard::Preference),
            Done::Kept(_)
        ));
        let mut held = core.runner.standing.capture.lock(&core.store).unwrap();
        let mut next = (*held).clone();
        next.kept.iter_mut().for_each(|k| k.by_owner = true);
        core.store.append(&[next.row().unwrap()]).unwrap();
        held.set(next);
    });
    r.turn(&sid, change("import.notes.enabled", json!(false)))
        .await;
    assert_eq!(
        held(&r),
        (
            vec![],
            vec!["from now on never install without asking me".to_string()]
        )
    );
    assert!(r.rows("standing.revoked").is_empty());
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_failed_withdrawal_keeps_its_recovery_handle() {
    let r = rig();
    let sid = r.session();
    assert!(matches!(
        hear(&r.core, &sid, "msg_x", PERSONA, Heard::Preference),
        Done::Kept(_)
    ));
    r.core.store.fail_plain_frame(|rows| {
        rows.iter()
            .any(|r| r.key.as_deref().is_some_and(|k| k.starts_with("standing:")))
    });
    r.core.settled_by_act(&sid, "msg_x");
    let state = r.core.runner.standing.capture.peek(&r.core.store).unwrap();
    assert_eq!(
        state.kept.len(),
        1,
        "a failed ending must retain its recovery handle"
    );
    let cold = crate::capture::state::Capture::default()
        .peek(&r.core.store)
        .unwrap();
    assert_eq!(
        cold, state,
        "a fresh reader retains the same recovery handle"
    );
    r.core.settled_by_act(&sid, "msg_x");
    assert_eq!(held(&r), (vec![], vec![]));
    assert!(r
        .core
        .runner
        .standing
        .capture
        .peek(&r.core.store)
        .unwrap()
        .kept
        .is_empty());
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn settings_between_capture_check_and_lock_suppresses_the_record() {
    let r = rig();
    let sid = r.session();
    let core = r.core.clone();
    let id = sid.clone();
    crate::capture::act::before_lock(
        &sid,
        Box::new(move || {
            core.settled_by_act(&id, "msg_x");
        }),
    );
    assert_eq!(
        hear(&r.core, &sid, "msg_x", PERSONA, Heard::Preference),
        Done::Nothing
    );
    assert_eq!(held(&r), (vec![], vec![]));
}
