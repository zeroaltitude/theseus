//! A settings question is answered by its own press or the owner's terminal,
//! never by words in their name (issue 22): the model's answer tool, driven
//! against a waiting change, undo, restart and install, leaves each waiting.

use serde_json::json;
use theseus_protocol::ConfirmRequest;

use super::tests::{change, rig, Rig};
use crate::approval::{Answerer, Surface};
use crate::rpc::Rides;

const OWNER: u64 = 4_100_000_000_000_000_001;

/// The owner, in their own direct message: the answerer of the message that
/// started a turn, which is whom the answer tool acts as.
pub(crate) fn owner_in_dm() -> Answerer {
    Answerer {
        label: format!("discord:{OWNER}"),
        surface: Surface::Discord,
        discord: Some(theseus_protocol::DiscordOrigin {
            user_id: OWNER.to_string(),
            channel_id: (OWNER + 1).to_string(),
            guild_id: None,
        }),
        agent: None,
    }
}

/// A yes through the tool is refused in words the model can pass on, by the
/// tool and again where an answer becomes a decision; the question still waits;
/// a no changes nothing and is allowed.
fn held_against_words(r: &Rig, q: &ConfirmRequest) {
    let e = r
        .core
        .answer_plain_act(q, "yes", None, owner_in_dm())
        .unwrap_err();
    assert!(
        e.contains("answered by its own button") && e.contains("Nothing changed"),
        "{e}"
    );
    // The stronger place: the daemon's own answer, whichever caller asks.
    let e = r
        .core
        .confirm_action_with(
            &q.correlation_id,
            true,
            None,
            owner_in_dm(),
            Rides {
                typed: true,
                ..Default::default()
            },
        )
        .unwrap_err();
    assert!(
        format!("{e:#}").contains("answered by its own button"),
        "{e:#}"
    );
    assert!(
        r.asked()
            .iter()
            .any(|c| c.correlation_id == q.correlation_id),
        "it still waits"
    );
    // The tool is not offered a yes for it.
    let said = crate::chat_acts::tools::answer_description(std::slice::from_ref(q));
    assert!(said.contains("`no` only"), "{said}");
}

#[tokio::test]
async fn a_change_is_not_answered_by_words() {
    let r = rig();
    let sid = r.session();
    r.turn(&sid, change("import.notes.enabled", json!(false)))
        .await;
    let q = r.asked().remove(0);
    held_against_words(&r, &q);
    assert_eq!(r.file_text(), super::tests::FILE, "nothing was written");
    // The owner's own press still answers it.
    r.core
        .confirm_action(&q.correlation_id, true, None, "cli")
        .unwrap();
    assert!(r.file_text().contains("enabled = false"));
}

#[tokio::test]
async fn a_no_from_words_is_allowed_and_changes_nothing() {
    let r = rig();
    let sid = r.session();
    r.turn(&sid, change("import.notes.enabled", json!(false)))
        .await;
    let q = r.asked().remove(0);
    r.core
        .answer_plain_act(&q, "no", None, owner_in_dm())
        .unwrap();
    assert!(r.asked().is_empty());
    assert_eq!(r.file_text(), super::tests::FILE);
}

#[tokio::test]
async fn a_restart_is_not_answered_by_words() {
    let r = rig();
    let sid = r.session();
    r.turn_with("daemon_restart", &sid, json!({"reason": "it feels stuck"}))
        .await;
    let q = r.asked().remove(0);
    held_against_words(&r, &q);
    assert!(r.core.restart_requested().is_none());
}

#[tokio::test]
async fn an_undo_is_not_answered_by_words() {
    let r = rig();
    let after = super::tests_undo::after_1();
    r.seed(super::tests::FILE, &after, &["import.notes.enabled"]);
    let sid = r.session();
    r.turn_with("config_undo", &sid, super::tests_undo::undo(None))
        .await;
    let q = r.asked().remove(0);
    held_against_words(&r, &q);
    assert_eq!(r.file_text(), after, "nothing was put back");
}

/// The needs-you post's body (what the Discord binding draws for a question that
/// has no card of its own, and always for the install card) carries the whole
/// words under `settings`, the proof's marks and the short id.
#[tokio::test]
async fn the_needs_you_post_carries_the_whole_words() {
    let r = rig();
    let sid = r.session();
    r.turn(&sid, change("import.notes.enabled", json!(false)))
        .await;
    let q = r.asked().remove(0);
    let action = r.core.kernel.action(&q.correlation_id).unwrap().unwrap();
    let body = crate::needs_you::card_body(&action, &q, "Theseus", None);
    assert_eq!(
        body["settings"]["detail"],
        json!(q.reason),
        "the card whole"
    );
    assert_eq!(body["settings"]["needs_proof"], json!(q.needs_proof));
    assert!(body["settings"]["short"]
        .as_str()
        .is_some_and(|s| !s.is_empty()));
    // An ordinary question carries none.
    let mut other = q.clone();
    other.plain = None;
    assert!(crate::needs_you::card_body(&action, &other, "Theseus", None)["settings"].is_null());
}

/// A call the floor holds (here the shell on the settings file) is approved only
/// by the owner's own press: the answer tool leaves it waiting, in words it can
/// pass on, a no from words works, and the owner's press still runs it.
#[tokio::test]
async fn a_floor_hold_is_not_approved_by_words() {
    let r = rig();
    let sid = r.session();
    let file = r.file.display().to_string();
    r.turn_with(
        "proc_run",
        &sid,
        json!({"argv": ["bash", "-c", format!("cat {file}")]}),
    )
    .await;
    let q = r.asked().remove(0);
    assert!(q.floor, "{q:?}");
    let e = r
        .core
        .answer_plain_act(&q, "yes", None, owner_in_dm())
        .unwrap_err();
    assert!(
        e.contains("held by the floor") && e.contains("Nothing ran"),
        "{e}"
    );
    let e = r
        .core
        .confirm_action_with(
            &q.correlation_id,
            true,
            None,
            owner_in_dm(),
            Rides {
                typed: true,
                ..Default::default()
            },
        )
        .unwrap_err();
    assert!(format!("{e:#}").contains("held by the floor"), "{e:#}");
    assert_eq!(r.asked().len(), 1, "it still waits");
    let said = crate::chat_acts::tools::answer_description(std::slice::from_ref(&q));
    assert!(
        said.contains("`no` only") && said.contains("floor"),
        "{said}"
    );
    // A no from words works.
    r.core
        .answer_plain_act(&q, "no", None, owner_in_dm())
        .unwrap();
    assert!(r.asked().is_empty());
}
