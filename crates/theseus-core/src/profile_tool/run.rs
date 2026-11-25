//! `profile.run`: hand a named model profile a piece of work that it does WITH TOOLS (issue 28).
//!
//! `profile.ask` gives the asked model one question and no tool, so a turn told to "use <profile>
//! to list a folder" ran the listing itself and handed the other model the output to format. An
//! agent with tools is a TASK, and Theseus has tasks: a session of its own, the task's tools, its
//! own spend limit carved from this conversation's, its approvals and the floor as for any task,
//! and a report. So this tool is built ON that machinery, and is no second agent loop:
//!
//! - **It opens a task now** through `task::create`, the path `task.create` and `task.fan_out`
//!   use, with the profile as the task's model (the one thing `task.create` does not let a model
//!   name). The task gets what a task started from this conversation has: nothing more. There is
//!   no budget argument: its limit is a task's default, a quarter of what this turn has left.
//! - **It waits** for the task's report through `task_wait::wait_on_tasks`, the wait `task.wait`
//!   runs, for at most [`WAIT`]; in time, the report is the tool's result (and, as for `task.wait`,
//!   a message of the turn that is read once). Not in time, the task keeps working.
//! - **The report speaks in the conversation** either way: a marker written in the frame that
//!   opens the task makes the frame that ENDS it stage one `subagent` post (the message and
//!   footer `profile.ask`'s answer has) to the place that asked, so the in-time report and the
//!   late one are the same post, once, in order before the asking turn's reply. The task keeps
//!   its own post to the forum or channel its reports go to; where that IS the place, the one
//!   message stands for both.
//! - **The wall** is `profile.ask`'s (`config_change::wall`): the owner's own turn, in a private
//!   place only they write in. A task, a routine, the self loop, an MCP session, a shared place
//!   and another participant are refused, so no agent starts agents through it. It is asked
//!   first after outside text, and whenever `task.create` itself would be asked.
//!
//! The whole design, and the table of what it can and cannot cause: `docs/design/agent-tools.md`.

use std::sync::Arc;
use std::time::Duration;

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use theseus_kernel::{usd, Action, Execution};
use theseus_protocol::SubagentAnswer;
use theseus_store::{kinds, NewRecord};
use theseus_tools::{
    parse, AsyncMediaRun, Backend, Plan, Retry, Tool, ToolClass, ToolCtx, ToolFailure,
};

use super::{refusal, Board};
use crate::node::Origin;
use crate::session::TargetRef;
use crate::store::Store;
use crate::task::{self, Report};
use crate::task_wait::{self, TurnRef, WaitAsk};
use crate::toolrun::TurnCtx;
use crate::Core;

pub const RUN: &str = "profile.run";
/// The longest the asking turn waits for the agent: a few minutes. The task goes on after it.
pub const WAIT: Duration = Duration::from_secs(300);
/// The most of the agent's report the post carries, in characters.
const POST_CHARS: usize = task::REPORT_NODE_CHARS;

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct Input {
    profile: String,
    work: String,
}

/// The profile and the work, checked.
fn input_of(input: &Value) -> Result<(String, String), String> {
    let i: Input = parse(input)?;
    let (profile, work) = (i.profile.trim(), i.work.trim());
    if profile.is_empty() || work.is_empty() {
        return Err("name a profile and say the work to do".into());
    }
    let n = work.chars().count();
    if n > task::MAX_BRIEF_CHARS {
        return Err(format!(
            "the work is {n} characters, over the {} a task takes",
            task::MAX_BRIEF_CHARS
        ));
    }
    Ok((profile.to_string(), work.to_string()))
}

/// `profile.run`.
pub struct ProfileRun;

impl Tool for ProfileRun {
    fn name(&self) -> &str {
        RUN
    }

    fn description(&self) -> &str {
        "Hand another model profile a piece of WORK to do, with tools (files, a shell, the web: what a \
         task has), and get its report. Use it for \"use <profile> to DO x\"; to ask a profile a question \
         with no tools, use profile.ask. It starts a task now on that profile with a task's own limit and \
         approvals, waits a few minutes for its report, and Theseus shows the report to the owner as \
         that profile's own message. If it is not done in time it goes on, and its report is shown when \
         it ends. Write `work` in full: the profile sees nothing else. Theseus's own command line in a shell is not \
         the way. Only for the owner, in a private place."
    }

