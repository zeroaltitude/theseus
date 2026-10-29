//! `task.fan_out` (theseus-z9mze.3): split work that divides by area into
//! helpers that run at once, each in a fresh context, and bring their findings
//! back in the same turn. The owner's bug hunt of 2026-10-24 showed why: the
//! rival harnesses ran five to thirteen helpers a trial, each reading one area,
//! where one context holding the whole codebase sawtoothed through clearing.
//!
//! - **The helpers** are tasks opened through `task::create`, the one path
//!   `task.create` uses, helper *i* of the call `act_X` as `exe_Xh<i>` in
//!   `ses_Xh<i>`, each with `Opening::helper` set to the call's id. A call run
//!   again after a crash finds its helpers (`opened == false`) and opens no
//!   second set. Each reads the turn's own input message as its objective (the
//!   model quotes nothing), its brief wrapped with one fixed report format.
//! - **There is no count cap**, on purpose. The owner (2026-10-24): "we should be
//!   very very lightweight on any limits to theseus tasking itself ... notify,
//!   work, and allow feedback is the model", and "All recs!" to the design's cap
//!   question: no per-turn child cap beyond admission slots and spend. A wide
//!   fan-out queues at `Class::Task`; it is refused only for the money (the
//!   floor), for depth, and for a bad input.
//! - **The money.** `budget_usd`, else half of what the session has left (half
//!   of the greater of that and its limit, under a limit that notifies), split
//!   equally. Each helper needs the floor: `[tools] helper_floor_usd`, and never
//!   less than twice the worst-case reservation of one of its calls.
//! - **Read only.** A helper's tools are the reader set (`narrow`): every tool
//!   of class `Read`, less the task, job and channel ones. A call to any other is
//!   refused in its words (`refusal`). Six helpers writing one tree, or running
//!   six builds against one target, is the failure this avoids: the parent
//!   applies the fixes and runs the tests.
//! - **The wait** is `task_wait::wait_on_tasks`, the same one `task.wait` runs.

use std::sync::Arc;

use serde::Deserialize;
use serde_json::{json, Value};
use theseus_kernel::{usd, usd_to_micros, Micros};
use theseus_tools::{
    parse, AsyncMediaRun, Backend, Plan, Retry, Tool, ToolClass, ToolCtx, ToolFailure, ToolOutput,
};

use crate::node::{Body, Origin};
use crate::task::{self, MAX_BRIEF_CHARS};
use crate::task_wait::{self, TurnRef, WaitAsk};
use crate::toolrun::TurnCtx;
use crate::Core;

/// The longest helper title, in characters.
pub const TITLE_MAX: usize = 80;
/// The context one helper call is reserved for, in tokens, when the floor is
/// worked out: the largest a helper's request grows.
pub const FLOOR_CONTEXT_TOKENS: u64 = 150_000;
/// The harness's one report-format line, added to each brief.
pub const REPORT_FORMAT: &str = "End with your report: up to 25 findings, one per line, \
    `path:line — function — the problem — the fix`; then one line `Read whole: …`, one \
    `Read in part: …`. Write findings down as you find them.";

/// One helper of the call: an area and a brief for it.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct HelperArg {
    title: String,
    brief: String,
    /// The files or directories it covers, for the size hint and the coverage
    /// line.
    #[serde(default)]
    paths: Vec<String>,
}

/// `task.fan_out`'s input.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct Args {
    helpers: Vec<HelperArg>,
    #[serde(default)]
    objective: Option<String>,
    #[serde(default)]
    budget_usd: Option<f64>,
    /// Why the model fans out, when no one asked this turn (theseus-o1h3f.1):
    /// each helper is a task started by the turn, and carries it.
    #[serde(default)]
    because: Option<String>,
    #[serde(default = "yes")]
    wait: bool,
    #[serde(default)]
    timeout_secs: Option<u64>,
    #[serde(default)]
    report_chars: Option<usize>,
}

fn yes() -> bool {
    true
}

/// The toollet side of `task.fan_out`: its name, schema and plan. The core runs
/// it (`run`), as it runs `task.wait`.
pub struct TaskFanOut;

