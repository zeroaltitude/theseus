//! `owner_ask` (theseus-j7cje; the owner's target of 2026-10-08: "you'll
//! always be kept in the loop on status, and when your feedback is needed").
//! A task, or a conversation, asks the owner a question with 2 to 6 options
//! and a default it expires to (30 minutes unless it says otherwise, 24 hours
//! at most), instead of guessing or stopping.
//!
//! - **It is a call that waits for the owner, as an approval does.** The gate
//!   asks at every posture (`Plan::authority`), so the execution parks
//!   `Waiting{Confirm}` and the question is an `Ask` (`QuestionKind`) with its
//!   `choices`. The floor never applies: the call acts on nothing.
//! - **An answer is not an approval of a run.** `work.answer` with `choice:<i>`
//!   (or a number from 1, or `other` with a note) binds the confirm as an
//!   approval does, in the same frame as the answer's row and a META record,
//!   `ask.answer.<id>`, holding the choice. The continuation then "runs" the
//!   call, which only reads that record back as its result (`result`):
//!   `The owner chose 2, "Ship it behind the flag"; note: …`.
//! - **Expiry is the driver's tick, not a timer.** A question's life is its
//!   plan's time plus its own (`expiry_of`), read by the same tick that
//!   expires approvals (`expire_questions`); the default applies, or the
//!   result says no one answered (`expire_ask`).
//! - **Private places only**, like the wake tools (`places::public_tool`
//!   does not list it), and answers count from the owner in a private place,
//!   as approvals do (`judge_act`).
//!
//! The pure parts, the input's limits and the words the model reads, are
//! `theseus_protocol::work::ask`.

use anyhow::Result;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use theseus_kernel::Action;
use theseus_protocol::term::words::cut_marked;
use theseus_protocol::work::ask::{self as words, AskInput, Choice};
use theseus_store::{kinds, NewRecord};
use theseus_tools::{parse, Backend, Plan, Retry, Tool, ToolClass, ToolCtx};

use crate::toolrun::TurnCtx;

pub use theseus_protocol::work::ask::OWNER_ASK;

/// The META record that holds an `owner_ask`'s answer, by the call's
/// correlation id.
pub const ANSWER_PREFIX: &str = "ask.answer.";

/// How an `owner_ask` was answered: by the owner, or by its life running out.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AskAnswer {
    /// The answer's id: `choice:<i>` or `choice:other`; empty when it expired.
    pub option: String,
    /// The owner's words with it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub note: Option<String>,
    /// Who answered, as the answer's label says; `expiry` for the clock.
    pub by: String,
    pub at_ms: u64,
    /// No one answered in time.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub expired: bool,
}

impl AskAnswer {
    /// The record, for the frame that binds the confirm.
    pub fn record(&self, correlation_id: &str) -> Result<NewRecord> {
        NewRecord::json(
            kinds::META,
            Some(&format!("{ANSWER_PREFIX}{correlation_id}")),
            self,
        )
    }
}

pub struct OwnerAsk;

impl Tool for OwnerAsk {
    fn name(&self) -> &str {
        OWNER_ASK
    }

    fn description(&self) -> &str {
        "Ask the owner to choose when a decision is truly theirs; otherwise decide and say what \
         you decided. 2 to 6 options; `default` is the number (from 1) that applies if no one \
         answers within `expires_in` minutes (30; at most 1440)."
    }

    fn input_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "question": {"type": "string"},
                "options": {
                    "type": "array",
                    "items": {
                        "type": "object",
                        "properties": {"label": {"type": "string"}, "detail": {"type": "string"}},
                        "required": ["label"]
                    }
                },
                "default": {"type": "integer"},
                "expires_in": {"type": "integer"}
            },
            "required": ["question", "options"],
            "additionalProperties": false
        })
    }

    fn class(&self) -> ToolClass {
        ToolClass::Read
    }

    fn backend(&self) -> Backend {
        Backend::Harness
    }

    fn retry(&self) -> Retry {
        // Run again after a restart, it reads the same answer back.
        Retry::SafeToRepeat
    }

    fn plan(&self, input: &Value, _ctx: &ToolCtx) -> Result<Plan, String> {
        let a = AskInput::parse(input)?;
        Ok(Plan {
            summary: format!("ask the owner: {}", cut_marked(a.question.trim(), 80)),
            // Asking acts on nothing, but only the owner answers it: the
            // gate waits at every posture, as it does for a layer-1 change.
            authority: Some("the owner's decision".into()),
            ..Default::default()
        })
    }
}

