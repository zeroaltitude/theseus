//! An in-turn wait on this conversation's tasks (theseus-z9mze.2): `task.wait`
//! holds the turn until the tasks it names have ended, then brings each one's
//! report into the turn that is waiting, where `turn.rs::read_reports` would
//! have brought it into the next.
//!
//! - **The result** is a short index (`INDEX_MAX_CHARS`, under the line at
//!   which a result is stubbed) and, beside it, **one relayed report node per
//!   ended task**, exactly the node `read_reports` writes: a harness message
//!   titled `task:<short>`, never a `Kind::ToolResult`, so a clearing pass can
//!   never erase a delivered finding. They are taken through
//!   `Kernel::take_reports_in_turn`, so the parent's next turn never writes
//!   them again.
//! - **What ends the wait**: every task named has ended (`JobWaits::
//!   task_settled`, with the one-second look as the backstop), a person's
//!   message, a stop (the call is aborted, as any call's is), the timeout, and
//!   nothing else. A task still running at the timeout keeps running and
//!   stays on the parent's `reports`: a note of its findings so far is written,
//!   and its report comes when it ends.
//! - **The slot.** A waiting parent lends its admission slot after
//!   `LEND_AFTER` and takes one again at its class's head, as a foreground job
//!   does; helper turns are `Class::Task`, the lowest, and a few waiting
//!   parents would otherwise starve the tasks they wait on.
//! - **The cache.** The wait appends no node while it waits and compiles
//!   nothing, so the parent's request after it has the same prefix. It reads
//!   the turn's cache again before it lapses (`keep_warm_wait`).
//! - **The fan-in is bounded** so that N reports arriving at once cannot push
//!   a long conversation over its clearing line: each report to
//!   `report_chars`, all of them to `[tools] fan_in_chars` and to half the room
//!   before the clearing line (`KeepWarm::room`, recorded when the response
//!   that called the wait was sent); past that each report is cut to its share,
//!   and its whole text is kept in the session's outputs.
//!
//! `task.fan_out` starts helpers and then calls [`wait_on_tasks`] with the ones
//! it opened, so there is one wait.

use std::pin::Pin;
use std::sync::Arc;
use std::time::Duration;

use serde::Deserialize;
use serde_json::{json, Value};
use theseus_kernel::{micros_to_usd, ExecState, Execution, Kernel, Micros};
use theseus_store::NewRecord;
use theseus_tools::{
    parse, AsyncMediaResult, AsyncMediaRun, Backend, Plan, Retry, Tool, ToolClass, ToolCtx,
    ToolFailure, ToolOutput,
};
use tokio::time::Instant;

use crate::fact::{self, To};
use crate::narrative;
use crate::node::{Body, Node, Origin};
use crate::session::SessionRecord;
use crate::store::Store;
use crate::task::{self, Report};
use crate::toolrun::jobs_tools::INTERRUPTED_BY;
use crate::toolrun::TurnCtx;
use crate::turn::admission::LEND_AFTER;
use crate::Core;

/// The longest index the wait's result carries, in characters: under
/// `compiler::clearing::MIN_CHARS`, so the clearing pass never stubs it.
pub const INDEX_MAX_CHARS: usize = 2_000;
/// A report's room in the turn, in characters, when the call names none.
pub const REPORT_CHARS: usize = 6_000;
/// The least and the most a call may ask of one report.
pub const REPORT_CHARS_MIN: usize = 1_000;
/// The shortest a wait is allowed to be.
pub const TIMEOUT_MIN_SECS: u64 = 30;
/// The meta key of the call that opened the tasks, when one did.
pub const FAN_OUT_KEY: &str = "fan_out";
/// A task row's state words.
const RUNNING: &str = "running";
/// The reason a task that reached its carve ends with.
pub(crate) const BUDGET_REASON: &str = "budget";

/// `task.wait`'s input.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct Args {
    /// The tasks to wait on, by their ids or the ends of them.
    #[serde(default)]
    tasks: Option<Vec<String>>,
    #[serde(default)]
    timeout_secs: Option<u64>,
    #[serde(default)]
    report_chars: Option<usize>,
}

/// The toollet side of `task.wait`: its name, schema and plan. The core runs
/// it (`run`), as it runs a job's handle.
pub struct TaskWait;

