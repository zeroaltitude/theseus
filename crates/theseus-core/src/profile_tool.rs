//! Asking a model profile, and putting a conversation on one, by tools (issue 28): Theseus
//! does its own work by a tool, not by its own command line in a shell.
//!
//! Asked to "use the `mythos` profile to say hello", a turn ran `theseus ask -P mythos` in a
//! shell. The profile's model answered, and the conversation never showed its words: the
//! answer was produced inside the shell and stayed there. These two tools are the way:
//!
//! - **`profile.ask`** (`profile_tool/ask.rs`): ONE question to a named profile, with no tool
//!   offered to the asked model, an answer capped at [`ANSWER_MAX_TOKENS`] tokens and a
//!   time limit of [`TIME_LIMIT`]. The profile's own words come back as the result, with
//!   its model and provider named, and the call is a priced provider call of this turn's (an
//!   action of its execution, reserved against the spend limit and the day ceiling and
//!   settled at its price, as compaction's summary call is).
//! - **`profile.hold`**: this conversation, and no other, goes on in a named profile from its
//!   next turn, or is let go. It calls `Core::session_set_profile`, which `theseus sessions
//!   model` calls, as the owner who spoke, so `judge_act` and the floor's refusal of a profile
//!   for the session's data judge it exactly as they judge the command.
//!
//! **The wall** is `config.show`'s (`config_change::wall`): the owner's own turn, in a private
//! place only they write in; refused to a job, a task, the self loop, a session an MCP client
//! opened and a shared place. A turn gets no new power: asking costs what a model call of
//! the turn costs and sends the question where the provider layer already lets this session's
//! data go, and a hold is the owner's own act, taken only in a turn the owner's own message
//! started (the chat acts' rule). In a clean turn both are *told*, not asked; **when the turn
//! has read text from outside since the owner's last message, each is asked first**
//! ([`ask_if_outside`]), because a tricked turn must not send words to another model, or move
//! the conversation, unasked.
//!
//! The family is deferred and registered, as the looks are, only in a daemon started from a
//! config. The whole design: `docs/design/own-tools.md`.

use std::sync::{Arc, OnceLock, Weak};

use serde::Deserialize;
use serde_json::{json, Value};
use theseus_protocol::ops::SessionProfileParams;
use theseus_protocol::Plan;
use theseus_tools::{
    parse, AsyncMediaRun, Backend, Registry, Retry, Tool, ToolClass, ToolCtx, ToolFailure,
    ToolOutput,
};

use crate::chat_acts::Said;
use crate::config_change::{outside_text, wall};
use crate::toolrun::TurnCtx;
use crate::Core;

mod ask;

pub use ask::{ANSWER_MAX_TOKENS, QUESTION_MAX_CHARS, TIME_LIMIT};

pub const ASK: &str = "profile.ask";
pub const HOLD: &str = "profile.hold";
/// The family the tools' names give them, and the deferred family's name.
pub const FAMILY: &str = "profile";
/// The tools' names, as the config's `[policy.tools]` checks them.
pub const NAMES: [&str; 2] = [ASK, HOLD];

/// Register both tools, in a daemon started from a config (as `look::register` does): no
/// request a test builds carries them.
pub fn register(cfg: &crate::Config, r: &mut Registry) {
    if cfg.config_file.is_some() || cfg.config_copy.is_some() {
        r.register(Arc::new(ProfileAsk));
        r.register(Arc::new(ProfileHold));
    }
}

/// What each tool is about, in the words a refusal and a card use.
fn about(tool: &str) -> &'static str {
    match tool {
        HOLD => "put a conversation on a model profile",
        _ => "ask another model profile",
    }
}

/// Why this call may not act, or `None` when it may: the wall `config.show` keeps, in this
/// tool's own words.
pub(crate) fn refusal(tc: &TurnCtx<'_>, tool: &str) -> Option<String> {
    worded(wall::refusal(tc), tool)
}

/// The wall's refusal, in this tool's own words.
fn worded(why: Option<&'static str>, tool: &str) -> Option<String> {
    let what = about(tool);
    Some(match why? {
        w if w == wall::NOT_PRIVATE => format!(
            "{tool} can {what}, so it is only for the owner's own private place: it is not offered in a shared one"
        ),
        w if w == wall::NOT_ALONE => format!(
            "{tool} can {what}, so it is for a place only the owner writes in: this one is shared with others"
        ),
        w if w == wall::NOT_A_TASK => format!(
            "{tool} is not for a task's session: a task may not {what}; report what is needed and the owner asks for it"
        ),
        _ => format!(
            "{tool} is only for the owner's own conversation: this session belongs to a job, the self-improvement loop or a connected client, none of which may {what}"
        ),
    })
}