    fn input_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "profile": {"type": "string", "description": "The profile's name, as look.theseus lists it."},
                "work": {"type": "string", "description": "The work, in full: what to do, where, and what to report."}
            },
            "required": ["profile", "work"],
            "additionalProperties": false,
        })
    }

    /// It starts work that runs on its own, as `task.create` does.
    fn class(&self) -> ToolClass {
        ToolClass::Run
    }

    fn backend(&self) -> Backend {
        Backend::Async
    }

    fn retry(&self) -> Retry {
        // A second call is a second agent, and is paid for.
        Retry::NonRepeatable
    }

    fn deadline(&self) -> Option<Duration> {
        Some(WAIT + Duration::from_secs(60))
    }

    fn plan(&self, input: &Value, _ctx: &ToolCtx) -> Result<Plan, String> {
        let (profile, _) = input_of(input)?;
        Ok(Plan {
            summary: format!("hand the profile {profile} a piece of work, as a task"),
            ..Default::default()
        })
    }

    fn run_async(&self, _input: &Value, _ctx: &ToolCtx) -> theseus_tools::AsyncRun {
        Box::pin(std::future::ready(Err(ToolFailure::new(
            "profile.run runs through the runtime, which starts the task",
        ))))
    }
}

/// What the task's end needs to speak: who was handed the work, who handed it, and where.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub(crate) struct Marker {
    pub profile: String,
    pub provider: String,
    pub model: String,
    /// The profile of the turn that handed the work over.
    pub asked_by: String,
    /// The place of the conversation that asked: where the report speaks.
    pub place: Option<String>,
    /// The conversation that asked and the turn that did, so the late message is drawn as that turn's
    /// reply is (a thread's tag in a direct message that hosts several): the post's own session is
    /// the task's.
    #[serde(default)]
    pub session: String,
    #[serde(default)]
    pub turn_id: String,
}

fn key(task_session: &str) -> String {
    format!("task.run.{task_session}")
}

/// The record that marks `task_session` as work handed to the profile `on`, written in the frame
/// that opens the task.
pub(crate) fn marker(
    task_session: &str,
    on: &TargetRef,
    tc: &TurnCtx<'_>,
) -> anyhow::Result<NewRecord> {
    let m = Marker {
        profile: on.profile.clone(),
        provider: on.provider.clone(),
        model: on.model.clone(),
        asked_by: tc.target.map(|t| t.profile.clone()).unwrap_or_default(),
        place: tc.outbox.target(tc.session_id),
        session: tc.session_id.to_string(),
        turn_id: tc.turn_id.to_string(),
    };
    NewRecord::json(kinds::META, Some(&key(task_session)), &m)
}

/// The marker of a task handed to a profile, if it is one.
pub(crate) fn read(store: &Store, task_session: &str) -> Option<Marker> {
    store.get_meta::<Marker>(&key(task_session)).ok().flatten()
}

/// The tasks of `execution` that a turn handed to a profile and that are still working, for a stop to
/// say (`theseus_protocol::StopAgent`): each one's profile, the model, and the id its own cancel takes.
pub(crate) fn agents_working(
    kernel: &theseus_kernel::Kernel,
    store: &Store,
    execution: &str,
) -> Vec<theseus_protocol::StopAgent> {
    let Ok(tasks) = kernel.tasks(Some(execution)) else {
        return vec![];
    };
    tasks
        .iter()
        .filter(|t| !t.state.is_terminal())
        .filter_map(|t| {
            let m = read(store, &t.session_id)?;
            Some(theseus_protocol::StopAgent {
                profile: m.profile,
                model: m.model,
                task: task::short(&t.session_id),
            })
        })
        .collect()
}

/// The post that makes the end of a handed task speak in the conversation.
pub(crate) struct Spoke {
    pub post: Action,
    pub records: Vec<NewRecord>,
    /// Where it went.
    pub place: String,
}