impl Tool for TaskWait {
    fn name(&self) -> &'static str {
        task::WAIT
    }

    fn description(&self) -> &'static str {
        "Wait in this turn for tasks this conversation started, and get each one's report. It \
         returns when all have ended, at the timeout, or when a message arrives. A task still \
         running at the timeout keeps running; wait again or close it with `task_close`."
    }

    fn input_schema(&self) -> Value {
        json!({"type": "object", "additionalProperties": false,
        "properties": {
            "tasks": {"type": "array", "items": {"type": "string"},
                "description": "the tasks to wait on, by id or the end of it (default: every task of this conversation that is running or has a report you have not read)"},
            "timeout_secs": {"type": "integer", "minimum": 1,
                "description": "how long to wait, from 30 s to the configured longest wait (default 900)"},
            "report_chars": {"type": "integer", "minimum": REPORT_CHARS_MIN, "maximum": task::REPORT_NODE_CHARS,
                "description": "the most of one report to bring in (default 6000)"}
        }})
    }

    fn class(&self) -> ToolClass {
        ToolClass::Read
    }

    fn backend(&self) -> Backend {
        Backend::Async
    }

    fn retry(&self) -> Retry {
        Retry::SafeToRepeat
    }

    /// Past the longest wait any config allows, and a margin.
    fn deadline(&self) -> Option<Duration> {
        Some(Duration::from_secs(theseus_tools::jobs::WAIT_SECS_MAX + 30))
    }

    fn plan(&self, input: &Value, _ctx: &ToolCtx) -> Result<Plan, String> {
        let a: Args = parse(input)?;
        let what = match a.tasks.as_deref() {
            Some([one]) => format!("wait for task {one}"),
            Some(many) if !many.is_empty() => format!("wait for {} tasks", many.len()),
            _ => "wait for this conversation's tasks".into(),
        };
        Ok(Plan {
            summary: what,
            ..Default::default()
        })
    }

    fn run_async(&self, _: &Value, _: &ToolCtx) -> theseus_tools::AsyncRun {
        Box::pin(std::future::ready(Err(ToolFailure::new(
            "task.wait runs through the core",
        ))))
    }
}

/// Register the task tools the harness runs: `task.create` and `task.wait`.
pub(crate) fn register(r: &mut theseus_tools::Registry) {
    r.register(Arc::new(task::TaskCreate));
    r.register(Arc::new(TaskWait));
    r.register(Arc::new(crate::task_fan_out::TaskFanOut));
}

/// Whether a call of `tool` runs alone in a batch: it holds the turn, and a
/// call beside it would run while the turn waits.
pub(crate) fn alone(tool: &str) -> bool {
    tool == task::WAIT || tool == task::FAN_OUT
}

/// The tools that wait on a conversation's tasks or fan work out to them: a
/// shared place is not offered them (`places::public_tool`). Its first request
/// holds every public tool, and the shared channel's is at its budget; the
/// owner's fan-out is for the CLI, headless and work sessions.
pub(crate) fn helper_tool(name: &str) -> bool {
    name == task::WAIT || name == task::FAN_OUT
}

/// Whether `name` (a response's wire name) is a call of the wait or of the
/// fan-out that waits.
pub(crate) fn is_task_wait_wire(name: &str) -> bool {
    name == theseus_tools::wire_name(task::WAIT) || name == theseus_tools::wire_name(task::FAN_OUT)
}

impl crate::toolrun::ToolRuntime {
    /// Whether an async tool of this family runs through the core: the jobs' and
    /// the tasks'.
    pub(crate) fn core(family: &str) -> bool {
        family == theseus_tools::jobs::FAMILY || family == task::FAMILY
    }
}

/// The turn a wait runs in, held by value: the wait runs in its own task, so
/// it cannot hold the turn's context, and it writes through the turn's own
/// handles so that its nodes join the turn's transcript.
pub(crate) struct TurnRef {
    execution_id: String,
    turn: u64,
    turn_id: String,
    store: Store,
    kernel: Kernel,
}

impl TurnRef {
    /// The turn `tc` is running.
    pub(crate) fn of(tc: &TurnCtx<'_>) -> Self {
        Self {
            execution_id: tc.execution_id.to_string(),
            turn: tc.guard.turn,
            turn_id: tc.turn_id.to_string(),
            store: tc.store.clone(),
            kernel: tc.kernel.view(tc.store.shared()),
        }
    }
}

/// What a wait on this session's tasks was asked for. `task.fan_out` builds
/// one of these for the helpers it just opened (theseus-z9mze.3).
pub(crate) struct WaitAsk {
    /// The tasks to wait on, already resolved to execution ids.
    pub tasks: Vec<String>,
    /// Already clamped by the caller's own rule, or by [`clamp_timeout`].
    pub timeout: Duration,
    /// Per report, `REPORT_CHARS_MIN..=REPORT_NODE_CHARS`.
    pub report_chars: usize,
    /// The fan-out call's correlation id, when this wait came from one: it
    /// goes in the index's `fan_out` meta and groups the rows.
    pub fan_out: Option<String>,
    /// The turn that waits.
    pub turn: TurnRef,
}

