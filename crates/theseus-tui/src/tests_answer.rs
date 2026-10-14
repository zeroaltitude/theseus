//! The answer keys over `work.answer` (theseus-ds5os): one path for every
//! kind of question. A question's own first option is what `y` sends (a
//! budget question's `reset`), a key whose answer the daemon would refuse is
//! hidden when the question says this connection `may_answer` nothing, and a
//! `confirm.resolved` from another surface clears the card.

use crossterm::event::KeyCode;
use serde_json::json;
use theseus_protocol::work::QuestionKind;
use theseus_protocol::BudgetAsk;

use crate::tests::*;
use theseus_protocol::{attention, utc_hm};

/// The world with the spec review's question changed by `edit`.
fn asking(
    edit: impl FnOnce(&mut theseus_protocol::ConfirmRequest),
) -> std::sync::Arc<std::sync::Mutex<World>> {
    let world = harbour_world();
    {
        let mut w = world.lock().unwrap();
        let mut q = gate_question("ses_spec01", "cor_spec01", 98, 98 + 252);
        edit(&mut q);
        w.board["confirms"] = json!([q]);
    }
    world
}

/// A budget question's `y` sends `reset`, the first option the daemon words
/// for it, through `work.answer`.
#[tokio::test]
async fn y_on_a_budget_question_sends_its_first_option() {
    let world = asking(|q| {
        q.tool = "budget.reset".into();
        q.budget = Some(BudgetAsk {
            spent_usd: 10.02,
            limit_usd: 10.0,
            needed_usd: 0.5,
            lifetime_usd: 42.0,
        });
        q.options = q.question(QuestionKind::Budget, true).options;
    });
    let mut rig = Rig::new(120, 20, script(world));
    rig.shows("ready  DM +1").await;
    rig.press(&[KeyCode::Tab, KeyCode::Char('y')]).await;
    rig.asked("work.answer", 1).await;
    assert_eq!(
        rig.daemon().asked("work.answer"),
        [json!({"question_id": "cor_spec01", "option": "reset", "author": "the TUI"})]
    );
}

/// What `y` and the daemon's own options agree on, with no screen: the first
/// option of each kind's answers is the plain yes.
#[test]
fn y_is_the_first_option_of_each_kind() {
    use crate::board::Question;
    let mut q = gate_question("ses_spec01", "cor_spec01", 98, 98 + 252);
    // A daemon that sends no options: by its kind.
    assert_eq!(Question::Whole(&q).yes(), "approve");
    for (kind, first) in [
        (QuestionKind::Approval, "approve"),
        (QuestionKind::Budget, "reset"),
        (QuestionKind::Change, "accept"),
        (QuestionKind::Extension, "ack"),
    ] {
        q.options = q.question(kind, true).options;
        assert_eq!(Question::Whole(&q).yes(), first);
        assert_eq!(q.options.last().unwrap().id, "decline");
    }
    assert!(Question::Whole(&q).may_answer());
    q.may_answer = Some(false);
    assert!(!Question::Whole(&q).may_answer());
}

/// A question this connection may not answer hides its keys: nothing is
/// sent, and the flash says where to answer instead.
#[tokio::test]
async fn a_question_this_connection_may_not_answer_sends_nothing() {
    let world = asking(|q| q.may_answer = Some(false));
    let mut rig = Rig::new(120, 20, script(world));
    rig.shows("ready  DM +1").await;
    rig.press(&[KeyCode::Tab]).await;
    rig.shows("[y] approve").await;
    rig.press(&[KeyCode::Char('y')]).await;
    rig.shows("theseus answer").await;
    rig.press(&[KeyCode::Char('n')]).await;
    rig.settle().await;
    assert!(rig.daemon().asked("work.answer").is_empty());
    assert!(rig.daemon().asked("action.confirm").is_empty());
}

/// Another surface answered: its `confirm.resolved` (with the option) clears
/// the card here, and the question is not drawn again if it arrives late.
#[tokio::test]
async fn a_resolution_from_another_surface_clears_the_card() {
    let world = asking(|_| {});
    let mut rig = Rig::new(120, 20, script(world));
    rig.shows("ready  DM +1").await;
    rig.press(&[KeyCode::Tab]).await;
    rig.shows("[y] approve").await;
    rig.daemon().notify(
        "confirm.resolved",
        json!({"session_id": "ses_spec01", "correlation_id": "cor_spec01",
               "approved": true, "by": "the CLI", "option": "approve"}),
    );
    // The push's change follows, with nothing pending.
    let mut spec = view(
        99,
        "ses_spec01",
        theseus_protocol::SessionKind::Conversation,
        None,
        "waiting",
        input(),
        vec![],
        1,
        1.2,
    );
    spec.attention = attention(&spec, &utc_hm);
    rig.daemon().notify("execution.changed", changed(&spec));
    rig.until("the card to clear", |r| {
        !r.screen().iter().any(|l| l.contains("[y] approve"))
    })
    .await;
    assert!(rig.daemon().asked("work.answer").is_empty());
}

/// A question with options (`owner_ask`, theseus-j7cje): its card shows the
/// question with each option numbered, a number sends that option through
/// `work.answer`, and `y` sends nothing.
#[tokio::test]
async fn a_number_answers_a_question_with_options() {
    let world = asking(|q| {
        q.tool = "owner_ask".into();
        q.input = json!({"question": "Which plan?",
                         "options": [{"label": "Careful"}, {"label": "Fast", "detail": "skips the check"}],
                         "default": 1});
        q.options = q.question(QuestionKind::Ask, true).options;
    });
    let mut rig = Rig::new(120, 24, script(world));
    rig.shows("ready  DM +1").await;
    rig.press(&[KeyCode::Tab]).await;
    rig.shows("[2] Fast — skips the check").await;
    rig.press(&[KeyCode::Char('y')]).await;
    rig.shows("press their number").await;
    assert!(rig.daemon().asked("work.answer").is_empty());
    rig.press(&[KeyCode::Char('3')]).await;
    rig.shows("no option 3").await;
    rig.press(&[KeyCode::Char('2')]).await;
    rig.asked("work.answer", 1).await;
    assert_eq!(
        rig.daemon().asked("work.answer"),
        [json!({"question_id": "cor_spec01", "option": "choice:1", "author": "the TUI"})]
    );
}
