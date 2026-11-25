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
use theseus_protocol::SubagentAnswer;
use theseus_tools::{
    parse, AsyncMediaRun, Backend, Registry, Retry, Tool, ToolClass, ToolCtx, ToolFailure,
    ToolOutput,
};

use crate::approval::Answerer;
use crate::chat_acts::Said;
use crate::config_change::{flag_way, hold_words, outside_text, wall};
use crate::toolrun::TurnCtx;
use crate::Core;

mod ask;
mod hold;
pub(crate) mod names;
pub(crate) mod run;

pub use ask::{ANSWER_MAX_TOKENS, QUESTION_MAX_CHARS, TIME_LIMIT};
pub(crate) use names::told;
pub use run::{ProfileRun, RUN};

pub const ASK: &str = "profile.ask";
pub const HOLD: &str = "profile.hold";
/// The family the tools' names give them, and the deferred family's name.
pub const FAMILY: &str = "profile";
/// The tools' names, as the config's `[policy.tools]` checks them.
pub const NAMES: [&str; 3] = [ASK, HOLD, RUN];

/// Register both tools, in a daemon started from a config (as `look::register` does): no
/// request a test builds carries them.
pub fn register(cfg: &crate::Config, r: &mut Registry) {
    if cfg.config_file.is_some() || cfg.config_copy.is_some() {
        r.register(Arc::new(ProfileAsk));
        r.register(Arc::new(ProfileHold));
        r.register(Arc::new(ProfileRun));
    }
}

/// What each tool is about, in the words a refusal and a card use.
fn about(tool: &str) -> &'static str {
    match tool {
        HOLD => "put a conversation on a model profile",
        RUN => "hand another model profile a piece of work",
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
    held(name, outside, steered, &flag_way(tc), plan)
}

/// Handing work to a profile starts a task, so where `task.create` is held to ask first (its
/// posture is `approve`) the hand-over asks too: a profile of its own posture must not be the way
/// round the owner's setting for starting a task.
pub(crate) fn ask_if_task_asks(name: &str, task_asks: bool, mut plan: Plan) -> Plan {
    if name == RUN && task_asks && plan.authority.is_none() {
        plan.authority = Some(
            "hand another model profile a piece of work: it starts a task, and starting a task is \
             set to ask first"
                .into(),
        );
    }
    plan
}

/// [`ask_if_outside`], on the facts it reads: each cause in its own words, with `way` to clear
/// the session's flag.
fn held(name: &str, outside: bool, steered: bool, way: &str, mut plan: Plan) -> Plan {
    if NAMES.contains(&name) {
        if let Some(said) = hold_words(about(name), outside, steered, way) {
            plan.authority = Some(said);
        }
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
         Theseus shows the answer to the owner as that profile's own message, so your reply need not repeat it. To have a profile DO work with tools, use \
         profile.run. To put this conversation on a profile, use profile.hold; Theseus's own command line in a shell is not the way. Offered only to the \
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

/// The META key that says how the turn that asked ended, for the answer of the call `call` (issue
/// 28): written when it did not end well, read by the node list, which gives it to every surface
/// that draws the answer from its result node.
pub(crate) fn ended_key(call: &str) -> String {
    format!("subagent.ended.{call}")
}

/// The record of it: `ended` is `stopped` or `failed`.
pub(crate) fn ended_record(call: &str, ended: &str) -> anyhow::Result<theseus_store::NewRecord> {
    theseus_store::NewRecord::json(theseus_store::kinds::META, Some(&ended_key(call)), &ended)
}

/// What the tools reach: the core, given once it exists.
#[derive(Default)]
pub struct Board {
    core: OnceLock<Weak<Core>>,
    /// The answers a turn's calls got, until the turn posts them (`give`, `take`): in memory
    /// only, as a staged post's source is; the answer itself is in the result node. Each is
    /// kept with its session, so a turn that ends without posting is swept (`forget`).
    answers: std::sync::Mutex<std::collections::HashMap<String, Held>>,
    /// Who pressed the owner's yes on a hold's card, by the call's id, until the approved call
    /// runs and takes it (`keep_yes`, `take_yes`): in memory only, so a restart between the yes
    /// and the call leaves the hold refused, which is the way to fail.
    yes: std::sync::Mutex<std::collections::HashMap<String, Pressed>>,
}

/// The answers one turn's calls got, and whose session it was.
struct Held {
    session: String,
    answers: Vec<SubagentAnswer>,
}

/// A yes kept for a hold's call: who answered, and when.
struct Pressed {
    who: Answerer,
    session: String,
    execution: String,
    /// The confirm the yes answers has been bound (`bind_yes`): until then a turn that arrives
    /// after the yes may not have seen the confirm, so it does not drop the yes.
    bound: bool,
    at: std::time::Instant,
}

/// A yes kept for a hold's call, until its confirm is bound (`Board::keep_yes_until_bound`): drops
/// the yes unless it was bound, so an unwind drops it as an error does.
pub(crate) struct KeptYes<'a> {
    board: &'a Board,
    call: String,
    bound: bool,
}

impl KeptYes<'_> {
    /// The confirm is bound: the yes stays, for the call to take.
    pub(crate) fn bind(mut self) {
        self.board.bind_yes(&self.call);
        self.bound = true;
    }
}

impl Drop for KeptYes<'_> {
    fn drop(&mut self) {
        if !self.bound {
            self.board.drop_yes(&self.call);
        }
    }
}