/// The wait's length: what was asked (or `[tools] fan_out_wait_secs`), from
/// 30 s to `[tools] fan_out_max_wait_secs`; and when the turn has a deadline,
/// no more than the time left less `max(60 s, 15 %)`, so the turn can still
/// apply what the tasks found.
pub(crate) fn clamp_timeout(
    cfg: &crate::config::Config,
    asked: Option<u64>,
    deadline_left: Option<Duration>,
) -> Duration {
    let t = &cfg.tools;
    let max = t.fan_out_max_wait_secs.max(TIMEOUT_MIN_SECS);
    let secs = asked
        .unwrap_or(t.fan_out_wait_secs)
        .clamp(TIMEOUT_MIN_SECS, max);
    let wait = Duration::from_secs(secs);
    match deadline_left {
        Some(left) => {
            let keep = Duration::from_secs(60).max(left.mul_f64(0.15));
            wait.min(left.saturating_sub(keep))
        }
        None => wait,
    }
}

fn fail(why: impl Into<String>) -> AsyncMediaRun {
    let f = ToolFailure::new(why.into());
    Box::pin(std::future::ready(Err(f)))
}

/// `task.wait`'s run, in its call's own task: the tasks resolved now, in the
/// calling turn, and the wait after.
pub(crate) fn run(core: Arc<Core>, tc: &TurnCtx<'_>, input: &Value) -> AsyncMediaRun {
    let a: Args = match parse(input) {
        Ok(a) => a,
        Err(e) => return fail(e),
    };
    if let Err(refusal) = crate::task_lineage::check_depth(tc.kernel, tc.execution_id) {
        return fail(refusal);
    }
    let ids = match chosen(tc, a.tasks.as_deref()) {
        Ok(ids) => ids,
        Err(e) => return fail(e),
    };
    if ids.is_empty() {
        let text = "No task of this conversation is running, and none has a report you have not \
                    read.";
        let out = ToolOutput {
            text: text.into(),
            meta: json!({"tasks": [], "waited_ms": 0, "timed_out": 0}),
        };
        return Box::pin(std::future::ready(Ok((out, None, None))));
    }
    let left = crate::turn::clock_step::left_in_force(tc.session_id);
    let ask = WaitAsk {
        tasks: ids,
        timeout: clamp_timeout(&core.cfg, a.timeout_secs, left),
        report_chars: a
            .report_chars
            .unwrap_or(REPORT_CHARS)
            .clamp(REPORT_CHARS_MIN, task::REPORT_NODE_CHARS),
        fan_out: None,
        turn: TurnRef::of(tc),
    };
    Box::pin(wait_on_tasks(core, tc.session_id.to_string(), ask))
}

/// The execution ids a call waits on: the ones it names, resolved among this
/// conversation's tasks; else every task that runs or has a report not yet read.
fn chosen(tc: &TurnCtx<'_>, named: Option<&[String]>) -> Result<Vec<String>, String> {
    let tasks = tc
        .kernel
        .tasks(Some(tc.execution_id))
        .map_err(|e| format!("this conversation's tasks could not be read: {e}"))?;
    let Some(named) = named.filter(|n| !n.is_empty()) else {
        let unread = tc
            .kernel
            .execution(tc.execution_id)
            .ok()
            .flatten()
            .map(|e| e.reports)
            .unwrap_or_default();
        return Ok(tasks
            .iter()
            .filter(|t| !t.state.is_terminal() || unread.contains(&t.id))
            .map(|t| t.id.clone())
            .collect());
    };
    let mut ids = Vec::new();
    for name in named {
        let id = task::resolve(&tasks, name)?.id.clone();
        if !ids.contains(&id) {
            ids.push(id);
        }
    }
    Ok(ids)
}

/// The state words of the index: `done`, `budget`, `failed`, `cancelled`, or
/// `running` for a task that has not ended.
fn state_of(e: &Execution) -> &'static str {
    match e.state {
        // A helper that reached its carve ends with the reason `budget`
        // (theseus-z9mze.3); the old state of that name reads the same.
        ExecState::Complete if e.ended_reason.as_deref() == Some(BUDGET_REASON) => "budget",
        ExecState::Complete => "done",
        ExecState::BudgetExhausted => "budget",
        ExecState::Failed => "failed",
        ExecState::Cancelled => "cancelled",
        _ => RUNNING,
    }
}

#[cfg(test)]
impl TurnRef {
    /// A turn of `exec`'s, for a test that calls the wait without a model.
    pub(crate) fn for_exec(core: &Core, exec: &Execution) -> Self {
        Self {
            execution_id: exec.id.clone(),
            turn: exec.turns,
            turn_id: "trn_test".into(),
            store: core.store.clone(),
            kernel: core.kernel.view(core.store.shared()),
        }
    }
}