/// The subagent's message for the ended task `e`, staged in the frame that ends it: none for a
/// task that was not handed to a profile, or one that asked from nowhere a post can go.
pub(crate) fn speak(
    outbox: &crate::outbox::Outbox,
    store: &Store,
    e: &Execution,
    report: &Report,
) -> anyhow::Result<Option<Spoke>> {
    let Some(m) = read(store, &e.session_id) else {
        return Ok(None);
    };
    let Some(place) = m.place.clone() else {
        return Ok(None);
    };
    let ran = report.ran_on.as_ref();
    let said = match (report.outcome, &report.text, &report.reason) {
        (_, Some(text), _) => crate::scrub::safe::head(text, POST_CHARS).0.to_string(),
        ("cancelled", _, why) => format!(
            "The agent was cancelled before it reported{}.",
            why.as_deref().map(|w| format!(": {w}")).unwrap_or_default()
        ),
        (_, _, why) => format!(
            "The agent did not finish{}.",
            why.as_deref().map(|w| format!(": {w}")).unwrap_or_default()
        ),
    };
    let answer = SubagentAnswer {
        profile: m.profile,
        provider: ran.map_or(m.provider, |r| r.provider.clone()),
        model: ran.map_or(m.model, |r| r.model.clone()),
        asked_by: m.asked_by,
        text: said,
        cost_usd: Some(theseus_kernel::micros_to_usd(report.spent_micros)),
        elapsed_ms: report.elapsed_ms,
        output_tokens: 0,
        cut: false,
        call: e.session_id.clone(),
        outcome: match report.outcome {
            "complete" => String::new(),
            other => other.to_string(),
        },
    };
    let asked = |own: &String, task: &String| match own.is_empty() {
        true => task.clone(),
        false => own.clone(),
    };
    let body = json!({
        "kind": "subagent",
        "turn_id": m.turn_id,
        "session_id": asked(&m.session, &e.session_id),
        "answer": answer,
        "reply_to": Value::Null,
    });
    let (post, records) = outbox.stage(&e.session_id, &e.id, &place, body)?;
    Ok(Some(Spoke {
        post,
        records,
        place,
    }))
}

/// The owner's message that asked for this work: the one of the turn the call was made in (a call
/// the owner approved runs in a later turn, but its call node belongs to the turn it was made in).
fn owner_message(tc: &TurnCtx<'_>, call_id: &str) -> Result<String, String> {
    let nodes = tc
        .store
        .transcript(tc.session_id)
        .map_err(|e| format!("Not started: {e:#}"))?;
    let made_in = task::holder_of(&nodes, call_id)
        .and_then(|reply| nodes.iter().find(|(_, n)| n.id == reply))
        .and_then(|(_, n)| n.turn_id.clone())
        .unwrap_or_else(|| tc.turn_id.to_string());
    nodes
        .iter()
        .rev()
        .find(|(_, n)| {
            n.turn_id.as_deref() == Some(made_in.as_str())
                && n.kind == crate::stub::Kind::UserMessage
                && n.origin == Origin::Operator
        })
        .map(|(_, n)| n.id.clone())
        .ok_or_else(|| {
            "Not started: no message of the owner's started this turn, and an agent is handed work \
             only on the owner's word."
                .into()
        })
}

/// The target `profile` names, or why not, in the layer's own words: it must exist.
fn target_of(core: &Core, profile: &str) -> Result<TargetRef, String> {
    let gone = (!core.cfg.all_profiles().contains_key(profile))
        .then(|| crate::config::renamed::renamed_profile(profile))
        .flatten();
    if let Some(why) = gone.or_else(|| core.runner.profile_refusal(profile)) {
        return Err(why);
    }
    core.tools.profiles.get(profile).cloned().ok_or_else(|| {
        crate::config::renamed::or_unknown(profile, || {
            format!(
                "no profile is named `{profile}`; configured: {}",
                core.tools
                    .profiles
                    .keys()
                    .cloned()
                    .collect::<Vec<_>>()
                    .join(", ")
            )
        })
    })
}

fn fail(why: impl Into<String>) -> AsyncMediaRun {
    let f = ToolFailure::new(why.into());
    Box::pin(std::future::ready(Err(f)))
}

