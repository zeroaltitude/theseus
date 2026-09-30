//! Task sessions (DD7, theseus-qn2; spec §3.2a, §3.5): `task.create` opens a
//! child session that works on its own and reports back to the place.
//!
//! - **The tool.** `task.create { brief, budget_usd? }` is a tool the harness
//!   runs (`Backend::Harness`). It opens the child in one kernel frame
//!   (`Kernel::open_task`) and returns at once with the task's id, and the
//!   parent's turn goes on. The driver takes the child's first turn, whose
//!   input is the brief.
//! - **What the child inherits.** The parent's authority (the kernel copies
//!   it), its persona and context files (the config's, for every session),
//!   its postures (the daemon has one tool runtime), and its model: the
//!   child's turns run on the parent's target. Its cards and its budget
//!   question go where the parent's go, and name the task.
//! - **Its budget** is carved from what the parent has left: `budget_usd`, or
//!   a quarter of what is left, capped at all of it. Its spend counts against
//!   the parent (the kernel's carve and carry).
//! - **Depth one.** A task cannot start tasks.
//! - **The report.** A task whose turn has nothing left to wait on is done,
//!   and its last message is its report. The frame that ends it carries one
//!   outbox post naming it; a failed turn or a cancel reports so, once. The
//!   parent's next turn writes the report into the parent's session as a
//!   node, before its new input. Nothing starts a parent turn.

use serde::Deserialize;
use serde_json::{json, Value};
use theseus_kernel::{
    micros_to_usd, task_ids, usd, usd_to_micros, ExecState, Execution, KernelError, Micros,
    SessionKind,
};
use theseus_store::{kinds, NewRecord};
use theseus_tools::{parse, Backend, Plan, Retry, Tool, ToolClass, ToolCtx};

use crate::narrative::{self, narrate_turn};
use crate::node::{Body, Node, Origin};
use crate::session::{title_from, SessionRecord, TargetRef, TaskOf};
use crate::toolrun::TurnCtx;

/// The one `task.*` verb needed now.
pub const CREATE: &str = "task.create";
/// The tools this module adds, for the template's `[policy.tools]` list.
pub const NAMES: [&str; 1] = [CREATE];
/// A task's budget when the call names none: this share of what the parent
/// has left.
pub const DEFAULT_SHARE: (u64, u64) = (1, 4);
/// The longest brief a task takes, in characters.
pub const MAX_BRIEF_CHARS: usize = 32_000;
/// The most of a report's text the parent's node carries, in characters.
pub const REPORT_NODE_CHARS: usize = 8_000;

/// What a task says to a model that asks it to start one (depth one).
pub const DEPTH_REFUSAL: &str = "Refused: this session is itself a task, and a task cannot start \
    tasks (depth one). Do the work here, or finish and say in your report what should be started \
    next; the conversation that started you can start it.";

/// How people name a task: the last six characters of its id.
pub fn short(task_id: &str) -> String {
    let n = task_id.chars().count();
    task_id.chars().skip(n.saturating_sub(6)).collect()
}

/// `task.create`'s input.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct Input {
    brief: String,
    #[serde(default)]
    budget_usd: Option<f64>,
}

fn input_of(input: &Value) -> Result<Input, String> {
    let i: Input = parse(input)?;
    if i.brief.trim().is_empty() {
        return Err("the brief is empty: say what the task should do".into());
    }
    let n = i.brief.chars().count();
    if n > MAX_BRIEF_CHARS {
        return Err(format!(
            "the brief is {n} characters, over the {MAX_BRIEF_CHARS} a task takes"
        ));
    }
    if let Some(b) = i.budget_usd {
        if !(b.is_finite() && b > 0.0) {
            return Err(format!("budget_usd must be more than $0, not {b}"));
        }
    }
    Ok(i)
}

/// The toollet side of `task.create`: its name, description, and schema, and
/// the plan the gate reads. The harness runs it (`create`).
pub struct TaskCreate;