/// Gives the waiter back its slot if the wait is dropped before it took it,
/// and, when the wait was a fan-out's, cancels that call's helpers if the wait
/// is dropped before it ended: a stop of the turn stops the helpers the call
/// opened (the owner, z9mze Q3: "/stop cancels waiting helpers"), and no
/// others, so a plain `task.wait` leaves its tasks running.
struct Lent {
    core: Arc<Core>,
    exec: String,
    lent: bool,
    /// The fan-out call's id and the helpers it opened, when the wait is one's.
    fan_out: Option<(String, Vec<String>)>,
    /// The wait ran to its end: a timeout leaves the helpers running.
    ended: bool,
}

impl Drop for Lent {
    fn drop(&mut self) {
        let helpers = match (&self.fan_out, self.ended) {
            (Some((call, ids)), false) => Some((call.clone(), ids.clone())),
            _ => None,
        };
        if !self.lent && helpers.is_none() {
            return;
        }
        let Ok(rt) = tokio::runtime::Handle::try_current() else {
            return;
        };
        let (core, exec, lent) = (self.core.clone(), self.exec.clone(), self.lent);
        rt.spawn(async move {
            if lent {
                core.tools.admission.retake(&exec).await;
            }
            if let Some((call, ids)) = helpers {
                cancel_helpers(&core, &call, &ids).await;
            }
        });
    }
}

/// Cancel the helpers of fan-out call `call` among `ids` that have not ended.
async fn cancel_helpers(core: &Core, call: &str, ids: &[String]) {
    for id in ids {
        let mine = core
            .kernel
            .execution(id)
            .ok()
            .flatten()
            .is_some_and(|e| !e.state.is_terminal() && e.helper_of() == Some(call));
        if mine {
            if let Err(e) = core.cancel_execution(id, "stop").await {
                tracing::warn!(error = %format!("{e:#}"), task = %id, "a stopped fan-out's helper was not cancelled");
            }
        }
    }
}

/// Wait in this turn for `ask.tasks` and deliver their reports: the index
/// result, and one relayed report node per ended task.
pub(crate) async fn wait_on_tasks(
    core: Arc<Core>,
    session_id: String,
    ask: WaitAsk,
) -> AsyncMediaResult {
    let rt = &core.tools;
    let t0 = Instant::now();
    let until = t0 + ask.timeout;
    let steer = rt.job_waits.steers.watch(&session_id);
    // Ops says "waiting on n tasks" from this row until the matching `ended` (theseus-z9mze.6).
    fact::Rec {
        narrator: &core.narrator,
        session: Some(&session_id),
        turn: Some(&ask.turn.turn_id),
        to: To::Session(&core.bus, &session_id),
        store: &ask.turn.store,
    }
    .record(&fact::fan_out::TaskWaitStarted {
        tasks: ask.tasks.len() as u32,
        fan_out: ask.fan_out.as_deref(),
        timeout_s: ask.timeout.as_secs(),
    });
    let warmth = core.runner.keep_warm.warmth(&session_id);
    let mut reading: Option<
        Pin<Box<dyn std::future::Future<Output = crate::keep_warm::Outcome> + Send + '_>>,
    > = None;
    let mut reads = 0u32;
    let mut lent = Lent {
        core: core.clone(),
        exec: ask.turn.execution_id.clone(),
        lent: false,
        fan_out: ask.fan_out.clone().map(|call| (call, ask.tasks.clone())),
        ended: false,
    };
    let mut interrupted = false;
    let mut tried = false;
    loop {
        let settled = rt.job_waits.on_task_settle();
        tokio::pin!(settled);
        settled.as_mut().enable();
        let open = unended(&core, &ask.tasks);
        if open == 0 {
            break;
        }
        if steer.pending() {
            interrupted = true;
            break;
        }
        let left = until.saturating_duration_since(Instant::now());
        if left.is_zero() {
            break;
        }
        // The slot is lent once the wait has outlasted a turn's quick look.
        let lend_in = LEND_AFTER.saturating_sub(t0.elapsed());
        if !tried && lend_in.is_zero() {
            // Nothing to lend (a turn the queue does not hold): never mind, and
            // do not ask again.
            tried = true;
            lent.lent = rt.admission.lend(&ask.turn.execution_id);
        }
        let due = warmth.as_ref().and_then(|w| w.due_before(until));
        let nap = left.min(crate::toolrun::waits::LOOK);
        let nap = if tried {
            nap
        } else {
            nap.min(lend_in.max(Duration::from_millis(1)))
        };
        tokio::select! {
            () = &mut settled => {}
            () = steer.raised() => {}
            () = tokio::time::sleep(nap) => {}
            () = async { tokio::time::sleep_until(due.unwrap_or(until)).await },
                if due.is_some() && reading.is_none() => {
                if let Some(w) = warmth.as_ref().filter(|w| w.claim(Instant::now())) {
                    reads += 1;
                    let runner = &core.runner;
                    let session = session_id.as_str();
                    reading = Some(Box::pin(async move { w.read(runner, session).await }));
                }
            }
            out = async { reading.as_mut().unwrap().await }, if reading.is_some() => {
                reading = None;
                tracing::debug!(?out, "task_wait: the turn's cache was read");
            }
        }
    }
    lent.ended = true;
    if lent.lent {
        rt.admission.retake(&ask.turn.execution_id).await;
        lent.lent = false;
    }
    let waited = t0.elapsed();
    let taken =
        theseus_store::blocking(|| deliver(&core, &session_id, &ask, waited, (reads, interrupted)));
    taken
        .map(|out| (out, None, None))
        .map_err(|e| ToolFailure::new(format!("the tasks' reports could not be brought in: {e:#}")))
}