/// The gate's hold on a call that is not free: when the turn has read text from outside since
/// the owner's last message, the call is put to the owner as any call that asks is.
pub(crate) fn ask_if_outside(tc: &TurnCtx<'_>, name: &str, plan: Plan) -> Plan {
    if !NAMES.contains(&name) {
        return plan;
    }
    let (outside, steered) = outside_text(tc);
    held(name, outside, steered, plan)
}

/// [`ask_if_outside`], on the facts it reads.
fn held(name: &str, outside: bool, steered: bool, mut plan: Plan) -> Plan {
    if NAMES.contains(&name) && (outside || steered) {
        plan.authority = Some(format!(
            "{}: this conversation has read text from outside since your last message, so the \
             request may not be only yours",
            about(name)
        ));
    }
    plan
}

/// `profile.ask`.
pub struct ProfileAsk;

impl Tool for ProfileAsk {
    fn name(&self) -> &str {
        ASK
    }

    fn description(&self) -> &str {
        "Ask ONE question of another model profile and get its own words back, with its model named: \
         one call, no tools for the asked model, an answer of at most 4,000 tokens, 100 seconds. It is \
         counted and priced as a model call of this turn. Use it for \"use <profile> to say hello\". \
         Give the answer to the owner as that profile's words. To put this conversation on a profile, \
         use profile.hold; Theseus's own command line in a shell is not the way. Offered only to the \
         owner, in a private place."
    }

    fn input_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "profile": {"type": "string", "description": "The profile's name, as look.theseus lists it."},
                "question": {"type": "string", "description": "The one question, in full: the profile sees nothing else."}
            },
            "required": ["profile", "question"],
            "additionalProperties": false,
        })
    }

    fn class(&self) -> ToolClass {
        ToolClass::Read
    }

    fn backend(&self) -> Backend {
        Backend::Async
    }

    fn retry(&self) -> Retry {
        // A second call is a second question, and is paid for.
        Retry::NonRepeatable
    }

    fn plan(&self, input: &Value, _ctx: &ToolCtx) -> Result<Plan, String> {
        let (profile, _) = ask::input_of(input)?;
        Ok(Plan {
            summary: format!("ask the profile {profile} one question"),
            ..Default::default()
        })
    }

    fn run_async(&self, _input: &Value, _ctx: &ToolCtx) -> theseus_tools::AsyncRun {
        Box::pin(std::future::ready(Err(ToolFailure::new(
            "profile.ask runs through the runtime, which asks the profile",
        ))))
    }
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct HoldInput {
    #[serde(default)]
    profile: Option<String>,
    #[serde(default)]
    release: bool,
}

/// The profile to hold the conversation to, or `None` to let the hold go.
fn hold_of(input: &Value) -> Result<Option<String>, String> {
    let i: HoldInput = parse(input)?;
    match (i.profile.as_deref().map(str::trim), i.release) {
        (Some(p), false) if !p.is_empty() => Ok(Some(p.to_string())),
        (None, true) => Ok(None),
        _ => Err(
            "name a profile to hold this conversation to, or set `release` to let the hold go"
                .into(),
        ),
    }
}

/// `profile.hold`.
pub struct ProfileHold;

impl Tool for ProfileHold {
    fn name(&self) -> &str {
        HOLD
    }

    fn description(&self) -> &str {
        "Put THIS conversation on a named model profile from its next turn (the owner's \"stay on <profile>\"), \
         or let the hold go (`release`). It is what `theseus sessions model` does, judged the same: the \
         floor may refuse a profile for this conversation's data, and the answer says so. The turn that \
         calls it keeps its own model. Only in a turn the owner's own message started. To ask a profile \
         one question, use profile.ask; Theseus's own command line in a shell is not the way."
    }

