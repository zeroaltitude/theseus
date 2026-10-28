//! `work.answer` (theseus-ds5os): one answer verb for every question. A
//! question is named by its id (the correlation id, whole or a unique end of
//! at least four characters) and answered by one of its options
//! (`work::answer_options`); the daemon routes it to the handler of the
//! question's kind. Types and pure functions only, like `work.rs`.

use serde::{Deserialize, Serialize};

use crate::work::{answer_options, AnswerOption, Question, QuestionKind, Style, WorkRef};
use crate::{ConfirmRequest, DiscordOrigin};

/// The `option` that puts an expired question to its session again
/// (theseus-27p6w): not an `AnswerOption` of an open question, since an
/// expired one has none, but what its settled card's button sends.
pub const ASK_AGAIN: &str = "ask_again";

/// `work.answer`: answer a question with one of its options.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct WorkAnswerParams {
    /// The question's id, whole, or a unique end of at least four characters.
    pub question_id: String,
    /// An `AnswerOption.id` of the question.
    pub option: String,
    /// Words with the answer: the model reads a decline's.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub note: Option<String>,
    /// Who answered, as a label. A label names; it proves nothing: the
    /// connection's surface decides (the place rule).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub author: Option<String>,
    /// Set by the Discord binding: the channel and user the answer came from.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub discord: Option<DiscordOrigin>,
    /// Subscribe this connection to the session's events before the answer
    /// wakes it, as `action.confirm`'s `watch` does.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub watch: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct WorkAnswerResult {
    /// The question's whole id, as the daemon resolved it.
    pub question_id: String,
    pub option: String,
    /// The work the answer sets going again: the execution a call or a
    /// budget question held. Absent when nothing resumes (a decline, an
    /// extension's ack, a reminder).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub resumes: Option<WorkRef>,
    /// Words the answer gives back: a routine's `details` (its latest runs
    /// and why the last one failed or was blocked).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub detail: Option<String>,
}

/// `confirm.list`'s params, all optional: the place the asking surface
/// speaks for, so each question says whether it may answer there.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct ConfirmListParams {
    /// Set by the Discord binding: the channel and user it lists for.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub discord: Option<DiscordOrigin>,
}

/// A reminder's answers, as options: the four every reminder carries
/// (`wakes::reminder_answers`, the one wording), Done first. A wake is not a
/// `Question`, so its options are made here from that list.
pub fn reminder_options() -> Vec<AnswerOption> {
    crate::wakes::reminder_answers()
        .into_iter()
        .map(|a| AnswerOption {
            style: if a.id == "done" {
                Style::Primary
            } else {
                Style::Plain
            },
            takes_note: false,
            effect: match &a.when {
                Some(w) => format!("it fires again: {w}"),
                None => "the reminder is settled".into(),
            },
            id: a.id,
            label: a.label,
        })
        .collect()
}

/// Whether an option approves (the call runs, the spend resets, the change
/// is accepted, the extension is acknowledged) rather than declines.
pub fn approves(option: &str) -> bool {
    matches!(
        option,
        "approve" | "approve_grant" | "reset" | "accept" | "ack"
    )
}

impl ConfirmRequest {
    /// The question this request is, of the `kind` the daemon judged it to
    /// be. `may_answer` is the connection's: false until the daemon says.
    pub fn question(&self, kind: QuestionKind, may_answer: bool) -> Question {
        let asking = kind == QuestionKind::Ask;
        let mut q = Question {
            id: self.correlation_id.clone(),
            kind,
            asked_by: WorkRef {
                id: self.execution_id.clone(),
                kind: match self.task {
                    Some(_) => crate::work::WorkKind::Task,
                    None => crate::work::WorkKind::Conversation,
                },
                session_id: Some(self.session_id.clone()),
                title: String::new(),
            },
            prompt: self.reason.clone(),
            detail: None,
            input: Some(self.input.clone()),
            options: Vec::new(),
            on_expiry: (self.expires_at_ms > 0).then(|| match asking {
                true => crate::work::ask::AskInput::parse(&self.input)
                    .map_or_else(|_| "no default".to_string(), |a| a.on_expiry()),
                false => "not run".to_string(),
            }),
            expires_at_ms: (self.expires_at_ms > 0).then_some(self.expires_at_ms),
            floor: self.floor,
            may_answer,
            tool: Some(self.tool.clone()),
            asked_at_ms: self.requested_at_ms,
            choices: match asking {
                true => crate::work::ask::choices_of(&self.input),
                false => Vec::new(),
            },
        };
        q.options = answer_options(&q);
        q
    }

    /// The option a plain yes is: the first the question offers.
    pub fn yes_option(&self) -> Option<&AnswerOption> {
        self.options.first()
    }
}