/// How many of `ids` have not ended.
fn unended(core: &Core, ids: &[String]) -> usize {
    ids.iter()
        .filter(|id| {
            core.kernel
                .execution(id)
                .ok()
                .flatten()
                .is_some_and(|e| !e.state.is_terminal())
        })
        .count()
}

/// One task of the wait, as the index and the meta tell it.
struct Row {
    id: String,
    short: String,
    title: Option<String>,
    state: &'static str,
    spent: Micros,
    limit: Micros,
    elapsed_ms: u64,
    /// What the report says it found, or what is known so far.
    findings: Option<usize>,
    reason: Option<String>,
    /// Characters of the task's note in the turn, whether it was cut, and
    /// where its whole text is kept.
    chars: usize,
    cut: bool,
    outputs: Option<String>,
    delivered: bool,
}

/// The findings a report states: its non-empty lines, less the two lines that
/// say what the helper read.
fn findings_in(text: &str) -> usize {
    text.lines()
        .map(str::trim)
        .filter(|l| !l.is_empty() && !l.starts_with("Read whole") && !l.starts_with("Read in part"))
        .count()
}

/// What a task has said so far: its assistant messages' text, oldest first.
fn said_so_far(store: &Store, session: &str) -> Option<String> {
    let nodes = store.session_nodes(session).ok()?;
    let parts: Vec<String> = nodes
        .iter()
        .filter_map(|(_, n)| match &n.body {
            Body::AssistantMessage { blocks, .. } => {
                Some(crate::provider::text_of(blocks)).filter(|t| !t.is_empty())
            }
            _ => None,
        })
        .collect();
    (!parts.is_empty()).then(|| parts.join("\n"))
}

/// `text` cut to its newest `max` characters, at a line's edge.
fn newest(text: &str, max: usize) -> String {
    let n = text.chars().count();
    if n <= max {
        return text.to_string();
    }
    let from = text.char_indices().nth(n - max).map_or(0, |(i, _)| i);
    let tail = &text[from..];
    let tail = tail.find('\n').map_or(tail, |i| &tail[i + 1..]);
    format!("[the earliest of its findings are in its session]\n{tail}")
}

/// A report taken into the turn: its node, whether its text was cut, and where
/// its whole text is kept.
struct Delivered {
    report: Report,
    node: Node,
    cut: bool,
    outputs: Option<String>,
}

/// What a take wrote, for the facts and the rows that follow it.
#[derive(Default)]
struct Wrote {
    delivered: Vec<Delivered>,
}

/// One report as the parent's node: the node, and the edge that says it was
/// copied from the task's last message, as `read_reports` writes them.
fn relay(
    core: &Core,
    session: &str,
    turn: &TurnRef,
    (mut r, per): (Report, usize),
) -> anyhow::Result<(Vec<NewRecord>, Delivered)> {
    let (text, cut, outputs) = fit(core, session, &r, per);
    r.text = text;
    let node = Node::relayed(
        session,
        Some(&turn.turn_id),
        Origin::Harness,
        &format!("task:{}", r.short),
        &r.node_text(),
    );
    let mut records = vec![node.record()?];
    if let Some(last) = &r.node {
        let kind = crate::graph::EdgeKind::DerivedFrom;
        let edge = crate::graph::Edge::new(kind, &node.id, last, crate::graph::VIA_REPORT);
        records.push(edge.record()?);
    }
    let got = Delivered {
        report: r,
        node,
        cut,
        outputs,
    };
    Ok((records, got))
}

