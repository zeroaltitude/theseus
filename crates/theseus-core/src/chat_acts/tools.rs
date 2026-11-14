//! The three chat acts as the registry holds them: name, schema, plan and
//! class. The *descriptions the model reads* are built per turn
//! ([`answer_description`], [`allow_description`]) because what is waiting and
//! what is held are facts of one session at one moment; the `description()`
//! here is the generic one, which is what `tools.find`, `explain` and the
//! policy table show.
//!
//! All three are `Backend::Harness`: each is a verb over Theseus's own state,
//! needing the core and not the world, and each is run by
//! `ToolRuntime::run_chat_act`.
//!
//! **All three are `ToolClass::Read`, and that is deliberate** — the class
//! `owner_ask` takes, for the reason its header gives: the call acts on
//! nothing of the world. Each of the three RECORDS a decision the operator has
//! already made, through a verb that keeps its own owner gate; the act that
//! follows is judged on its own terms, with its own posture, its own place
//! rule and its own `undo.v1` verdict. The class is what `undo.v1`'s
//! `worth_asking` reads, and a `Write` here would be asked about every one of
//! them — a question the pack cannot usefully answer, since none of the three
//! names a target, a command or an address.
//!
//! For [`AllowHeldCall`] it is more than a saving. A `Write` here is asked, and
//! an answer of `reshapeable` would HOLD the release of a hold: the owner's
//! S17-D6 route past a safety hold would be removed by the safety hold itself.
//! The call it admits is the one `undo.v1` already judged, it still runs at its
//! own posture, and one allowance still admits exactly one call.

use serde::Deserialize;
use serde_json::{json, Value};
use theseus_protocol::work::undo::HeldCall;
use theseus_protocol::{ConfirmRequest, Plan};
use theseus_tools::{parse, Backend, Retry, Tool, ToolClass, ToolCtx};

use super::{ALLOW, ANSWER, SYNC};

/// The answer tool's arguments.
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct AnswerArgs {
    /// The waiting question, by its id or an end of it. Omitted when exactly
    /// one waits.
    #[serde(default)]
    pub question: Option<String>,
    /// `yes`, `no`, or, for a question with options, the option's number from
    /// 1 or `other`.
    pub answer: String,
    /// The owner's own words with the answer.
    #[serde(default)]
    pub note: Option<String>,
}

/// The allow tool's arguments.
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct AllowArgs {
    /// The held call, by its id or an end of it. Omitted when exactly one is
    /// held.
    #[serde(default)]
    pub hold: Option<String>,
}

/// The sync tool's arguments: **there are none**, and that is the point. The
/// one command `crate::import::sync_run::command` builds takes no argument
/// either, so no word of a message and no string of a model's can reach a run.
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct SyncArgs {}

pub(crate) fn answer_args(input: &Value) -> Result<AnswerArgs, String> {
    let a: AnswerArgs = parse(input)?;
    match a.answer.trim().is_empty() {
        true => Err("say the answer: `yes`, `no`, or the option's number".into()),
        false => Ok(a),
    }
}

pub(crate) fn allow_args(input: &Value) -> Result<AllowArgs, String> {
    parse(input)
}

/// One waiting question, as a description names it: by something the owner can
/// say back, never by the correlation id alone.
pub(crate) fn named_question(q: &ConfirmRequest) -> String {
    let id = tail(&q.correlation_id, 8);
    match (&q.change, &q.task) {
        (Some(ch), _) => format!("`{}` — {} «{}» ({})", q.tool, ch.field, ch.title, ch.task),
        (None, Some(t)) => format!("`{}` for task «{}» (…{id})", q.tool, t.short),
        (None, None) => format!("`{}` (…{id})", q.tool),
    }
}

/// One held call, as a description names it.
pub(crate) fn named_hold(h: &HeldCall) -> String {
    format!("`{}` — {} ({})", h.tool, h.summary, h.id)
}

/// The last `n` characters of `s`.
fn tail(s: &str, n: usize) -> String {
    let count = s.chars().count();
    s.chars().skip(count.saturating_sub(n)).collect()
}

/// What the answers to `q` are, in the words the model chooses among.
fn answers_of(q: &ConfirmRequest) -> String {
    if q.tool == crate::ask::OWNER_ASK {
        return "one of its options, by its number from 1 (or `other` with a note)".into();
    }
    "`yes` or `no`".into()
}

/// The answer tool's description for THIS turn: what is waiting here, and what
/// each may be answered with. Built fresh each turn, so it can never name a
/// question that has since been answered.
pub(crate) fn answer_description(waiting: &[ConfirmRequest]) -> String {
    let mut s = String::from(
        "Answer a question waiting in this conversation, when the operator's latest message \
         answers it. Call this only for an answer they actually gave: do not answer a question \
         on their behalf, and do not call it for a message that says nothing about any of them. ",
    );
    if waiting.len() == 1 {
        s.push_str(&format!(
            "One question is waiting: {} — answer it with {}. `question` may be omitted.",
            named_question(&waiting[0]),
            answers_of(&waiting[0])
        ));
        return s;
    }
    s.push_str(&format!(
        "{} questions are waiting; name the one you mean in `question`, by its id or the end of \
         it. Answering the wrong one cannot be taken back, so when their message does not say \
         which, ask them instead of guessing:",
        waiting.len()
    ));
    for q in waiting {
        s.push_str(&format!(
            "\n- {} — answer it with {}",
            named_question(q),
            answers_of(q)
        ));
    }
    s
}