impl Tool for TaskFanOut {
    fn name(&self) -> &'static str {
        task::FAN_OUT
    }

    fn description(&self) -> &'static str {
        "Split work that divides by area into helpers that run at once, each with a fresh \
         context, and get their findings back in this turn. Give each helper one area (a crate, a \
         directory, a concern) and a brief that names its files and what to look for. Helpers read \
         and report; they do not edit or run programs. You apply the fixes from their findings: \
         read only the lines you change, not every file a helper named, or you rebuild the \
         context the helpers were there to keep small. With `wait: false` it returns at once and \
         `task_wait` collects them later."
    }

    fn input_schema(&self) -> Value {
        json!({"type": "object", "required": ["helpers"], "additionalProperties": false,
        "properties": {
            "helpers": {"type": "array", "minItems": 2, "items": {
                "type": "object", "required": ["title", "brief"], "additionalProperties": false,
                "properties": {
                    "title": {"type": "string", "maxLength": TITLE_MAX},
                    "brief": {"type": "string", "description": "its files, and what to look for"},
                    "paths": {"type": "array", "items": {"type": "string"}}
                }}},
            "objective": {"type": "string", "description": "the shared goal (default: this turn's message)"},
            "because": {"type": "string", "maxLength": 160, "description": "one line, why you start them, when no one asked this turn (a wake, a report)"},
            "budget_usd": {"type": "number", "exclusiveMinimum": 0,
                "description": "the total for all helpers, split equally (default: half of what this session has left)"},
            "wait": {"type": "boolean", "description": "wait for them here (default true)"},
            "timeout_secs": {"type": "integer", "minimum": 1},
            "report_chars": {"type": "integer", "minimum": 1000, "maximum": task::REPORT_NODE_CHARS}
        }})
    }

    /// It starts work that runs on its own, and runs alone in a batch.
    fn class(&self) -> ToolClass {
        ToolClass::Run
    }

    fn backend(&self) -> Backend {
        Backend::Async
    }

    /// Run again after a crash, it finds its helpers (`Opening::opened`).
    fn retry(&self) -> Retry {
        Retry::SafeToRepeat
    }

    fn deadline(&self) -> Option<std::time::Duration> {
        Some(std::time::Duration::from_secs(
            theseus_tools::jobs::WAIT_SECS_MAX + 30,
        ))
    }

    fn plan(&self, input: &Value, _ctx: &ToolCtx) -> Result<Plan, String> {
        let a: Args = parse(input)?;
        let titles: Vec<&str> = a.helpers.iter().map(|h| h.title.as_str()).collect();
        Ok(Plan {
            summary: format!(
                "start {} helpers that read and report: {}",
                titles.len(),
                titles.join("; ")
            ),
            ..Default::default()
        })
    }

    fn run_async(&self, _: &Value, _: &ToolCtx) -> theseus_tools::AsyncRun {
        Box::pin(std::future::ready(Err(ToolFailure::new(
            "task.fan_out runs through the core",
        ))))
    }
}

/// Whether a tool is in a helper's reader set: a read, less the tools that
/// start, wait for or close tasks (a helper starts none), jobs, and the
/// glides.
pub(crate) fn reads(class: ToolClass, name: &str) -> bool {
    class == ToolClass::Read
        && ![
            "task_", "task.", "job_", "job.", "channel_", "channel.", "term_", "term.", "plan_",
            "plan.",
        ]
        .iter()
        .any(|p| name.starts_with(p))
}

/// Whether the execution of `session` is a helper, from the call that grouped
/// it.
pub(crate) fn is_helper(
    store: &crate::store::Store,
    kernel: &theseus_kernel::Kernel,
    session: &str,
) -> bool {
    let Some(rec) = store
        .get_session::<crate::session::SessionRecord>(session)
        .ok()
        .flatten()
    else {
        return false;
    };
    rec.task.is_some()
        && rec
            .execution_id
            .and_then(|id| kernel.execution(&id).ok().flatten())
            .is_some_and(|e| e.helper_of().is_some())
}