/// Take the reports of `ask.tasks` that have ended into the turn, in one
/// frame with their nodes, the way the parent's next turn would have.
fn take(core: &Core, session: &str, ask: &WaitAsk, total: usize) -> anyhow::Result<Wrote> {
    let turn = &ask.turn;
    let mut wrote = Wrote::default();
    turn.kernel
        .take_reports_in_turn(&turn.execution_id, turn.turn, &ask.tasks, |ids| {
            let mut reports = Vec::new();
            for id in ids {
                if let Some(mut r) = task::load_report(&turn.store, &turn.kernel, id)? {
                    // A helper that reached its carve may have said nothing
                    // as its last words: its findings so far are its report.
                    let ended_by_budget = turn
                        .kernel
                        .execution(id)?
                        .is_some_and(|e| state_of(&e) == "budget");
                    r.text = match ended_by_budget {
                        true => said_so_far(&turn.store, &r.task).or(r.text.take()),
                        false => r.text.take().or_else(|| said_so_far(&turn.store, &r.task)),
                    };
                    reports.push(r);
                }
            }
            let per = shares(&reports, ask.report_chars, total);
            let mut records = Vec::new();
            for (r, per) in reports.into_iter().zip(per) {
                let (more, got) = relay(core, session, turn, (r, per))?;
                records.extend(more);
                wrote.delivered.push(got);
            }
            // Where the reply to them goes if the session posts nowhere by
            // then, kept past this turn (theseus-4lx).
            let target = wrote
                .delivered
                .iter()
                .find_map(|d| d.report.target.as_deref());
            if let Some(target) = target {
                records.push(core.outbox.wake_target_record(session, target)?);
            }
            Ok(records)
        })?;
    Ok(wrote)
}

/// A task that has not ended, as the note the parent reads in its place.
fn running_note(store: &Store, e: &Execution, chars: usize) -> (String, Option<usize>) {
    let short = task::short(&e.session_id);
    let said = said_so_far(store, &e.session_id);
    let findings = said.as_deref().map(findings_in);
    let body = match (said, chars) {
        (None, _) => "it has said nothing yet".into(),
        (Some(_), 0) => format!("the room for notes is used; read them in task {short}'s session"),
        (Some(t), _) => newest(&t, chars),
    };
    (
        format!("[Task {short} still running: its latest findings so far]\n{body}"),
        findings,
    )
}

/// The tasks' state now: take what ended into this turn, and write the index.
fn deliver(
    core: &Core,
    session: &str,
    ask: &WaitAsk,
    waited: Duration,
    (warm_reads, interrupted): (u32, bool),
) -> anyhow::Result<ToolOutput> {
    let turn = &ask.turn;
    let room = core.runner.keep_warm.room(session);
    let (total, bound) = fan_in_bound(core, room);
    let wrote = take(core, session, ask, total)?;
    let rec = fact::Rec {
        narrator: &core.narrator,
        session: Some(session),
        turn: Some(&turn.turn_id),
        to: To::Session(&core.bus, session),
        store: &turn.store,
    };
    for d in &wrote.delivered {
        let node = &d.node;
        rec.record(&fact::turn::NodeWritten {
            session_id: session,
            node,
        });
    }
    // The reports taken in are items of the session, asked together (theseus-9ymz0).
    let items: Vec<crate::judge::content::ContentItem> = wrote
        .delivered
        .iter()
        .filter_map(|d| {
            crate::judge::content::ContentItem::of_message(
                &d.node,
                crate::content::ItemKind::TaskReport,
                "task report",
            )
        })
        .collect();
    if !items.is_empty() {
        core.runner.judge.ask_content(session, items);
    }
    // The rows of the index, in the order the tasks were named; a task still
    // running gets a note of its findings so far, from what the delivered
    // reports left of the same bound.
    let mut rows = Vec::new();
    let mut notes: Vec<Node> = Vec::new();
    let mut open: Vec<(usize, Execution)> = Vec::new();
    for id in &ask.tasks {
        let Some(e) = turn.kernel.execution(id)? else {
            continue;
        };
        let mut row = Row::of(&turn.store, &e);
        if let Some(d) = wrote
            .delivered
            .iter()
            .find(|d| d.report.execution_id == e.id)
        {
            row.findings = d.report.text.as_deref().map(findings_in);
            row.chars = d.report.node_text().chars().count();
            (row.cut, row.outputs, row.delivered) = (d.cut, d.outputs.clone(), true);
        } else if row.state == RUNNING {
            open.push((rows.len(), e));
        }
        rows.push(row);
    }
    let used: usize = rows.iter().map(|r| r.chars).sum();
    let each = split_room(total.saturating_sub(used), open.len(), ask.report_chars);
    for ((at, e), chars) in open.iter().zip(each) {
        let (text, findings) = running_note(&turn.store, e, chars);
        let row = &mut rows[*at];
        (row.findings, row.chars) = (findings, text.chars().count());
        let author = format!("task:{}", row.short);
        notes.push(Node::relayed(
            session,
            Some(&turn.turn_id),
            Origin::Harness,
            &author,
            &text,
        ));
    }
    if !notes.is_empty() {
        let records = notes
            .iter()
            .map(Node::record)
            .collect::<Result<Vec<_>, _>>()?;
        turn.store.append(&records)?;
        for node in &notes {
            rec.record(&fact::turn::NodeWritten {
                session_id: session,
                node,
            });
        }
    }
    record_ended(&rec, ask, &rows, (wrote.delivered.len(), waited));
    let figures = (total, bound, room);
    Ok(index(
        ask,
        &rows,
        waited,
        (warm_reads, interrupted),
        figures,
    ))
}

