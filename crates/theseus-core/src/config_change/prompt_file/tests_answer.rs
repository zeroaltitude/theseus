//! A prompt file's change, its undo and a long setting's change are answered
//! only by the owner's own press or terminal, never by words in their name
//! (issue 23 under issue 22's rule): the model's answer tool, driven against
//! each waiting card, leaves it waiting, and a no from words works.

use serde_json::json;

use super::tests::{answer, files, replace};
use crate::config_change::tests::{change, rig, FILE};
use crate::config_change::tests_answer::{held_against_words, owner_in_dm};
use crate::rpc::Rides;

/// The card of a change to a prompt file is a settings question as the other
/// kinds are: it carries the daemon's words, the line of what it changes and
/// the card whole, and no word of the model's reason is the line.
#[tokio::test]
async fn a_prompt_file_card_is_a_settings_question_with_its_own_words() {
    let f = files();
    let q = f
        .ask(replace(&f.named, "Rule 5: be careful.", "Rule 5: be kind."))
        .await;
    let p = q.plain.as_ref().expect("the daemon worded it");
    assert_eq!(p.detail, q.reason, "the card whole, as the daemon built it");
    assert!(
        p.what.starts_with("change the prompt file `")
            && p.what
                .ends_with("(1 line added, 1 line removed); no restart"),
        "{}",
        p.what
    );
    assert!(!p.what.contains("the owner asked for it"), "{}", p.what);
    assert!(
        p.yes.contains("next message") && !p.yes.contains("restart"),
        "{p:?}"
    );
}

/// A yes from words for a prompt file's change is refused by the tool and
/// again where an answer becomes a decision; it still waits; a no works.
#[tokio::test]
async fn a_prompt_file_change_is_not_answered_by_words() {
    let f = files();
    let before = f.text();
    let q = f
        .ask(replace(&f.named, "Rule 5: be careful.", "Rule 5: be kind."))
        .await;
    held_against_words(&f.r, &q);
    assert_eq!(f.text(), before, "nothing was written");
    f.r.core
        .answer_plain_act(&q, "no", None, owner_in_dm())
        .unwrap();
    assert!(f.r.asked().is_empty());
    assert_eq!(f.text(), before);
}

/// The same for the undo of a prompt file's change, which goes through the
/// same card.
#[tokio::test]
async fn a_prompt_file_undo_is_not_answered_by_words() {
    let f = files();
    let secret = f.r.core.proofs.enrol_stand_in();
    let before = f.text();
    let q = f
        .ask(replace(&f.named, "Rule 5: be careful.", "Rule 5: be kind."))
        .await;
    let code = crate::owner_proof::Proofs::stand_in_code(&secret, 0);
    answer(&f.r, &q.correlation_id, true, Some(&code))
        .await
        .unwrap();
    let written = f.text();
    let sid = f.r.session();
    f.r.turn_with("config_undo", &sid, json!({"reason": "put it back"}))
        .await;
    let u = f.r.asked().pop().expect("the undo's card");
    assert!(u.plain.as_ref().is_some_and(|p| p
        .what
        .starts_with("undo change #1, putting back the prompt file")));
    held_against_words(&f.r, &u);
    assert_eq!(f.text(), written, "nothing was put back");
    assert_ne!(written, before);
}

/// A setting too long for an ordinary card goes on a card in this form, and a
/// yes from words for it is refused as for any setting.
#[tokio::test]
async fn a_long_value_change_is_not_answered_by_words() {
    let r = rig();
    let sid = r.session();
    let value: String = (1..=60)
        .map(|i| format!("Rule {i}: be careful. "))
        .collect();
    r.turn(&sid, change("model.system", json!(value))).await;
    let q = r.asked().remove(0);
    assert!(q.input["whole"] == true, "{q:?}");
    let p = q.plain.as_ref().expect("the daemon worded it");
    assert_eq!(p.detail, q.reason);
    assert!(p.what.contains("`model.system`"), "{}", p.what);
    held_against_words(&r, &q);
    assert_eq!(r.file_text(), FILE, "nothing was written");
    r.core
        .answer_plain_act(&q, "no", None, owner_in_dm())
        .unwrap();
    assert!(r.asked().is_empty());
}

/// The check is by the card's kind and not by a tool's name: a yes typed for the
/// card, whichever caller carries it, is refused at the one place an answer
/// becomes a decision.
#[tokio::test]
async fn the_decision_itself_refuses_a_typed_yes_for_a_prompt_file() {
    let f = files();
    let q = f
        .ask(replace(&f.named, "Rule 5: be careful.", "Rule 5: be kind."))
        .await;
    let e =
        f.r.core
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
    assert_eq!(f.r.asked().len(), 1, "it still waits");
}