impl Tool for TaskCreate {
    fn name(&self) -> &'static str {
        CREATE
    }

    fn description(&self) -> &'static str {
        "Start a background task: a new session that works on `brief` by itself, with your tools, \
         postures, and context files, and reports back here when it is done. It returns at once \
         with the task's id, and you carry on. The task sees only the brief, not this \
         conversation, so write the brief as a complete instruction: what to do, where, and what \
         to report. Its budget is `budget_usd`, capped at what this session has left; without it, \
         a quarter of what is left. What it spends counts against this session. Its report \
         arrives in a later turn of this conversation. A task cannot start tasks."
    }

    fn input_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "brief": {
                    "type": "string",
                    "description": "The complete instruction for the task: what to do, where, and what to report."
                },
                "budget_usd": {
                    "type": "number",
                    "exclusiveMinimum": 0,
                    "description": "Its spend limit in US dollars, capped at what this session has left (default: a quarter of that)."
                }
            },
            "required": ["brief"],
            "additionalProperties": false
        })
    }

    fn class(&self) -> ToolClass {
        // It starts work that runs on its own, and runs alone in a batch.
        ToolClass::Run
    }

    fn backend(&self) -> Backend {
        Backend::Harness
    }

    fn retry(&self) -> Retry {
        // Run again after a crash, it finds the task it opened (`task_ids`).
        Retry::NonRepeatable
    }

    fn plan(&self, input: &Value, _ctx: &ToolCtx) -> Result<Plan, String> {
        let i = input_of(input)?;
        Ok(Plan {
            summary: format!("start a task: {}", title_from(&i.brief)),
            ..Default::default()
        })
    }
}

/// The child's first input: what it is, and then the brief.
fn brief_text(task_session: &str, parent_session: &str, brief: &str) -> String {
    format!(
        "[Task {}: a background task started by session {}. Work on the brief below on your \
         own. Nobody reads this session while you work, and nobody answers questions asked in \
         it: decide, and say what you decided. Your last message is your report to the \
         conversation that started you, so end with what you did and what you found. You \
         cannot start tasks of your own.]\n\n{brief}",
        short(task_session),
        narrative::short(parent_session)
    )
}

