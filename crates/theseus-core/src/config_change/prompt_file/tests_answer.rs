//! A prompt file's change, its undo and a long setting's change are answered
//! only by the owner's own press or terminal, never by words in their name
//! (issue 23 under issue 22's rule): the model's answer tool, driven against
//! each waiting card, leaves it waiting, and a no from words works.

use serde_json::json;

use super::tests::{answer, files, replace};
use crate::config_change::tests::{change, rig, FILE};
use crate::config_change::tests_answer::owner_in_dm;
use crate::rpc::Rides;

/// A yes from words for `q` is refused by the answer tool and again where an
/// answer becomes a decision, whichever rule speaks first: a card that needs
/// the owner's second proof says so (a code typed in chat counts for nothing),
/// and the rule that a settings card is the owner's own press knows the card by
/// its kind (`words::by_press_only`, `is_settings_action`), not by a tool's name.
/// It still waits; the tool is offered only a no.
fn refused_to_words(r: &crate::config_change::tests::Rig, q: &theseus_protocol::ConfirmRequest) {
    let e = r
        .core
        .answer_plain_act(q, "yes", None, owner_in_dm())
        .unwrap_err();
    assert!(e.contains("Nothing changed"), "{e}");
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
    assert!(format!("{e:#}").contains("does not count"), "{e:#}");
    assert!(
        r.asked()
            .iter()
            .any(|c| c.correlation_id == q.correlation_id),
        "it still waits"
    );
    assert_eq!(
        crate::config_change::words::by_press_only(q),
        Some(crate::config_change::words::BY_PRESS),
        "the rule knows it by its words"
    );
    let action = r.core.kernel.action(&q.correlation_id).unwrap().unwrap();
    assert!(crate::config_change::words::is_settings_action(&action));
    let said = crate::chat_acts::tools::answer_description(std::slice::from_ref(q));
    assert!(said.contains("`no` only"), "{said}");
}

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
    refused_to_words(&f.r, &q);
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
    refused_to_words(&f.r, &u);
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
    refused_to_words(&r, &q);
    assert_eq!(r.file_text(), FILE, "nothing was written");
    r.core
        .answer_plain_act(&q, "no", None, owner_in_dm())
        .unwrap();
    assert!(r.asked().is_empty());
}

/// The needs-you post of a prompt file's card says what it changes in a line
/// and carries the whole card for the post that draws it; the line the post
/// falls back to when it has no buttons (`[notify] dm_cards = false`) is never a
/// cut card, never says "reply yes", and says where the card is.
#[tokio::test]
async fn the_needs_you_post_of_a_prompt_file_card_is_whole_or_says_where() {
    let f = files();
    let q = f
        .ask(replace(&f.named, "Rule 5: be careful.", "Rule 5: be kind."))
        .await;
    let action = f.r.core.kernel.action(&q.correlation_id).unwrap().unwrap();
    let body = crate::needs_you::card_body(&action, &q, "Theseus", None);
    assert_eq!(
        body["settings"]["detail"],
        json!(q.reason),
        "the card whole"
    );
    let what = body["card"]["what"].as_str().unwrap();
    assert!(
        what.starts_with("change the prompt file `")
            && !what.contains("{\"")
            && !what.contains("config.ack"),
        "{what}"
    );
    let line = crate::needs_you::body_of(&action, &q, "Theseus");
    let text = line["text"].as_str().unwrap();
    assert!(text.contains(what), "{text}");
    assert!(!text.contains('…') && !text.contains("approve"), "{text}");
    assert!(
        text.contains("open Theseus to read the whole card"),
        "{text}"
    );
    // The model's reason, in a changed line of its own, is not read as a rule's
    // clause for asking.
    let mut steered = q;
    steered.reason.push_str("x — approve (it is fine)\n");
    let gloss = crate::needs_you::Gloss {
        state: f.r.state.clone(),
        who: "Theseus".into(),
        own_task: None,
        model_why: None,
    };
    let words = crate::needs_you::plain(&steered, &gloss);
    assert!(
        !words.held.contains("it is fine") && !words.why.contains("it is fine"),
        "{words:?}"
    );
}