/// The wait's `task.wait.ended` row (theseus-z9mze.6): how its tasks stood when it ended.
fn record_ended(
    rec: &fact::Rec<'_>,
    ask: &WaitAsk,
    rows: &[Row],
    (delivered, waited): (usize, Duration),
) {
    let count = |s: &str| rows.iter().filter(|r| r.state == s).count() as u32;
    rec.record(&fact::fan_out::TaskWaitEnded {
        tasks: rows.len() as u32,
        delivered: delivered as u32,
        running: count(RUNNING),
        failed: count("failed"),
        cancelled: count("cancelled"),
        budget: count("budget"),
        waited_ms: waited.as_millis() as u64,
        cut: rows.iter().any(|r| r.cut),
        fan_out: ask.fan_out.as_deref(),
    });
}

impl Row {
    /// The task `e` as its row stands before its report is read.
    fn of(store: &Store, e: &Execution) -> Self {
        let state = state_of(e);
        let title = store
            .get_session::<SessionRecord>(&e.session_id)
            .ok()
            .flatten()
            .and_then(|r| r.title);
        Self {
            id: e.id.clone(),
            short: task::short(&e.session_id),
            title,
            state,
            spent: e.budget.spent_micros,
            limit: e.budget.limit_micros,
            elapsed_ms: e.updated_at_ms.saturating_sub(e.created_at_ms),
            findings: None,
            reason: (state != "done" && state != RUNNING)
                .then(|| e.ended_reason.clone())
                .flatten(),
            chars: 0,
            cut: false,
            outputs: None,
            delivered: false,
        }
    }
}

/// The room the fan-in may take, in characters, and which bound set it: the
/// configured `fan_in_chars`, or half the room before the clearing line.
pub(crate) fn fan_in_bound(core: &Core, room: Option<(u64, u64)>) -> (usize, &'static str) {
    let fixed = core.cfg.tools.fan_in_chars;
    let Some((tokens, limit)) = room.filter(|(_, l)| *l > 0) else {
        return (fixed, "fan_in_chars");
    };
    let line = limit * crate::compiler::clearing::CLEAR_AT / 100;
    let half = (line.saturating_sub(tokens) / 2 * 4) as usize;
    match half < fixed {
        true => (half, "room"),
        false => (fixed, "fan_in_chars"),
    }
}

/// What each report may take: `report_chars`, or, when the reports together
/// pass `total`, each one's share of it, never under the least a report keeps.
/// When there are so many that `total` cannot give each that least, the first
/// `total / REPORT_CHARS_MIN` keep it and the rest take none: they arrive as a
/// pointer to their whole text (`fit`), so the floor cannot defeat the bound.
fn shares(reports: &[Report], report_chars: usize, total: usize) -> Vec<usize> {
    let size = |r: &Report| {
        r.text
            .as_deref()
            .map_or(0, |t| t.chars().count().min(report_chars))
    };
    let n = reports.len();
    match reports.iter().map(size).sum::<usize>() <= total {
        true => vec![report_chars; n],
        false => split_room(total, n, report_chars),
    }
}

/// `total` characters among `n` texts of at most `cap` each: an equal share
/// when that is no less than `REPORT_CHARS_MIN`; else the first
/// `total / REPORT_CHARS_MIN` get the least and the rest nothing.
pub(crate) fn split_room(total: usize, n: usize, cap: usize) -> Vec<usize> {
    if n > 0 && total / n >= REPORT_CHARS_MIN {
        return vec![(total / n).min(cap); n];
    }
    let inline = total / REPORT_CHARS_MIN;
    (0..n)
        .map(|i| match i < inline {
            true => REPORT_CHARS_MIN.min(cap),
            false => 0,
        })
        .collect()
}