/// Run `task.create` for the call `correlation_id` of the turn `tc` (the
/// harness's side): open the child, and say what was opened. An error is the
/// result the model reads.
pub fn create(
    tc: &TurnCtx<'_>,
    input: &Value,
    correlation_id: &str,
) -> Result<(String, Value), String> {
    let i = input_of(input)?;
    let parent = tc
        .kernel
        .execution(tc.execution_id)
        .map_err(|e| format!("{e:#}"))?
        .ok_or("this session has no execution")?;
    if parent.parent.is_some() || parent.kind == SessionKind::Task {
        return Err(DEPTH_REFUSAL.into());
    }
    let left = parent.budget.available();
    let want = match i.budget_usd {
        Some(b) => usd_to_micros(b),
        None => left / DEFAULT_SHARE.1 * DEFAULT_SHARE.0,
    };
    let target = tc.outbox.target(tc.session_id);
    let title = title_from(&i.brief);
    let (_, task_session) = task_ids(correlation_id);
    let text = brief_text(&task_session, tc.session_id, &i.brief);
    let opened = tc
        .kernel
        .open_task(tc.guard, correlation_id, want, target.clone(), |task| {
            let mut rec = SessionRecord::with_id(
                task.session_id.clone(),
                SessionKind::Task,
                Some("task".into()),
            );
            rec.title = Some(title.clone());
            rec.execution_id = Some(task.id.clone());
            rec.last_target = tc.target.map(|t| TargetRef {
                profile: t.profile.clone(),
                provider: t.provider.clone(),
                model: t.model.clone(),
            });
            rec.task = Some(TaskOf {
                parent_session: tc.session_id.into(),
                parent_execution: tc.execution_id.into(),
                by: correlation_id.into(),
                target: target.clone(),
            });
            let brief = Node::relayed(
                &task.session_id,
                None,
                Origin::Agent,
                &format!("session:{}", tc.session_id),
                &text,
            );
            let mut records = vec![
                NewRecord::json(kinds::SESSION, Some(&rec.session_id), &rec)?,
                brief.record()?,
            ];
            if let Some(t) = &target {
                records.push(tc.outbox.task_record(&task.session_id, t)?);
            }
            Ok(records)
        });
    let opened = match opened {
        Ok(o) => o,
        Err(e) => {
            return Err(match e.downcast_ref::<KernelError>() {
                Some(KernelError::TaskDepth { .. }) => DEPTH_REFUSAL.into(),
                Some(KernelError::NothingToCarve { .. }) => {
                    format!("Not started: {e}. Finish the work here instead.")
                }
                _ => format!("Not started: {e:#}"),
            })
        }
    };
    let task = &opened.task;
    let s = short(&task.session_id);
    let limit = task.budget.limit_micros;
    if opened.opened {
        if let Some(t) = &target {
            tc.outbox.task_bound(&task.session_id, t);
        }
        narrate_turn!(
            tc,
            Session,
            "Task {s} started (\"{title}\"): its session {} runs on its own with {} carved from \
             the {} this session had left, and reports {}.",
            narrative::short(&task.session_id),
            narrative::dollars(limit),
            narrative::dollars(opened.available_before),
            match &target {
                Some(t) => format!("to {t}"),
                None => "in this session".to_string(),
            }
        );
    }
    let capped = i.budget_usd.is_some() && want > limit;
    let budget = if capped {
        format!(
            "{} (you asked for {}, and this session had {} left)",
            usd(limit),
            usd(want),
            usd(opened.available_before)
        )
    } else {
        usd(limit)
    };
    let text = if opened.opened {
        format!(
            "Started task {s} (\"{title}\") with a budget of {budget}. It works on its own and \
             reports here when it finishes; its report reaches a later turn of this \
             conversation. Its id is {}.",
            task.session_id
        )
    } else {
        format!(
            "Task {s} (\"{title}\") was already started by this call; it is {} now. Its id is {}.",
            task.state.as_str(),
            task.session_id
        )
    };
    let meta = json!({
        "task_id": task.session_id,
        "short": s,
        "execution_id": task.id,
        "limit_usd": micros_to_usd(limit),
        "asked_usd": micros_to_usd(want),
        "left_usd": micros_to_usd(opened.available_before),
        "capped": capped,
        "opened": opened.opened,
    });
    Ok((text, meta))
}

/// The last thing a session's model said: the id and text of its last
/// assistant node with any text. A task's report.
pub fn last_message<'a>(
    nodes: impl DoubleEndedIterator<Item = &'a Node>,
) -> Option<(String, String)> {
    nodes.rev().find_map(|n| match &n.body {
        Body::AssistantMessage { blocks, .. } => {
            let text = crate::provider::text_of(blocks);
            (!text.is_empty()).then(|| (n.id.clone(), text))
        }
        _ => None,
    })
}

/// A task's report, as its post and its parent's node say it (DD7).
#[derive(Debug, Clone)]
pub struct Report {
    pub task: String,
    pub short: String,
    pub execution_id: String,
    pub title: Option<String>,
    /// `complete`, `failed`, or `cancelled`.
    pub outcome: &'static str,
    /// Its last message, for a task that finished: the node and its text.
    pub node: Option<String>,
    pub text: Option<String>,
    /// Why it failed, or who cancelled it.
    pub reason: Option<String>,
    pub spent_micros: Micros,
    pub limit_micros: Micros,
    pub turns: u64,
    pub elapsed_ms: u64,
}

