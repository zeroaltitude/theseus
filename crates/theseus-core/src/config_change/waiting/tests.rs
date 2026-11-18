//! A proposal's reply says what the daemon checked and which settings questions
//! are waiting for the owner (issue 22): the turn has nothing to guess, and so
//! nothing to get wrong from the conversation's memory.

use serde_json::json;

use crate::config_change::tests::{change, rig};
use crate::node::ResultStatus;

const CHECKED: &str = "the daemon loaded all of Theseus's settings with this change, the value's \
                       meaning included, and they are valid";
const NONE_WAITING: &str = "No other settings question is waiting for the owner.";

/// The reply says the whole settings loaded with the change, and with nothing
/// else open says this is the only settings question.
#[tokio::test]
async fn the_reply_says_the_whole_settings_loaded_and_that_this_is_the_only_question() {
    let r = rig();
    let sid = r.session();
    r.turn(&sid, change("import.notes.enabled", json!(false)))
        .await;
    let (status, text) = r.result(&sid);
    assert_eq!(status, ResultStatus::Ok, "{text}");
    assert!(text.contains(CHECKED), "{text}");
    assert!(text.contains("nothing to tell the owner about whether the value is valid"));
    assert!(text.contains(NONE_WAITING), "{text}");
    assert!(
        text.contains("not tool approvals or other questions"),
        "it says what it does not count: {text}"
    );
}

/// What waits is named by its words, one line each, and what was answered or
/// lapsed is not named: a turn that says an old change waits has it from its
/// memory, never from here.
#[tokio::test]
async fn an_open_question_is_named_and_an_answered_or_lapsed_one_is_not() {
    let r = rig();
    let first = r.session();
    r.turn(&first, change("import.notes.enabled", json!(false)))
        .await;
    let second = r.session();
    r.turn(&second, change("narrative", json!(true))).await;
    let (status, text) = r.result(&second);
    assert_eq!(status, ResultStatus::Ok, "{text}");
    assert!(text.contains(CHECKED), "{text}");
    assert!(
        text.contains(
            "Other settings questions waiting for the owner:\n- change `import.notes.enabled` \
             from on to off; Theseus then restarts\n"
        ),
        "{text}"
    );
    assert!(!text.contains(NONE_WAITING), "{text}");
    assert_eq!(r.asked().len(), 2);
    // The first is declined, the second lapses: a third reply names neither.
    let q = r.asked();
    let (a, b) = (&q[0], &q[1]);
    r.core
        .confirm_action(&a.correlation_id, false, Some("no"), "cli")
        .unwrap();
    let ttl = r.core.kernel.config().confirm_ttl_ms;
    assert_eq!(r.core.expire_questions(b.requested_at_ms + ttl + 1), 1);
    assert!(r.asked().is_empty());
    let third = r.session();
    r.turn(&third, change("narrative", json!(true))).await;
    let (status, text) = r.result(&third);
    assert_eq!(status, ResultStatus::Ok, "{text}");
    assert!(text.contains(NONE_WAITING), "{text}");
    assert!(!text.contains("import.notes.enabled"), "{text}");
}

/// A question past its life that the sweep has not yet settled is not
/// counted: the reply is built from what can still be answered.
#[tokio::test]
async fn a_question_past_its_life_is_not_counted_before_the_sweep_settles_it() {
    let r = rig_short_life();
    let first = r.session();
    r.turn(&first, change("import.notes.enabled", json!(false)))
        .await;
    tokio::time::sleep(std::time::Duration::from_millis(1_200)).await;
    let second = r.session();
    r.turn(&second, change("narrative", json!(true))).await;
    let (_, text) = r.result(&second);
    assert!(text.contains(NONE_WAITING), "{text}");
}

fn rig_short_life() -> crate::config_change::tests::Rig {
    crate::config_change::tests::rig_with(|cfg, _| {
        cfg.kernel.confirm_ttl_secs = 1;
    })
}

/// A question asked from another place is named, and said to be.
#[test]
fn a_question_from_another_place_says_so() {
    let w = "change `narrative` from off to on; no restart".to_string();
    let named = |there, here| super::tagged(w.clone(), there, here);
    assert_eq!(named(Some("discord:dm:1"), Some("discord:dm:1")), w);
    assert_eq!(named(None, Some("discord:dm:1")), w);
    assert_eq!(named(Some("discord:dm:1"), None), w);
    assert_eq!(
        named(Some("discord:dm:1"), Some("web")),
        format!("{w} (asked from another place)")
    );
}