impl crate::turn::TurnRunner {
    /// A helper's request offers its reader set and no more (`reads`): a spec of
    /// any other session is left as it is.
    pub(crate) fn helper_spec(&self, spec: &mut crate::compiler::RequestSpec, session: &str) {
        if !is_helper(&self.store, &self.kernel, session) {
            return;
        }
        let registry = &self.tools.registry;
        spec.tools = spec.tools.reduced("helper_reader", |all| {
            let keep = |d: &&Value| {
                let wire = d["name"].as_str().unwrap_or_default();
                wire == crate::tools_defer::FIND
                    || registry
                        .by_wire(wire)
                        .is_some_and(|t| reads(t.class(), t.name()))
            };
            all.iter().filter(keep).cloned().collect()
        });
    }

    /// The target of a helper's turns: its output capped at `[tools]
    /// helper_max_output_tokens`, so the worst case one call reserves is small
    /// enough for a small carve to be workable.
    pub(crate) fn helper_target(
        &self,
        s: &crate::session::SessionRecord,
        mut t: crate::turn::Target,
    ) -> crate::turn::Target {
        if is_helper(&self.store, &self.kernel, &s.session_id) {
            t.max_tokens = t.max_tokens.min(self.cfg.tools.helper_max_output_tokens);
        }
        t
    }
}

/// Why a helper's session may not run `tool`, in words the model reads: a
/// helper reads and reports.
pub(crate) fn refusal(tool: &dyn Tool) -> Option<String> {
    (!reads(tool.class(), tool.name())).then(|| {
        format!(
            "{} is not for a helper: you read and report, and the conversation that started you \
             applies fixes and runs programs. Put what you found in your report.",
            tool.wire_name()
        )
    })
}

fn fail(why: impl Into<String>) -> AsyncMediaRun {
    let f = ToolFailure::new(why.into());
    Box::pin(std::future::ready(Err(f)))
}

/// What one helper call may be reserved at, doubled: the least a helper's carve
/// may be, whatever `[tools] helper_floor_usd` says.
pub(crate) fn call_floor(core: &Core, model: &str) -> Micros {
    let max = core.cfg.tools.helper_max_output_tokens;
    core.catalog.get(model).map_or(0, |price| {
        2 * price.reserve_micros(max, FLOOR_CONTEXT_TOKENS)
    })
}

/// The refusals of a bad input, each in words that name what was wrong.
fn check(a: &Args) -> Result<(), String> {
    if a.helpers.len() < 2 {
        return Err(format!(
            "Not started: task_fan_out needs at least 2 helpers, one per area (you gave {}); one \
             area is a task_create.",
            a.helpers.len()
        ));
    }
    for (n, h) in a.helpers.iter().enumerate() {
        let n = n + 1;
        if h.title.trim().is_empty() || h.title.chars().count() > TITLE_MAX {
            return Err(format!(
                "Not started: helper {n}'s title is {} characters; give one of 1 to {TITLE_MAX}.",
                h.title.chars().count()
            ));
        }
        let len = h.brief.trim().chars().count();
        if len == 0 {
            return Err(format!(
                "Not started: helper {n} (\"{}\") has an empty brief: name its files and what to \
                 look for.",
                h.title
            ));
        }
        if len > MAX_BRIEF_CHARS {
            return Err(format!(
                "Not started: helper {n}'s brief is {len} characters, over the {MAX_BRIEF_CHARS} \
                 a task takes."
            ));
        }
    }
    Ok(())
}

/// The brief a helper reads: the area's, then the covered paths, then the one
/// report format.
fn wrapped(h: &HelperArg) -> String {
    let paths = match h.paths.is_empty() {
        true => String::new(),
        false => format!("\n\nYour area: {}.", h.paths.join(", ")),
    };
    format!("{}{paths}\n\n{REPORT_FORMAT}", h.brief.trim())
}