/// `r`'s text within `per` characters, and where its whole text is kept when
/// it was cut.
pub(crate) fn fit(
    core: &Core,
    session: &str,
    r: &Report,
    per: usize,
) -> (Option<String>, bool, Option<String>) {
    let Some(text) = &r.text else {
        return (None, false, None);
    };
    if text.chars().count() <= per {
        return (Some(text.clone()), false, None);
    }
    let kept = core
        .tools
        .outputs
        .path_for(session, &format!("task-{}", r.short))
        .and_then(|to| core.tools.outputs.keep_text(text, &to).ok().map(|()| to))
        .map(|p| p.display().to_string());
    if per == 0 {
        let n = text.lines().filter(|l| !l.trim().is_empty()).count();
        let n = narrative::count(n as u64, "finding", "findings");
        let msg = match &kept {
            Some(path) => format!("{n} held back, the delivered reports took the room; the whole report: fs_read {path}"),
            None => format!(
                "{n} held back, the delivered reports took the room; the whole report stays in task {}'s session",
                r.short
            ),
        };
        return (Some(msg), true, kept);
    }
    let (cut, _) = crate::toolrun::cap(text, per, |left| {
        let n = left.lines().filter(|l| !l.trim().is_empty()).count();
        match &kept {
            Some(path) => format!(
                "{} cut; the whole report: fs_read {path}",
                narrative::count(n as u64, "finding", "findings")
            ),
            None => format!(
                "{} cut; the whole report stays in task {}'s session",
                narrative::count(n as u64, "finding", "findings"),
                r.short
            ),
        }
    });
    (Some(cut), true, kept)
}

/// `6m40s`.
fn clock(ms: u64) -> String {
    let s = ms / 1000;
    match s {
        0..=59 => format!("{s}s"),
        _ => format!("{}m{:02}s", s / 60, s % 60),
    }
}

/// The index: a head line that counts every state, one row a task, and the
/// meta with the same facts.
fn index(
    ask: &WaitAsk,
    rows: &[Row],
    waited: Duration,
    (warm_reads, interrupted): (u32, bool),
    (total, bound, room): (usize, &str, Option<(u64, u64)>),
) -> ToolOutput {
    let count = |s: &str| rows.iter().filter(|r| r.state == s).count();
    let (done, running) = (count("done"), count(RUNNING));
    let word = match ask.fan_out {
        Some(_) => "helpers",
        None => "tasks",
    };
    let mut head = format!("{done} of {} {word} reported", rows.len());
    for (n, what) in [
        (running, "still running".to_string()),
        (count("budget"), "ended at its budget".into()),
        (count("failed"), "failed".into()),
        (count("cancelled"), "cancelled".into()),
    ] {
        if n > 0 {
            head.push_str(&format!(", {n} {what}"));
        }
    }
    if interrupted {
        head.push_str("; a message arrived, so the wait ended early");
    }
    let spent: Micros = rows.iter().map(|r| r.spent).sum();
    let limit: Micros = rows.iter().map(|r| r.limit).sum();
    head.push_str(&format!(
        " (waited {}, {} of {} carved).",
        clock(waited.as_millis() as u64),
        narrative::dollars(spent),
        narrative::dollars(limit)
    ));
    let mut text = head;
    let foot = "\nEach report follows as its own note.";
    for (i, r) in rows.iter().enumerate() {
        let title = r
            .title
            .as_deref()
            .map(|t| format!(" \"{}\"", t.chars().take(48).collect::<String>()))
            .unwrap_or_default();
        let tail = match r.state {
            "done" => match r.findings {
                Some(0) | None => "0 findings: it came back empty-handed".to_string(),
                Some(n) => narrative::count(n as u64, "finding", "findings"),
            },
            "budget" => match r.findings {
                Some(n) if n > 0 => format!("partial: {n} so far"),
                _ => "partial: nothing yet".to_string(),
            },
            RUNNING => format!("task_wait {} to wait again", r.short),
            _ => r.reason.clone().unwrap_or_else(|| r.state.to_string()),
        };
        let line = format!(
            "\n{}{title}  {}  {} of {}  {}  {}",
            r.short,
            r.state,
            narrative::dollars(r.spent),
            narrative::dollars(r.limit),
            clock(r.elapsed_ms),
            tail
        );
        if text.chars().count() + line.chars().count() + foot.len() > INDEX_MAX_CHARS {
            text.push_str(&format!("\n… and {} more (see the meta)", rows.len() - i));
            break;
        }
        text.push_str(&line);
    }
    if rows.iter().any(|r| r.delivered || r.state == RUNNING) {
        text.push_str(foot);
    }
    let mut meta = json!({
        "tasks": rows.iter().map(|r| json!({
            "id": r.id, "short": r.short, "state": r.state,
            "spent_usd": micros_to_usd(r.spent), "limit_usd": micros_to_usd(r.limit),
            "elapsed_ms": r.elapsed_ms, "chars": r.chars, "cut": r.cut,
            "outputs": r.outputs,
        })).collect::<Vec<_>>(),
        "waited_ms": waited.as_millis() as u64,
        "timed_out": running,
        "warm_reads": warm_reads,
        "fan_in": {"bound": bound, "chars": total, "room": room.map(|(t, l)| json!({"tokens": t, "limit": l}))},
    });
    if let Some(f) = &ask.fan_out {
        meta[FAN_OUT_KEY] = json!(f);
    }
    if interrupted {
        meta[INTERRUPTED_BY] = json!("message");
    }
    ToolOutput { text, meta }
}