/// When a question of `a` expires: its plan's time plus its own life for an
/// `owner_ask`, else plus `ttl_ms` (`[kernel] confirm_ttl_secs`).
pub fn expiry_of(a: &Action, ttl_ms: u64) -> u64 {
    a.planned_at_ms + life_of(a).unwrap_or(ttl_ms)
}

/// An `owner_ask`'s life, in milliseconds.
pub fn life_of(a: &Action) -> Option<u64> {
    input_of(a).map(|i| i.life_ms())
}

/// An `owner_ask` action's input, as asked.
pub fn input_of(a: &Action) -> Option<AskInput> {
    if a.tool != OWNER_ASK {
        return None;
    }
    AskInput::parse(&a.proposal.as_ref()?.args).ok()
}

/// The call's result, run once the confirm is bound: the answer's record read
/// back as the words the model reads. A confirm bound without an answer (the
/// older verb) says so.
pub fn result(
    tc: &TurnCtx<'_>,
    input: &Value,
    correlation_id: &str,
) -> Result<(String, Value), String> {
    let ask: AskInput = parse(input)?;
    let key = format!("{ANSWER_PREFIX}{correlation_id}");
    let answer: Option<AskAnswer> = tc
        .store
        .get_meta(&key)
        .map_err(|e| format!("The owner's answer could not be read: {e:#}"))?;
    Ok(read_back(&ask, answer.as_ref()))
}

/// How a settled card says the owner answered: `2. Ship it behind the flag`,
/// or `in their words: …`.
pub fn answered_line(a: &Action, option: &str, note: Option<&str>) -> String {
    let n = input_of(a).map_or(0, |i| i.options.len());
    match (words::choice_of(option, n), input_of(a)) {
        (Some(Choice::Pick(i)), Some(ask)) => format!("{}. {}", i + 1, ask.options[i].label),
        _ => format!("in their words: {}", note.unwrap_or("").trim()),
    }
}

/// The words and the meta for `answer` to `ask`.
pub fn read_back(ask: &AskInput, answer: Option<&AskAnswer>) -> (String, Value) {
    let n = ask.options.len();
    let Some(ans) = answer else {
        return (
            "The owner's confirmation came without a choice. Ask again, or decide and say what \
             you decided."
                .to_string(),
            json!({"ask": {"answered": false}}),
        );
    };
    if ans.expired {
        let default = ask.default_index();
        return (
            words::expired(ask),
            json!({"ask": {"expired": true, "default": default.map(|i| i + 1)}}),
        );
    }
    let note = ans.note.as_deref();
    match words::choice_of(&ans.option, n) {
        Some(Choice::Pick(i)) => (
            words::chose(ask, i, note),
            json!({"ask": {"chosen": i + 1, "label": ask.options[i].label, "note": note}}),
        ),
        Some(Choice::Other) => (
            words::said_instead(note.unwrap_or("")),
            json!({"ask": {"other": true, "note": note}}),
        ),
        None => (
            "The owner answered with an option this question does not have. Ask again.".to_string(),
            json!({"ask": {"answered": false}}),
        ),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ask() -> AskInput {
        AskInput::parse(&json!({
            "question": "Ship?",
            "options": [{"label": "Hold"}, {"label": "Ship it behind the flag"}],
            "default": 1
        }))
        .unwrap()
    }

    fn answer(option: &str, note: Option<&str>, expired: bool) -> AskAnswer {
        AskAnswer {
            option: option.into(),
            note: note.map(str::to_string),
            by: "the CLI".into(),
            at_ms: 1,
            expired,
        }
    }

    #[test]
    fn the_model_reads_what_the_owner_chose() {
        let (t, m) = read_back(&ask(), Some(&answer("choice:1", Some("monday"), false)));
        assert_eq!(
            t,
            "The owner chose 2, \"Ship it behind the flag\"; note: monday."
        );
        assert_eq!(m["ask"]["chosen"], 2);
        let (t, _) = read_back(
            &ask(),
            Some(&answer("choice:other", Some("neither"), false)),
        );
        assert!(t.ends_with("their own words: neither"), "{t}");
    }

    #[test]
    fn an_expired_question_says_what_applied() {
        let (t, m) = read_back(&ask(), Some(&answer("", None, true)));
        assert!(t.contains("the default applied: option 1, \"Hold\""), "{t}");
        assert_eq!(m["ask"]["default"], 1);
    }

    #[test]
    fn a_confirm_without_a_choice_is_not_taken_for_one() {
        let (t, m) = read_back(&ask(), None);
        assert!(t.contains("without a choice"), "{t}");
        assert_eq!(m["ask"]["answered"], false);
    }
}