/// The message of this turn that asked for the work, the objective each helper
/// quotes by id: the operator's, or, in a turn a wake or a report started, the
/// message that started it. Never an earlier exchange's.
fn input_node(tc: &TurnCtx<'_>) -> Result<String, String> {
    let nodes = tc
        .store
        .transcript(tc.session_id)
        .map_err(|e| format!("Not started: {e:#}"))?;
    let here: Vec<_> = nodes
        .iter()
        .filter(|(_, n)| {
            n.turn_id.as_deref() == Some(tc.turn_id) && matches!(n.body, Body::UserMessage { .. })
        })
        .collect();
    here.iter()
        .rev()
        .find(|(_, n)| n.origin == Origin::Operator)
        .or(here.last())
        .map(|(_, n)| n.id.clone())
        .ok_or_else(|| {
            "Not started: no message of this turn asked for this work; start the helpers \
             with task_create and an arrangement instead."
                .into()
        })
}

/// `task.fan_out`'s run: the helpers opened now, in the calling turn, then the
/// wait (or the ids).
pub(crate) fn run(
    core: Arc<Core>,
    tc: &TurnCtx<'_>,
    call_id: &str,
    input: &Value,
) -> AsyncMediaRun {
    let a: Args = match parse(input) {
        Ok(a) => a,
        Err(e) => return fail(e),
    };
    if let Err(refusal) = crate::task_lineage::check_depth(tc.kernel, tc.execution_id) {
        return fail(refusal);
    }
    if let Err(e) = check(&a) {
        return fail(e);
    }
    let opened = match open_all(&core, tc, call_id, &a) {
        Ok(o) => o,
        Err(e) => return fail(e),
    };
    let ids: Vec<String> = opened.rows.iter().map(|r| r.id.clone()).collect();
    let text = opened.say();
    if !a.wait {
        let meta = json!({task_wait::FAN_OUT_KEY: call_id, "opened": opened.fresh,
            "tasks": opened.rows.iter().map(|r| json!({"id": r.id, "short": r.short,
                "title": r.title, "limit_usd": r.limit_usd})).collect::<Vec<_>>()});
        let out = ToolOutput { text, meta };
        return Box::pin(std::future::ready(Ok((out, None, None))));
    }
    let left = crate::turn::clock_step::left_in_force(tc.session_id);
    let timeout = task_wait::clamp_timeout(&core.cfg, a.timeout_secs, left);
    set_helper_deadlines(tc, &ids, timeout);
    let ask = WaitAsk {
        tasks: ids,
        timeout,
        report_chars: a
            .report_chars
            .unwrap_or(task_wait::REPORT_CHARS)
            .clamp(task_wait::REPORT_CHARS_MIN, task::REPORT_NODE_CHARS),
        fan_out: Some(call_id.to_string()),
        turn: TurnRef::of(tc),
    };
    Box::pin(task_wait::wait_on_tasks(
        core,
        tc.session_id.to_string(),
        ask,
    ))
}

/// How long before the wait runs out each helper must have reported: its time line counts down to
/// the wait's timeout less this, and the progress check's stop asks for its report at 85 % of that.
/// Switched on once the progress steer stopped nagging read-only work (theseus-ikr75): until then
/// a helper told "the smallest step that can pass" at half its time did the wrong thing.
pub const HELPER_REPORT_BEFORE: std::time::Duration = std::time::Duration::from_secs(60);

/// Give each waited helper a deadline of the wait's timeout less `HELPER_REPORT_BEFORE`, so it ends
/// and reports inside the wait instead of being cut off by it. A wait too short to leave a helper
/// half a minute after that sets none: those helpers run to their carve and the wait's timeout
/// returns what they have.
fn set_helper_deadlines(tc: &TurnCtx<'_>, ids: &[String], timeout: std::time::Duration) {
    let Some(span) = timeout.checked_sub(HELPER_REPORT_BEFORE) else {
        return;
    };
    if span < std::time::Duration::from_secs(30) {
        return;
    }
    let at = std::time::Instant::now() + span;
    for id in ids {
        if let Some(e) = tc.kernel.execution(id).ok().flatten() {
            crate::turn::clock_step::set_helper_deadline(&e.session_id, at);
        }
    }
}