impl Report {
    /// From the task's execution as it ends, its title, and its last message.
    pub fn new(e: &Execution, title: Option<String>, last: Option<(String, String)>) -> Self {
        let outcome = match e.state {
            ExecState::Complete => "complete",
            ExecState::Cancelled => "cancelled",
            _ => "failed",
        };
        let (node, text) = match (outcome, last) {
            ("complete", Some((n, t))) => (Some(n), Some(t)),
            _ => (None, None),
        };
        Self {
            task: e.session_id.clone(),
            short: short(&e.session_id),
            execution_id: e.id.clone(),
            title,
            outcome,
            node,
            text,
            reason: (outcome != "complete")
                .then(|| e.ended_reason.clone())
                .flatten(),
            spent_micros: e.budget.spent_micros,
            limit_micros: e.budget.limit_micros,
            turns: e.turns,
            elapsed_ms: e.updated_at_ms.saturating_sub(e.created_at_ms),
        }
    }

    /// The outbox post: the text by its node, as a reply names its loops'
    /// (§3.16 references, not payloads), and the facts beside it.
    pub fn post_body(&self) -> Value {
        json!({
            "kind": "report",
            "task": self.task,
            "short": self.short,
            "execution_id": self.execution_id,
            "title": self.title,
            "outcome": self.outcome,
            "node": self.node,
            "reason": self.reason,
            "spent_usd": micros_to_usd(self.spent_micros),
            "limit_usd": micros_to_usd(self.limit_micros),
            "turns": self.turns,
            "elapsed_ms": self.elapsed_ms,
        })
    }

    /// The node the parent's next turn reads.
    pub fn node_text(&self) -> String {
        let title = self
            .title
            .as_deref()
            .map(|t| format!(" (\"{t}\")"))
            .unwrap_or_default();
        let turns = narrative::count(self.turns, "turn", "turns");
        let money = format!(
            "{} of its {}",
            narrative::dollars(self.spent_micros),
            narrative::dollars(self.limit_micros)
        );
        let head = match self.outcome {
            "complete" => format!(
                "finished after {turns}, {money}, in {}",
                narrative::duration(self.elapsed_ms)
            ),
            "cancelled" => format!(
                "{} after {turns}, {money}",
                self.reason.as_deref().unwrap_or("cancelled")
            ),
            _ => format!(
                "failed after {turns}, {money}: {}",
                self.reason.as_deref().unwrap_or("its turn failed")
            ),
        };
        let mut out = format!("[Report from task {}{title}: {head}]", self.short);
        if let Some(t) = &self.text {
            let (t, _) = crate::toolrun::cap(t, REPORT_NODE_CHARS);
            out.push_str("\n\n");
            out.push_str(&t);
        }
        out
    }
}

/// A task's report, read from the store: its execution as it ended, its
/// title, and, when it finished, its last message. None when it is not a task
/// that ended.
pub fn load_report(
    store: &crate::store::Store,
    kernel: &theseus_kernel::Kernel,
    execution_id: &str,
) -> anyhow::Result<Option<Report>> {
    let Some(e) = kernel.execution(execution_id)? else {
        return Ok(None);
    };
    if e.parent.is_none() || !e.state.is_terminal() {
        return Ok(None);
    }
    let rec: Option<SessionRecord> = store.get_session(&e.session_id)?;
    let last = if e.state == ExecState::Complete {
        let nodes = store.session_nodes(&e.session_id)?;
        last_message(nodes.iter().map(|(_, n)| n))
    } else {
        None
    };
    Ok(Some(Report::new(&e, rec.and_then(|r| r.title), last)))
}

/// A session's task, for the questions it asks: a card names it (DD7).
pub fn task_ref(session: &SessionRecord) -> Option<theseus_protocol::TaskRef> {
    session.task.as_ref().map(|_| theseus_protocol::TaskRef {
        task_id: session.session_id.clone(),
        short: short(&session.session_id),
        title: session.title.clone(),
    })
}

