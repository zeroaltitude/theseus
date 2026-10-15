//! `watch`: the model waits on the world with no model (theseus-f2r2b; the
//! horizon row HZ6). A condition a command can check is a watch; work that
//! needs judgment on a day is a deferred task. See `watch.rs`.
//!
//! - **The harness runs it** (`Backend::Harness`): one frame, the wake's
//!   (`Kernel::set_wake_as`), exactly as `wake.at`'s; the open watch goes on
//!   the board so its first check is at the next tick.
//! - **A command is approved once, at creation, with the command shown** (the
//!   owner's decision D-H9): a `run` watch plans with an authority, so the gate
//!   asks at every posture, and the answer binds this call's exact input by
//!   digest. Nothing edits a watch afterwards (its record has no verb that
//!   does), so the command that was approved is the command that runs, in L1,
//!   every time. A `fetch` is judged as `http.fetch` judges its URL, and a
//!   `file` as a read of its path.
//! - **Not in a task**: a task must end, and a watch outlives it.
//! - **Cancelled by `wake.cancel`**, by the id the pending-wakes block shows.

use serde_json::{json, Value};
use theseus_kernel::KernelError;
use theseus_protocol::{Access, Resource, WatchCheck, WatchCreateParams};
use theseus_tools::{Backend, Plan, Retry, Tool, ToolClass, ToolCtx};

use crate::config::WatchesConfig;
use crate::toolrun::TurnCtx;
use crate::wake::{local, span};
use crate::watch::spec::{self, Spec};
use crate::watch::step::Entry;
use crate::watch::Board;

pub const WATCH: &str = "watch";

/// What a task is told when it asks for one.
pub const TASK_REFUSAL: &str = "Refused: this session is a task, and a task must end, so it \
    cannot set a watch, which outlives it. Say in your report what to watch for; the \
    conversation that started you can set it.";

pub struct WatchTool;

impl Tool for WatchTool {
    fn name(&self) -> &'static str {
        WATCH
    }

    fn description(&self) -> &'static str {
        "Check something on a timer with no model until a condition holds, then be told (\"when \
         PR 123 merges\"). `check`: {kind:run, argv, json?} (asks the owner once), {kind:fetch, \
         url, json?} or {kind:file, path}. `until`: `exit 0`, `equals X`, `contains X`, `matches \
         RE`, `changes`, `above N`, `below N`, `exists`. `then`: notice, turn, or task with a \
         `brief`. To wait for someone to answer, {kind:reply, place, from?, words?} (no `until`, no \
         `every`: it is told of each message, and only notices). End one with `wake.cancel`."
    }

    fn input_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "title": {"type": "string"},
                "check": {
                    "type": "object",
                    "properties": {
                        "kind": {"type": "string", "enum": ["run", "fetch", "file", "reply"]},
                        "argv": {"type": "array", "items": {"type": "string"}},
                        "url": {"type": "string"},
                        "path": {"type": "string"},
                        "place": {"type": "string"},
                        "from": {"type": "string"},
                        "words": {"type": "string"},
                        "json": {"type": "string"}
                    },
                    "required": ["kind"]
                },
                "until": {"type": "string"},
                "every": {"type": "string", "description": "5m by default, 1m least."},
                "for_at_most": {"type": "string", "description": "30d at most, and by default."},
                "then": {
                    "type": "object",
                    "properties": {
                        "kind": {"type": "string", "enum": ["notice", "turn", "task"]},
                        "brief": {"type": "string"}
                    },
                    "required": ["kind"]
                }
            },
            "required": ["title", "check"],
            "additionalProperties": false
        })
    }

    fn class(&self) -> ToolClass {
        ToolClass::Write
    }

    fn backend(&self) -> Backend {
        Backend::Harness
    }

    fn retry(&self) -> Retry {
        // Run again after a crash, it finds the watch it set (`wake_id`).
        Retry::NonRepeatable
    }

    fn plan(&self, input: &Value, _ctx: &ToolCtx) -> Result<Plan, String> {
        let p = params_of(input)?;
        // Every bound is checked here, so a call that cannot be set is invalid
        // input and asks nothing of the owner.
        let now = theseus_protocol::now_unix_ms();
        let built = spec::build(&p, now, &WatchesConfig::default(), None)?;
        let s = &built.spec;
        let how = format!(
            "{} every {} until {}, for at most {}, then {}",
            s.check.target(),
            crate::watch::spec::every_word(s.every_ms),
            s.until,
            span(s.expires_ms.saturating_sub(now)),
            s.then.word()
        );
        let mut plan = Plan {
            summary: format!("watch \"{}\": {} {how}", s.title, s.check.kind()),
            ..Default::default()
        };
        match &p.check {
            WatchCheck::Run { .. } => {
                plan.authority = Some(format!(
                    "a watch runs this command by itself, with no model, on a timer, until its \
                     condition holds: {how}"
                ));
            }
            WatchCheck::Reply { .. } => {
                plan.authority = Some(format!(
                    "a watch reads every new message in a place, with no model, and tells you of the one that matches: {how}"
                ));
            }
            WatchCheck::Fetch { url, .. } => plan.url = Some(url.clone()),
            WatchCheck::File { path } => {
                plan.resources = vec![Resource {
                    path: path.into(),
                    access: Access::Read,
                }];
            }
        }
        Ok(plan)
    }
}

