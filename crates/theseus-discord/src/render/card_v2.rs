//! The needs-you card as one Components V2 message (theseus-2j2ka). Pure.
//!
//! A question from the engine is one card in its place: the words the classic
//! card had (`render::card`, kept as they are), and a button for each of the
//! question's `answer_options`, whose `custom_id` is `wa:<last 6 of the
//! question>:<option>` and whose press is `work.answer` (`runtime/work_press.rs`).
//! A closed card is the same box with its settled line and no buttons.

use serde_json::Value;
use theseus_protocol::work::{AnswerOption, WorkState};

use super::board_v2::short;
use super::v2::{container, text};
use crate::runtime::emoji::state;

/// A button's `custom_id` prefix.
pub const PREFIX: &str = "wa:";
/// The card's accent while it waits, and once it closed.
const RED: u32 = 0xE5_48_4D;
const GREY: u32 = 0x8B_8D_98;

/// `wa:<last 6>:<option>`.
pub fn custom_id(question: &str, option: &str) -> String {
    format!("{PREFIX}{}:{option}", short(question))
}

/// The question's end and the option a `wa:` id names.
pub fn parse(custom_id: &str) -> Option<(&str, &str)> {
    let rest = custom_id.strip_prefix(PREFIX)?;
    let (q, option) = rest.split_once(':')?;
    (!q.is_empty() && !option.is_empty()).then_some((q, option))
}

/// The answer controls of `options`: buttons in rows of five, or one select
/// when there are more than five (`answer_v2::controls`).
pub fn buttons(question: &str, options: &[AnswerOption]) -> Vec<Value> {
    super::answer_v2::controls(question, options)
}

/// A waiting card: its words and its buttons.
pub fn waiting(question: &str, content: &str, options: &[AnswerOption]) -> Vec<Value> {
    let mut inner = vec![text(content)];
    inner.extend(buttons(question, options));
    vec![container(Some(RED), inner)]
}

/// A closed card: how it closed, and nothing to press.
pub fn closed(content: &str) -> Vec<Value> {
    vec![container(Some(GREY), vec![text(content)])]
}

/// The one message that says how many questions need you, when a burst of
/// them came inside the ping window; with none, that they were answered.
pub fn summary(open: usize) -> Vec<Value> {
    match open {
        0 => closed(&format!("{} The questions that needed you are answered.", state(WorkState::Done))),
        1 => vec![container(Some(RED), vec![text(format!("{} **1 question needs you** — it is on the card above, or answer it with `theseus answer`.", state(WorkState::NeedsYou)))])],
        n => vec![container(Some(RED), vec![text(format!("{} **{n} questions need you** — they are on the cards above, or answer them with `theseus answer`.", state(WorkState::NeedsYou)))])],
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::render::v2::{custom_ids, texts, validate};
    use theseus_protocol::work::{answer_options, Question, QuestionKind, WorkKind, WorkRef};

    fn question(kind: QuestionKind, choices: usize) -> Question {
        Question {
            id: "act_01J0000000000000000abcdef".into(),
            kind,
            asked_by: WorkRef {
                id: "exe_x".into(),
                kind: WorkKind::Task,
                session_id: None,
                title: String::new(),
            },
            prompt: "fs.write create projects/beta-summary.txt".into(),
            detail: None,
            input: None,
            options: vec![],
            on_expiry: None,
            expires_at_ms: None,
            floor: false,
            may_answer: true,
            tool: Some("fs.write".into()),
            asked_at_ms: 0,
            choices: (0..choices).map(|i| format!("choice {i}")).collect(),
        }
    }

    #[test]
    fn the_buttons_are_the_questions_options_on_wa_ids() {
        let q = question(QuestionKind::Approval, 0);
        let opts = answer_options(&q);
        let card = waiting(&q.id, "**Approve?** `fs.write` create", &opts);
        assert_eq!(validate(&card), Ok(()));
        assert_eq!(
            custom_ids(&card),
            ["wa:abcdef:approve", "wa:abcdef:decline"]
        );
        assert_eq!(texts(&card), ["**Approve?** `fs.write` create"]);
    }

    #[test]
    fn an_ask_with_many_choices_draws_one_select_and_stays_valid() {
        let q = question(QuestionKind::Ask, 30);
        let card = waiting(&q.id, "Which one?", &answer_options(&q));
        assert_eq!(validate(&card), Ok(()));
        assert_eq!(custom_ids(&card), ["ws:abcdef"]);
        assert!(serde_json::to_string(&card)
            .unwrap()
            .contains("\"choice:0\""));
    }

    #[test]
    fn a_wa_id_parses_back_and_an_option_may_hold_a_colon() {
        assert_eq!(parse("wa:abcdef:choice:2"), Some(("abcdef", "choice:2")));
        assert_eq!(parse("wa:abcdef:approve"), Some(("abcdef", "approve")));
        assert_eq!(parse("confirm:approve:act_1"), None);
        assert_eq!(parse("wa::approve"), None);
    }

    #[test]
    fn a_closed_card_has_no_buttons_and_the_summary_counts() {
        let c = closed("✅ **Approved** by the owner · `fs.write`");
        assert!(custom_ids(&c).is_empty());
        assert_eq!(texts(&summary(3)), ["🔴 **3 questions need you** — they are on the cards above, or answer them with `theseus answer`."]);
        assert!(texts(&summary(0))[0].starts_with('✅'));
    }
}
