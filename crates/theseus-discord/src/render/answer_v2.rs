//! Answers beyond buttons (theseus-a5uds). Pure: a question's controls as a
//! card draws them, and the modals that take words and a file.
//!
//! - **Controls.** Up to five answers are buttons (`wa:<q>:<option>`), more
//!   are one string select (`ws:<q>`, the option ids as its values, at most
//!   25): an ask with six choices and "something else" is seven answers.
//! - **A note.** "Decline with a note" (`wn:<q>:decline`) and an answer that
//!   takes words (`choice:other`, whichever control it came from) open a modal
//!   (`wm:<q>:<option>`): a paragraph input, and, for an ask's own words, a file
//!   upload. A submit is `work.answer` with the words as the note.

use serde_json::{json, Value};
use theseus_protocol::work::{AnswerOption, Style};

use super::board_v2::short;
use super::v2::{button, row, select, Look};

/// A select's `custom_id` prefix: `ws:<last 6 of the question>`.
pub const SELECT: &str = "ws:";
/// "Decline with a note": `wn:<last 6>:<option>`.
pub const NOTE: &str = "wn:";
/// A modal's `custom_id` prefix: `wm:<last 6>:<option>`.
pub const MODAL: &str = "wm:";
/// The modal's inputs.
pub const NOTE_FIELD: &str = "note";
pub const FILE_FIELD: &str = "file";
/// The most buttons before the answers become a select.
pub const MAX_BUTTONS: usize = 5;
/// The option whose words are the answer (an ask's "something else").
pub const OTHER: &str = "choice:other";

const TEXT_INPUT: u64 = 4;
const LABEL: u64 = 18;
const FILE_UPLOAD: u64 = 19;
/// Discord's most characters in a paragraph input.
const NOTE_MAX: usize = 4000;

fn id_of(prefix: &str, question: &str, option: Option<&str>) -> String {
    match option {
        Some(o) => format!("{prefix}{}:{o}", short(question)),
        None => format!("{prefix}{}", short(question)),
    }
}

/// `ws:<last 6>`.
pub fn select_id(question: &str) -> String {
    id_of(SELECT, question, None)
}

/// `wn:<last 6>:<option>`.
pub fn note_id(question: &str, option: &str) -> String {
    id_of(NOTE, question, Some(option))
}

/// `wm:<last 6>:<option>`.
pub fn modal_id(question: &str, option: &str) -> String {
    id_of(MODAL, question, Some(option))
}

/// The question's end and the option of a `wn:` or `wm:` id.
pub fn parse_pair<'a>(prefix: &str, custom_id: &'a str) -> Option<(&'a str, &'a str)> {
    let rest = custom_id.strip_prefix(prefix)?;
    let (q, option) = rest.split_once(':')?;
    (!q.is_empty() && !option.is_empty()).then_some((q, option))
}

/// The question's end in a `ws:` id.
pub fn parse_select(custom_id: &str) -> Option<&str> {
    custom_id
        .strip_prefix(SELECT)
        .filter(|q| !q.is_empty() && !q.contains(':'))
}

/// Whether `custom_id` is one of this module's controls.
pub fn is_control(custom_id: &str) -> bool {
    custom_id.starts_with(SELECT) || custom_id.starts_with(NOTE)
}

fn look(s: Style) -> Look {
    match s {
        Style::Primary => Look::Success,
        Style::Plain => Look::Secondary,
        Style::Danger => Look::Danger,
    }
}

/// `options` as a card draws them: buttons (rows of five) up to
/// [`MAX_BUTTONS`] answers, else one select.
pub fn controls(question: &str, options: &[AnswerOption]) -> Vec<Value> {
    if options.len() <= MAX_BUTTONS {
        return options
            .chunks(MAX_BUTTONS)
            .map(|c| {
                row(c
                    .iter()
                    .map(|o| {
                        button(
                            super::card_v2::custom_id(question, &o.id),
                            &o.label,
                            look(o.style),
                        )
                    })
                    .collect())
            })
            .collect();
    }
    let all: Vec<(String, String)> = options
        .iter()
        .enumerate()
        .map(|(i, o)| (format!("{}. {}", i + 1, o.label), o.id.clone()))
        .collect();
    vec![select(&select_id(question), "Choose your answer…", &all)]
}

/// The `/answer` view's extra row: "Decline with a note…", for a question
/// that takes a decline's words.
pub fn note_row(question: &str, options: &[AnswerOption]) -> Option<Value> {
    options
        .iter()
        .find(|o| o.id == "decline" && o.takes_note)
        .map(|o| {
            row(vec![button(
                note_id(question, &o.id),
                "Decline with a note…",
                Look::Secondary,
            )])
        })
}