/// The allow tool's description for THIS turn: the held call, and whose
/// decision it is.
pub(crate) fn allow_description(held: &[HeldCall]) -> String {
    let mut s = String::from(
        "Let a call that was held run exactly as it was first proposed, rather than re-planning \
         it. A call is held when it could destroy something nothing holds a copy of. **This is \
         the operator's decision, not yours**: call this when their latest message says to run \
         the call as it was, and not otherwise. A message that asks about the hold, agrees with \
         something else, or asks for a safer version is not an instruction to run it. ",
    );
    if held.len() == 1 {
        s.push_str(&format!(
            "One call is held: {}. `hold` may be omitted.",
            named_hold(&held[0])
        ));
        return s;
    }
    s.push_str(&format!(
        "{} calls are held; name the one they mean in `hold`, by its id or the end of it. When \
         their words could mean more than one, ask them which instead of choosing:",
        held.len()
    ));
    for h in held {
        s.push_str(&format!("\n- {}", named_hold(h)));
    }
    s
}

pub struct AnswerQuestion;

impl Tool for AnswerQuestion {
    fn name(&self) -> &'static str {
        ANSWER
    }
    fn description(&self) -> &'static str {
        "Answer a question waiting in this conversation with the answer the operator gave. \
         Offered only while a question is waiting here, and its offered description names what \
         is waiting and what the answers are."
    }
    fn input_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "question": {"type": "string",
                    "description": "The waiting question, by its id or the end of it; omit it when exactly one is waiting"},
                "answer": {"type": "string",
                    "description": "`yes` or `no`; for a question with options, the option's number from 1, or `other`"},
                "note": {"type": "string",
                    "description": "The operator's own words with the answer, when they said more than the answer"}
            },
            "required": ["answer"],
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
        // An answer binds one confirm: a second call finds it answered and says so.
        Retry::NonRepeatable
    }
    fn plan(&self, input: &Value, _ctx: &ToolCtx) -> Result<Plan, String> {
        let a = answer_args(input)?;
        Ok(Plan {
            summary: match a.question.as_deref() {
                Some(q) => format!("answer the waiting question {q}: {}", a.answer.trim()),
                None => format!("answer the waiting question: {}", a.answer.trim()),
            },
            ..Default::default()
        })
    }
}

pub struct AllowHeldCall;

impl Tool for AllowHeldCall {
    fn name(&self) -> &'static str {
        ALLOW
    }
    fn description(&self) -> &'static str {
        "Let a held call run as it was first proposed, when the operator's message says to. \
         Offered only while a call is held in this conversation, and its offered description \
         states the held call."
    }
    fn input_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "hold": {"type": "string",
                    "description": "The held call, by its id or the end of it; omit it when exactly one is held"}
            },
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
        // One allowance admits one call (S34-2); a second call finds it spent.
        Retry::NonRepeatable
    }
    fn plan(&self, input: &Value, _ctx: &ToolCtx) -> Result<Plan, String> {
        let a = allow_args(input)?;
        Ok(Plan {
            summary: match a.hold.as_deref() {
                Some(h) => format!("let the held call {h} run as it was first proposed"),
                None => "let the held call run as it was first proposed".into(),
            },
            // NOT `authority`. That is `owner_ask`'s field and it makes the
            // gate park on a confirm AT EVERY POSTURE -- which here would ask
            // the operator to approve carrying out the decision he has just
            // made. The gate this call takes is `judge_act(Act::UndoAllow)`
            // inside `Core::undo_allow`, which is his own.
            ..Default::default()
        })
    }
}

pub struct ImportSync;

impl Tool for ImportSync {
    fn name(&self) -> &'static str {
        SYNC
    }
    fn description(&self) -> &'static str {
        "Ask for an OpenClaw import sync now, when the operator asks for one (\"please \
         re-import\", \"run the sync\"). One runs at a time: if one is already running this \
         starts nothing and says so. It takes no arguments at all — nothing of their message \
         reaches the run — and it says what it imported in this conversation when it ends."
    }
    fn input_schema(&self) -> Value {
        json!({"type": "object", "properties": {}, "additionalProperties": false})
    }
    fn class(&self) -> ToolClass {
        ToolClass::Read
    }
    fn backend(&self) -> Backend {
        Backend::Harness
    }
    fn retry(&self) -> Retry {
        // A repeat finds the run it started and says "already running".
        Retry::SafeToRepeat
    }
    fn plan(&self, input: &Value, _ctx: &ToolCtx) -> Result<Plan, String> {
        let _: SyncArgs = parse(input)?;
        Ok(Plan {
            summary: "ask for an OpenClaw import sync now".into(),
            ..Default::default()
        })
    }
}