/// How long a yes that no call took is kept: past it the entry is dropped, as the next yes is kept.
const YES_KEPT: std::time::Duration = std::time::Duration::from_secs(3_600);

impl Board {
    /// A call of `turn`, in `session`, got `answer`: the turn posts it to its place, in order,
    /// before its reply.
    pub(crate) fn give(&self, session: &str, turn: &str, answer: SubagentAnswer) {
        self.answers
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .entry(turn.to_string())
            .or_insert_with(|| Held {
                session: session.to_string(),
                answers: Vec::new(),
            })
            .answers
            .push(answer);
    }

    /// The answers `turn` has not posted yet, in the order they came; taken once.
    pub(crate) fn take(&self, turn: &str) -> Vec<SubagentAnswer> {
        self.answers
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .remove(turn)
            .map(|h| h.answers)
            .unwrap_or_default()
    }

    /// A turn of `session`, which arrived at `arrived`, has resumed the calls the owner's answers
    /// let run. A yes kept before it arrived that no call took is dropped: this turn was the one
    /// to take it, and nothing will. Only after the resume: a turn that erred before it left
    /// the confirmed call waiting, and the next turn takes the yes. A yes kept while the turn ran
    /// is a later turn's and stays, and so is one whose confirm was not yet bound when the turn
    /// arrived (the owner's answer was mid-write).
    pub(crate) fn drop_untaken(&self, session: &str, arrived: std::time::Instant) {
        self.yes
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .retain(|_, p| p.session != session || p.at >= arrived || !p.bound);
    }

    /// A turn of `session` has ended. What it was given and did not post is handed back (a
    /// faulted turn's answers, which the end of the turn posts as the answers of a turn that
    /// failed; a task's, which are dropped), not kept until a restart; one turn runs in a session
    /// at a time, so none of it belongs to a later turn.
    pub(crate) fn sweep(&self, session: &str) -> Vec<SubagentAnswer> {
        let mut answers = self
            .answers
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let turns: Vec<String> = answers
            .iter()
            .filter(|(_, h)| h.session == session)
            .map(|(t, _)| t.clone())
            .collect();
        turns
            .into_iter()
            .filter_map(|t| answers.remove(&t))
            .flat_map(|h| h.answers)
            .collect()
    }

    /// A `/stop` of `execution` landed: no call of it runs on a yes kept before, so none is kept.
    pub(crate) fn drop_yes_of(&self, execution: &str) {
        self.yes
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .retain(|_, p| p.execution != execution);
    }

    /// The driver's tick: a yes older than [`YES_KEPT`] that nothing took (its execution ended some
    /// other way) is dropped, whether or not another is kept.
    pub(crate) fn prune_yes(&self) {
        self.yes
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .retain(|_, p| p.at.elapsed() < YES_KEPT);
    }

    /// How many yes entries are kept (a test reads it).
    #[cfg(test)]
    pub(crate) fn held_yes(&self) -> usize {
        self.yes
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .len()
    }

    /// How many turns have answers kept (a test reads it).
    #[cfg(test)]
    pub(crate) fn held_answers(&self) -> usize {
        self.answers
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .len()
    }

    /// The owner's yes to the card of the hold `call` was pressed by `who`: kept for the call.
    pub(crate) fn keep_yes(&self, call: &str, session: &str, execution: &str, who: &Answerer) {
        let mut yes = self
            .yes
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        yes.retain(|_, p| p.at.elapsed() < YES_KEPT);
        yes.insert(
            call.to_string(),
            Pressed {
                who: who.clone(),
                session: session.to_string(),
                execution: execution.to_string(),
                bound: false,
                at: std::time::Instant::now(),
            },
        );
    }

    /// [`keep_yes`](Self::keep_yes), held by a guard: from here until [`KeptYes::bind`] the yes goes
    /// when the guard does, whether the answer returned an error, a panic unwound through it, or
    /// the caller simply let go. It only ever removes a yes sooner.
    pub(crate) fn keep_yes_until_bound(
        &self,
        call: &str,
        session: &str,
        execution: &str,
        who: &Answerer,
    ) -> KeptYes<'_> {
        self.keep_yes(call, session, execution, who);
        KeptYes {
            board: self,
            call: call.to_string(),
            bound: false,
        }
    }

    /// The confirm the yes answers is bound: a turn that arrives from now on has seen it.
    pub(crate) fn bind_yes(&self, call: &str) {
        if let Some(p) = self
            .yes
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .get_mut(call)
        {
            p.bound = true;
        }
    }

    /// The yes was not taken (its answer failed): it is not kept.
    pub(crate) fn drop_yes(&self, call: &str) {
        self.take_yes(call);
    }

    /// Who pressed the yes kept for `call`; taken once.
    pub(crate) fn take_yes(&self, call: &str) -> Option<Answerer> {
        self.yes
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .remove(call)
            .map(|p| p.who)
    }

    pub fn attach(&self, core: &Arc<Core>) {
        let _ = self.core.set(Arc::downgrade(core));
    }

    fn core(&self) -> Option<Arc<Core>> {
        self.core.get().and_then(Weak::upgrade)
    }

    /// The call `call`, behind the wall. `said` is the message that started this turn, when one
    /// of the owner's did (`chat_acts::Board::offered`): the hold acts on that, or on the
    /// owner's own yes to its card (`hold::hold`), and on nothing else.
    pub fn run(
        &self,
        tc: &TurnCtx<'_>,
        name: &str,
        input: &Value,
        call: &str,
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
            HOLD => match hold::hold(self, &core, tc, input, call, said) {
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

#[cfg(test)]
mod tests;