/// A task as the protocol shows it (`task.list`, `/tasks`, the web UI).
pub fn info(
    e: &Execution,
    rec: Option<&SessionRecord>,
    pending_confirms: u32,
) -> theseus_protocol::TaskInfo {
    let task_of = rec.and_then(|r| r.task.as_ref());
    theseus_protocol::TaskInfo {
        task_id: e.session_id.clone(),
        short: short(&e.session_id),
        execution_id: e.id.clone(),
        parent_session_id: task_of
            .map(|t| t.parent_session.clone())
            .unwrap_or_default(),
        parent_execution_id: e.parent.clone().unwrap_or_default(),
        title: rec.and_then(|r| r.title.clone()),
        state: e.state.as_str().into(),
        waiting_on: (e.state == ExecState::Waiting)
            .then(|| e.wake.as_ref().map(wake_word))
            .flatten(),
        spent_usd: micros_to_usd(e.budget.spent_micros),
        limit_usd: micros_to_usd(e.budget.limit_micros),
        cost_usd: rec.map_or(0.0, |r| r.cost_usd),
        turns: e.turns,
        pending_confirms,
        target: task_of.and_then(|t| t.target.clone()),
        ended_reason: e.ended_reason.clone(),
        created_at_ms: e.created_at_ms,
        updated_at_ms: e.updated_at_ms,
    }
}

fn wake_word(w: &theseus_kernel::Wake) -> String {
    use theseus_kernel::Wake;
    match w {
        Wake::DueAt { .. } => "a due time",
        Wake::Actions { .. } => "a job",
        Wake::Execution { .. } => "another execution",
        Wake::Confirm { .. } => "an approval",
        Wake::Input => "input",
        Wake::Budget { .. } => "the budget",
    }
    .into()
}

/// No task, or more than one, answers to a name; the message says which.
#[derive(Debug, thiserror::Error)]
#[error("{0}")]
pub struct NoSuchTask(pub String);

/// The task a person means by `name`: its id, its execution's id, or the end
/// of either (`a1b2c3`, `…a1b2c3`), when exactly one task matches.
pub fn resolve<'a>(tasks: &'a [Execution], name: &str) -> Result<&'a Execution, String> {
    let n = name
        .trim()
        .trim_start_matches('…')
        .trim_start_matches("task ");
    if n.len() < 4 {
        return Err(format!(
            "`{name}` is too short to name a task: give at least four characters of its id"
        ));
    }
    let exact: Vec<&Execution> = tasks
        .iter()
        .filter(|e| e.session_id == n || e.id == n)
        .collect();
    if let [one] = exact.as_slice() {
        return Ok(one);
    }
    let ends: Vec<&Execution> = tasks
        .iter()
        .filter(|e| e.session_id.ends_with(n) || e.id.ends_with(n))
        .collect();
    match ends.as_slice() {
        [one] => Ok(one),
        [] => Err(format!("no task is named `{name}`")),
        many => Err(format!(
            "`{name}` names {} tasks ({}): give more of its id",
            many.len(),
            many.iter()
                .map(|e| short(&e.session_id))
                .collect::<Vec<_>>()
                .join(", ")
        )),
    }
}

/// The report of a task a cancel ended, for the cancel's frame: its post,
/// staged where the task reports, and the post itself for `posted` once the
/// frame is written. Nothing for an execution that is not a task, or a task
/// that reports nowhere.
pub fn cancelled_report(
    outbox: &crate::outbox::Outbox,
    store: &crate::store::Store,
    e: &Execution,
) -> anyhow::Result<(Vec<NewRecord>, Option<theseus_kernel::Action>)> {
    if e.parent.is_none() {
        return Ok((vec![], None));
    }
    let rec: Option<SessionRecord> = store.get_session(&e.session_id)?;
    let Some(target) = rec.as_ref().and_then(|r| r.task.as_ref()?.target.clone()) else {
        return Ok((vec![], None));
    };
    let report = Report::new(e, rec.and_then(|r| r.title), None);
    let (post, records) = outbox.stage(&e.session_id, &e.id, &target, report.post_body())?;
    Ok((records, Some(post)))
}