fn params_of(input: &Value) -> Result<WatchCreateParams, String> {
    if ["session_id", "author", "discord"]
        .iter()
        .any(|k| input.get(k).is_some())
    {
        return Err("a watch is set in this conversation: `session_id`, `author` and `discord` are not inputs".into());
    }
    serde_json::from_value(input.clone()).map_err(|e| format!("invalid input: {e}"))
}

/// Run `watch` for the call `correlation_id` of the turn `tc` (the harness's
/// side): set the watch and say what it will do. An error is the result the
/// model reads.
pub fn set(
    tc: &TurnCtx<'_>,
    board: &Board,
    cfg: &WatchesConfig,
    input: &Value,
    correlation_id: &str,
) -> Result<(String, Value), String> {
    if tc.task.is_some() {
        return Err(TASK_REFUSAL.into());
    }
    let p = params_of(input)?;
    let p = crate::watch::reply::resolved(&p, tc.places.rule)?;
    let now = tc.kernel.now_ms();
    let built = spec::build(&p, now, cfg, tc.outbox.target(tc.session_id))?;
    let set = match tc.kernel.set_wake_as(tc.guard, correlation_id, built.new) {
        Ok(s) => s,
        Err(e) => {
            return Err(match e.downcast_ref::<KernelError>() {
                Some(KernelError::TooManyWakes { max }) => {
                    format!("Not set: this conversation already holds {max} check-ins")
                }
                _ => format!("Not set: {e:#}"),
            })
        }
    };
    let w = &set.wake;
    tc.commitments.set(set.list.clone());
    if set.set {
        tc.commitments.scheduled_one();
    }
    let s = crate::task::short(&w.id);
    let spec = Spec::of(w).map_err(|e| format!("Not set: {e}"))?;
    if set.set {
        board.insert(Entry::seed(
            tc.execution_id.to_string(),
            w.id.clone(),
            spec.clone(),
            w.set_at_ms,
            None,
            now,
        ));
    }
    let until = local(spec.expires_ms).full();
    let text = if set.set {
        format!(
            "Watching \"{}\" (id {s}, {}): {} every {}, until {}, giving up at {until}; then {}. \
             It looks first at once and costs no model call. Cancel it with `wake.cancel`.",
            spec.title,
            w.id,
            spec.check.target(),
            crate::watch::spec::every_word(spec.every_ms),
            spec.until,
            spec.then.word(),
        )
    } else {
        format!(
            "Watch {s} was already set by this call. Its id is {}.",
            w.id
        )
    };
    let meta = json!({
        "wake_id": w.id, "short": s, "set": set.set, "kind": spec.check.kind(),
        "every_ms": spec.every_ms, "expires_at_ms": spec.expires_ms, "then": spec.then.word(),
    });
    Ok((text, meta))
}