    fn input_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "profile": {"type": "string", "description": "The profile's name, as look.theseus lists it."},
                "release": {"type": "boolean", "description": "True, with no profile, to let the hold go."}
            },
            "additionalProperties": false,
        })
    }

    fn class(&self) -> ToolClass {
        ToolClass::Read
    }

    fn backend(&self) -> Backend {
        Backend::Async
    }

    fn retry(&self) -> Retry {
        // A second call finds the hold as it was set and says so.
        Retry::SafeToRepeat
    }

    fn plan(&self, input: &Value, _ctx: &ToolCtx) -> Result<Plan, String> {
        let summary = match hold_of(input)? {
            Some(p) => format!("hold this conversation to the profile {p}"),
            None => "let go this conversation's held profile".into(),
        };
        Ok(Plan {
            summary,
            ..Default::default()
        })
    }

    fn run_async(&self, _input: &Value, _ctx: &ToolCtx) -> theseus_tools::AsyncRun {
        Box::pin(std::future::ready(Err(ToolFailure::new(
            "profile.hold runs through the runtime, which puts the conversation on the profile",
        ))))
    }
}

/// What the tools reach: the core, given once it exists.
#[derive(Default)]
pub struct Board {
    core: OnceLock<Weak<Core>>,
}

impl Board {
    pub fn attach(&self, core: &Arc<Core>) {
        let _ = self.core.set(Arc::downgrade(core));
    }

    fn core(&self) -> Option<Arc<Core>> {
        self.core.get().and_then(Weak::upgrade)
    }

    /// The call, behind the wall. `said` is the message that started this turn, when one of
    /// the owner's did (`chat_acts::Board::offered`): the hold acts on nothing else.
    pub fn run(
        &self,
        tc: &TurnCtx<'_>,
        name: &str,
        input: &Value,
        said: Option<Said>,
    ) -> AsyncMediaRun {
        let fail = |why: String| -> AsyncMediaRun {
            Box::pin(std::future::ready(Err(ToolFailure::new(why))))
        };
        if let Some(why) = refusal(tc, name) {
            return fail(why);
        }
        let Some(core) = self.core() else {
            return fail(format!("{name} is not ready: the core is not built"));
        };
        match name {
            HOLD => match hold(&core, tc, input, said) {
                Ok(said) => Box::pin(std::future::ready(Ok((output(said, HOLD), None, None)))),
                Err(why) => fail(why),
            },
            _ => {
                let planned = ask::input_of(input)
                    .and_then(|(profile, question)| ask::begin(&core, tc, &profile, &question));
                match planned {
                    Err(why) => fail(why),
                    Ok(p) => Box::pin(async move {
                        let (text, meta) = ask::finish(core, p).await.map_err(ToolFailure::new)?;
                        Ok((ToolOutput { text, meta }, None, None))
                    }),
                }
            }
        }
    }
}

/// What a tool gives the model: its words, and which tool it was.
fn output(text: String, tool: &str) -> ToolOutput {
    ToolOutput {
        text,
        meta: json!({ "profile_tool": tool }),
    }
}

/// Put this conversation on the profile the model named, as the owner who spoke: through
/// `session_set_profile`, which the command calls.
fn hold(
    core: &Core,
    tc: &TurnCtx<'_>,
    input: &Value,
    said: Option<Said>,
) -> Result<String, String> {
    let to = hold_of(input)?;
    let said = said.filter(|s| s.owner_input).ok_or(
        "profile.hold acts only in a turn the owner's own message started, and no message of \
         theirs started this one. Nothing was done.",
    )?;
    let done = core
        .session_set_profile(
            &SessionProfileParams {
                session_id: tc.session_id.to_string(),
                profile: to.clone(),
                author: None,
                discord: None,
            },
            &said.who,
        )
        .map_err(|e| format!("{e:#}"))?;
    if let Some(why) = done.refused {
        return Err(format!("Nothing was changed: {why}."));
    }
    let now = match &done.profile {
        Some(p) => format!("This conversation's next turns run on {p}"),
        None => {
            "This conversation's profile hold is let go: its next turns route as the default does"
                .into()
        }
    };
    let was = match (&done.previous, &done.profile) {
        (p, n) if p == n => " (it already was)".to_string(),
        (Some(p), _) => format!(" (it was held to {p})"),
        (None, _) => String::new(),
    };
    let note = done
        .note
        .map(|n| format!(" Note: {n}."))
        .unwrap_or_default();
    Ok(format!(
        "{now}{was}. This turn keeps the model it started on.{note}"
    ))
}

#[cfg(test)]
mod tests;