impl Board {
    /// The call, behind the wall: open the task now on the profile, then wait for its report.
    pub fn run_work(&self, tc: &TurnCtx<'_>, call_id: &str, input: &Value) -> AsyncMediaRun {
        if let Some(why) = refusal(tc, RUN) {
            return fail(why);
        }
        let Some(core) = self.core() else {
            return fail("profile.run is not ready: the core is not built");
        };
        match open(&core, tc, call_id, input) {
            Err(why) => fail(why),
            Ok((id, said, handed)) => {
                let left = crate::turn::clock_step::left_in_force(tc.session_id);
                let asked = task_wait::clamp_timeout(&core.cfg, Some(WAIT.as_secs()), left);
                let ask = WaitAsk {
                    tasks: vec![id],
                    timeout: asked.min(WAIT),
                    report_chars: task_wait::REPORT_CHARS,
                    fan_out: None,
                    turn: TurnRef::of(tc),
                };
                let session = tc.session_id.to_string();
                let core: Arc<Core> = core;
                Box::pin(async move {
                    let (mut out, external, media) =
                        task_wait::wait_on_tasks(core, session, ask).await?;
                    let still = out.meta["timed_out"].as_u64().unwrap_or(0) > 0;
                    let tail = match still {
                        true => {
                            "\nThe agent is still working. Its report is shown to the owner as \
                                 the agent's own message when it ends, and you read it at this \
                                 conversation's next turn."
                                .to_string()
                        }
                        false => "\nTheseus shows the report to the owner as that profile's own \
                                  message, so your reply need not repeat it."
                            .to_string(),
                    };
                    out.text = format!("{said}\n{}{tail}", out.text);
                    out.meta["handed"] = handed;
                    Ok((out, external, media))
                })
            }
        }
    }
}

/// A hand-over opens a task, so the place's ceiling must offer one: a ceiling that lists the `profile`
/// family and not `task` does not let a task start here, by this tool either. The task's own turns have
/// the place's ceiling as any task's do (`TurnRunner::view_of`: a task speaks in its parent's place).
fn allowed_here(tc: &TurnCtx<'_>) -> Result<(), String> {
    match tc.ceiling.and_then(|c| c.refusal(task::CREATE)) {
        None => Ok(()),
        Some(why) => Err(format!(
            "Not started: profile.run opens a task, and this place does not allow one ({why}). \
             Nothing was started."
        )),
    }
}

/// The floor `task.fan_out` keeps (`call_floor`, the same function): a task whose default share of what
/// this conversation has left cannot afford one call of the asked profile's model is refused up front,
/// in words that say what is left and what one call needs, and nothing is opened.
fn afford(core: &Core, tc: &TurnCtx<'_>, profile: &str, on: &TargetRef) -> Result<(), String> {
    // A model with no price cannot be reserved for: `profile.ask` refuses it up front, in these words.
    if core.catalog.get(&on.model).is_none() {
        return Err(super::ask::unpriced(&on.model));
    }
    let parent = tc
        .kernel
        .execution(tc.execution_id)
        .map_err(|e| format!("{e:#}"))?
        .ok_or("this session has no execution")?;
    let carve = task::default_carve(tc.kernel, &parent);
    let floor = crate::task_fan_out::call_floor(core, &on.model);
    if carve >= floor {
        return Ok(());
    }
    Err(format!(
        "Not started: this conversation has {} left, so a task's share of it is {}, and one call of \
         {profile}'s model ({}) needs at least {}. Nothing was started. Hand the work to a profile that \
         costs less, or do it when this conversation has more left.",
        usd(parent.budget.available()),
        usd(carve),
        on.model,
        usd(floor)
    ))
}

/// Open the task on the profile; its execution id and the words that say so.
fn open(
    core: &Core,
    tc: &TurnCtx<'_>,
    call_id: &str,
    input: &Value,
) -> Result<(String, String, Value), String> {
    let (profile, work) = input_of(input)?;
    crate::task_lineage::check_depth(tc.kernel, tc.execution_id)?;
    allowed_here(tc)?;
    let on = target_of(core, &profile)?;
    afford(core, tc, &profile, &on)?;
    let node = owner_message(tc, call_id)?;
    // The task's input is what `task.create` takes, less everything a model may not set here: no
    // budget (a task's default), no wake, no start time, no profile of its own.
    let task_input = json!({
        "brief": work,
        "title": format!("work for {profile}"),
        "arrangement": {"pieces": [{"node": node, "role": "objective"}]},
        "fidelity_ack": true,
    });
    let done = task::create(
        tc,
        &task_input,
        (call_id, call_id, None),
        (
            &core.tools.profiles,
            (&core.tools.routines, &core.tools.projects),
        ),
        &core.tools.forecasts,
        Some(&on),
    )?;
    let get = |k: &str| done.meta[k].as_str().unwrap_or_default().to_string();
    let said = format!(
        "Handed the work to {profile} (model {}, provider {}) as task {}, with a limit of ${:.2}. It \
         works with tools, as any task does.",
        on.model,
        on.provider,
        get("short"),
        done.meta["limit_usd"].as_f64().unwrap_or(0.0)
    );
    let handed = json!({"profile": profile, "provider": on.provider, "model": on.model, "task": get("task_id")});
    Ok((get("execution_id"), said, handed))
}