/// A helper opened by this call.
struct Row {
    id: String,
    short: String,
    title: String,
    limit_usd: f64,
}

/// What the opening did.
struct Opened {
    rows: Vec<Row>,
    /// Opened by this call, not found from an earlier run of it.
    fresh: bool,
    /// The helpers that could not be opened, and why.
    failed: Vec<String>,
}

impl Opened {
    fn say(&self) -> String {
        let list = self
            .rows
            .iter()
            .map(|r| format!("{} \"{}\"", r.short, r.title))
            .collect::<Vec<_>>()
            .join(", ");
        let was = match self.fresh {
            true => "started",
            false => "were already started by this call:",
        };
        let mut s = format!(
            "{} helpers {was} {list}. task_wait collects their reports.",
            self.rows.len()
        );
        for f in &self.failed {
            s.push_str(&format!("\n{f}"));
        }
        s
    }
}

/// The total for the helpers, the share of each, and the floor each needs, or
/// the refusal in words.
fn split(core: &Core, tc: &TurnCtx<'_>, a: &Args) -> Result<f64, String> {
    let parent = tc
        .kernel
        .execution(tc.execution_id)
        .map_err(|e| format!("{e:#}"))?
        .ok_or("this session has no execution")?;
    crate::task_lineage::check_depth(tc.kernel, &parent.id)?;
    let left = parent.budget.available();
    // Under a limit that notifies, half of the limit, even once the session has
    // passed it, as a task's quarter is (theseus-usei).
    let share_of = match tc.kernel.overdraws(&parent) {
        true => left.max(parent.budget.limit_micros),
        false => left,
    };
    // What the model asks for is bounded by what the session can carve, or the
    // floor would bound the fan-out against a figure the session does not have.
    let total = match a.budget_usd {
        Some(b) => usd_to_micros(b).min(share_of),
        None => share_of / 2,
    };
    let n = a.helpers.len() as u64;
    let floor = usd_to_micros(core.cfg.tools.helper_floor_usd)
        .max(call_floor(core, tc.target.map_or("", |t| t.model.as_str())));
    if total / n < floor {
        let affords = total / floor.max(1);
        return Err(format!(
            "Not started: {} carved affords {affords} helpers at the {} floor; {n} were asked \
             for. Ask for fewer, or give a larger `budget_usd`.",
            usd(total),
            usd(floor)
        ));
    }
    Ok(theseus_kernel::micros_to_usd(total / n))
}

/// Open every helper through `task::create`, in order.
fn open_all(core: &Core, tc: &TurnCtx<'_>, call_id: &str, a: &Args) -> Result<Opened, String> {
    let each = split(core, tc, a)?;
    let node = input_node(tc)?;
    let mut out = Opened {
        rows: Vec::new(),
        fresh: false,
        failed: Vec::new(),
    };
    for (n, h) in a.helpers.iter().enumerate() {
        let n = n + 1;
        let brief = wrapped(h);
        let input = task::helper_input(
            &brief,
            &h.title,
            a.objective.as_deref(),
            (&node, a.because.as_deref()),
            each,
        )?;
        let open_id = format!("{call_id}h{n}");
        let done = task::create(
            tc,
            &input,
            (call_id, &open_id, Some(call_id)),
            (
                &core.tools.profiles,
                (&core.tools.routines, &core.tools.projects),
            ),
            &core.tools.forecasts,
        );
        match done {
            Ok(d) => {
                let get = |k: &str| d.meta[k].as_str().unwrap_or_default().to_string();
                out.fresh |= d.meta["opened"].as_bool().unwrap_or(false);
                out.rows.push(Row {
                    id: get("execution_id"),
                    short: get("short"),
                    title: h.title.clone(),
                    limit_usd: d.meta["limit_usd"].as_f64().unwrap_or(0.0),
                });
            }
            Err(e) => out
                .failed
                .push(format!("Helper {n} (\"{}\") was not started: {e}", h.title)),
        }
    }
    match out.rows.is_empty() {
        true => Err(out.failed.join("\n")),
        false => Ok(out),
    }
}