/// The modal an answer with words opens: a paragraph input, and for an ask's
/// own words a file upload beside it (`with_file`).
pub fn modal(question: &str, option: &str, title: &str, with_file: bool) -> Value {
    let required = option == OTHER;
    let mut components = vec![json!({
        "type": LABEL,
        "label": if required { "Your answer" } else { "Your note" },
        "description": if required { "In your own words" } else { "The model reads it with your answer" },
        "component": {"type": TEXT_INPUT, "custom_id": NOTE_FIELD, "style": 2,
                      "required": required && !with_file, "max_length": NOTE_MAX},
    })];
    if with_file {
        components.push(json!({
            "type": LABEL,
            "label": "A file (optional)",
            "description": "Read as any file sent here is",
            "component": {"type": FILE_UPLOAD, "custom_id": FILE_FIELD,
                          "min_values": 0, "max_values": 1, "required": false},
        }));
    }
    json!({"type": 9, "data": {
        "custom_id": modal_id(question, option),
        "title": super::clip(title, 45),
        "components": components,
    }})
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::render::v2::{custom_ids, validate};
    use theseus_protocol::work::{answer_options, Question, QuestionKind, WorkKind, WorkRef};

    fn ask(n: usize) -> Question {
        Question {
            id: "act_01J0000000000000000abcdef".into(),
            kind: QuestionKind::Ask,
            asked_by: WorkRef {
                id: "exe_x".into(),
                kind: WorkKind::Task,
                session_id: None,
                title: String::new(),
            },
            prompt: "Which?".into(),
            detail: None,
            input: None,
            options: vec![],
            on_expiry: None,
            expires_at_ms: None,
            floor: false,
            may_answer: true,
            tool: Some("owner_ask".into()),
            asked_at_ms: 0,
            choices: (0..n).map(|i| format!("choice {i}")).collect(),
        }
    }

    #[test]
    fn up_to_five_answers_are_buttons_and_more_are_one_select() {
        let few = controls(&ask(2).id, &answer_options(&ask(2)));
        assert_eq!(custom_ids(&few).len(), 3, "two choices and something else");
        assert!(few
            .iter()
            .all(|r| validate(std::slice::from_ref(r)).is_ok()));
        let many = controls(&ask(6).id, &answer_options(&ask(6)));
        assert_eq!(
            custom_ids(&many),
            ["ws:abcdef"],
            "six choices and else: a select"
        );
        let opts = many[0]["components"][0]["options"].as_array().unwrap();
        assert_eq!(opts.len(), 7);
        assert_eq!(opts[0]["value"], "choice:0");
        assert_eq!(opts[6]["value"], OTHER);
    }

    #[test]
    fn a_select_holds_at_most_25_and_its_ids_parse_back() {
        let big = controls(&ask(30).id, &answer_options(&ask(30)));
        assert_eq!(
            big[0]["components"][0]["options"].as_array().unwrap().len(),
            25
        );
        assert_eq!(parse_select("ws:abcdef"), Some("abcdef"));
        assert_eq!(parse_select("ws:"), None);
        assert_eq!(
            parse_pair(NOTE, "wn:abcdef:decline"),
            Some(("abcdef", "decline"))
        );
        assert_eq!(
            parse_pair(MODAL, "wm:abcdef:choice:other"),
            Some(("abcdef", "choice:other"))
        );
        assert!(
            is_control("ws:abcdef")
                && is_control("wn:abcdef:decline")
                && !is_control("wa:abcdef:approve")
        );
    }

    #[test]
    fn the_modal_takes_words_and_an_asks_own_words_take_a_file_too() {
        let m = modal("act_x_abcdef", "decline", "Decline", false);
        assert_eq!(m["data"]["custom_id"], "wm:abcdef:decline");
        assert_eq!(m["data"]["components"].as_array().unwrap().len(), 1);
        assert_eq!(m["data"]["components"][0]["component"]["required"], false);
        let o = modal("act_x_abcdef", OTHER, "Something else", true);
        let parts = o["data"]["components"].as_array().unwrap();
        assert_eq!(parts.len(), 2);
        assert_eq!(parts[1]["component"]["type"], FILE_UPLOAD);
        assert_eq!(
            parts[0]["component"]["required"], false,
            "a file alone is an answer"
        );
    }

    #[test]
    fn a_decline_that_takes_words_gets_the_note_row() {
        let opts = answer_options(&ask(2));
        assert!(
            note_row("act_x_abcdef", &opts).is_none(),
            "an ask has no decline"
        );
        let mut q = ask(0);
        q.kind = QuestionKind::Approval;
        let opts = answer_options(&q);
        let r = note_row(&q.id, &opts).unwrap();
        assert_eq!(custom_ids(&[r]), ["wn:abcdef:decline"]);
    }
}
