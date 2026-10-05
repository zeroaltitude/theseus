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
//! - **Its arrangement** (M5 27, theseus-vug.2; `arrangement.rs`). A task
//!   starts only from quoted pieces of this session: the messages that define
//!   the work, resolved to nodes and written into the child after the brief,
//!   with the fidelity check for a one-line brief from a long discussion.
//! - **Depth one.** A task cannot start tasks.
//! - **The report.** A task whose turn has nothing left to wait on is done,
//!   and its last message is its report. The frame that ends it carries one
//!   outbox post naming it; a failed turn or a cancel reports so, once. The
//!   parent's next turn writes the report into the parent's session as a
//!   node, before its new input.
//! - **A task's own wakes** (37b, theseus-7kg). A task may set one-shot
//!   wakes (`wake.at`). A turn of it that has nothing left to wait on but a
//!   pending wake parks on input instead of ending (`parks_on_wake`), and the
//!   wake's turn continues it; the turn that would wait with no wake left
//!   ends it, and it reports then, once. Its cancel drops its wakes in the
//!   cancel's frame, so none fires after.
//! - **The report's wake** (W1, theseus-lji). With `wake_parent: true`, the
//!   report also starts that turn: the frame that ends the task queues the
//!   parent, as a due wake does, unless the task was cancelled. The turn runs
//!   in the parent's session, under its authority and budget, and its reply
//!   says which report started it (`📋 task a1b2c3 reported`). It is for a
//!   chain: the parent reviews each result and starts the next.

use serde::Deserialize;
use serde_json::{json, Value};
use theseus_kernel::{
    micros_to_usd, task_ids, usd, usd_to_micros, ExecState, Execution, KernelError, Micros,
    SessionKind,
};
use theseus_store::{kinds, NewRecord};
use theseus_tools::{parse, Backend, Plan, Retry, Tool, ToolClass, ToolCtx};

use crate::narrative;
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
    /// The work a task session does; without it, the call records a plan
    /// item (39a, M7 Q12).
    #[serde(default)]
    brief: Option<String>,
    /// The task's title (39a): the brief's first line when not given.
    #[serde(default)]
    title: Option<String>,
    /// The task's objective and acceptance (39a), when no arrangement's
    /// pieces give them.
    #[serde(default)]
    objective: Option<String>,
    #[serde(default)]
    acceptance: Vec<String>,
    /// The task it goes under, and the tasks it waits on (39a).
    #[serde(default)]
    parent: Option<String>,
    #[serde(default)]
    deps: Vec<String>,
    #[serde(default)]
    budget_usd: Option<f64>,
    /// Its report starts this conversation's next turn (W1).
    #[serde(default)]
    wake_parent: bool,
    /// The messages that define the work (M5 27).
    #[serde(default)]
    arrangement: Option<crate::arrangement::Input>,
    /// A one-line brief from a long discussion, with one piece, on purpose.
    #[serde(default)]
    fidelity_ack: bool,
    /// The task this one checks, by its claim (M5 28a, `check.rs`).
    #[serde(default)]
    check_of: Option<String>,
    /// A check's model: a profile's name (M5 28a). Only a check takes one.
    #[serde(default)]
    profile: Option<String>,
}

fn input_of(input: &Value) -> Result<Input, String> {
    let i: Input = parse(input)?;
    let Some(brief) = &i.brief else {
        if i.check_of.is_some() || i.profile.is_some() {
            return Err("a check (`check_of`) is a task session: give it a `brief`".into());
        }
        if i.title.as_deref().is_none_or(|t| t.trim().is_empty()) {
            return Err("give a `brief` to start a task session, or a `title` to record a plan                         item"
                .into());
        }
        return Ok(i);
    };
    if brief.trim().is_empty() {
        return Err("the brief is empty: say what the task should do".into());
    }
    if i.profile.is_some() && i.check_of.is_none() {
        return Err(
            "`profile` is for a check (`check_of`): every other task runs on this \
                    conversation's model"
                .into(),
        );
    }
    let n = brief.chars().count();
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
         reaches the person at once, and you read it at this conversation's next turn. With \
         `wake_parent: true`, the report starts that turn by itself, so you can review the result \
         and act on it without waiting for the person: use it for a chain, where you review each \
         task's result and start the next. Leave it off for work the person will ask about. A \
         task cannot start tasks.\n\n\
         Every task needs an `arrangement`: quote the messages of this conversation that define \
         the work, rather than paraphrasing them into the brief. Each piece is an exact quote \
         (copied character for character, at least 20 characters, from one message: the \
         person's, your own earlier replies, or a tool result) with its role: `objective` (what \
         to do), `acceptance` (how to know it is done), `design` (how it was decided it should \
         be done), or `context`. At least one piece is an `objective` or a `design`. The task \
         reads each quoted message whole and verbatim, after the brief, with who said it and \
         when. `trust` lists pieces to read as trusted testimony, and `supersedes` pairs \
         [older, newer] where a later message replaced an earlier one, so the task sees the \
         older by reference only (indexes into `pieces`, from 0). A quote that matches no \
         message, or more than one, fails with the reason: quote again, longer or exactly. A \
         short brief drawn from a long discussion with a single piece fails too: attach the \
         design, or say `fidelity_ack: true` if the one piece really is the whole work.\n\n\
         Every task is a record in the task graph, which you see each turn. Without `brief`, \
         the call records a plan item (a `title`, and `objective`, `acceptance`, `parent`, and \
         `deps` if you have them): no session works on it, and it needs no arrangement. Edit \
         the graph with task.update, task.split, and task.close.\n\n\
         To check a task's work independently, start a check: `check_of` names a task this \
         conversation started that has reported. The check reads its brief, that task's \
         objective and acceptance pieces, and its report as a claim, and nothing else of its \
         session, so write the brief from the claim and the goal, not from how the task worked. \
         It needs no arrangement of its own (you may add pieces), and `profile` runs it on \
         another model."
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
                },
                "wake_parent": {
                    "type": "boolean",
                    "description": "When the task finishes or fails, its report starts this conversation's next turn, so you review it and start the next step of a chain (default: false; a cancelled task wakes nothing)."
                },
                "arrangement": {
                    "type": "object",
                    "description": "The messages of this conversation that define the work, quoted exactly; the task reads each whole, verbatim, after the brief.",
                    "properties": {
                        "pieces": {
                            "type": "array",
                            "minItems": 1,
                            "maxItems": crate::arrangement::MAX_PIECES,
                            "items": {
                                "type": "object",
                                "properties": {
                                    "quote": {
                                        "type": "string",
                                        "description": "An exact span of one message of this conversation, at least 20 characters, copied character for character (whitespace runs count as one space)."
                                    },
                                    "node": {
                                        "type": "string",
                                        "description": "Instead of a quote: the message's node id, when a failed call named it."
                                    },
                                    "role": {
                                        "type": "string",
                                        "enum": ["objective", "acceptance", "design", "context"]
                                    }
                                },
                                "required": ["role"],
                                "additionalProperties": false
                            }
                        },
                        "trust": {
                            "type": "array",
                            "items": {"type": "integer", "minimum": 0},
                            "description": "Pieces (indexes from 0) the task reads as trusted testimony."
                        },
                        "supersedes": {
                            "type": "array",
                            "items": {"type": "array", "items": {"type": "integer", "minimum": 0}, "minItems": 2, "maxItems": 2},
                            "description": "[older, newer] pairs of piece indexes: the older is shown by reference only."
                        }
                    },
                    "required": ["pieces"],
                    "additionalProperties": false
                },
                "fidelity_ack": {
                    "type": "boolean",
                    "description": "Start it anyway when the fidelity check fails: a brief under 200 characters, from a long discussion, with a single piece (default: false)."
                },
                "title": {"type": "string", "description": "The task's title in the task graph (default: the brief's first line). Without `brief`, the plan item's title."},
                "objective": {"type": "string", "description": "What the task is for, when no `objective` piece says it."},
                "acceptance": {"type": "array", "items": {"type": "string"}, "description": "How to know it is done, one line each, beside any `acceptance` pieces."},
                "parent": {"type": "string", "description": "The task it goes under (tsk_…)."},
                "deps": {"type": "array", "items": {"type": "string"}, "description": "The tasks it waits on (tsk_…)."},
                "check_of": {"type": "string", "description": "Start a check of this task (its id, or the end of it): one this conversation started, which has reported."},
                "profile": {"type": "string", "description": "A check's model: a configured profile's name (default: this conversation's). Only for `check_of`."}
            },
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
        let Some(brief) = &i.brief else {
            return Ok(Plan {
                summary: format!("record a plan item: {}", i.title.unwrap_or_default().trim()),
                ..Default::default()
            });
        };
        // The notice says so when the task will start a turn by itself (W1).
        let how = match (&i.check_of, i.wake_parent) {
            (Some(c), _) => format!("start a check of task {c}"),
            (None, true) => "start a task whose report starts this conversation's next turn".into(),
            (None, false) => "start a task".into(),
        };
        Ok(Plan {
            summary: format!(
                "{how}: {}",
                i.title.as_deref().unwrap_or(&title_from(brief))
            ),
            ..Default::default()
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// What the gate's notice says of a task start (W1): whether its report
    /// starts the conversation's next turn.
    #[test]
    fn a_task_starts_notice_says_when_its_report_wakes_the_conversation() {
        let ctx = ToolCtx::for_tests(&std::env::temp_dir());
        let summary = |input: Value| TaskCreate.plan(&input, &ctx).unwrap().summary;
        assert_eq!(
            summary(json!({"brief": "Run the gate"})),
            "start a task: Run the gate"
        );
        assert_eq!(
            summary(json!({"brief": "Run the gate", "wake_parent": true})),
            "start a task whose report starts this conversation's next turn: Run the gate"
        );
        assert!(TaskCreate
            .plan(&json!({"brief": "x", "wake_parent": "yes"}), &ctx)
            .is_err());
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

/// The parent's reply that holds the call `correlation_id` (12a): its call
/// node, written when the call was planned, names it. The reply, not the
/// call node, is what the parent's contexts carry. None when no call node
/// names it.
pub(crate) fn holder_of(nodes: &crate::store::Transcript, correlation_id: &str) -> Option<String> {
    let mut calls = nodes
        .iter()
        .rev()
        .filter(|(_, n)| n.kind == crate::stub::Kind::ToolCall);
    calls.find_map(|(_, n)| match &n.body {
        Body::ToolCall {
            correlation_id: Some(c),
            assistant_node,
            ..
        } if c == correlation_id => Some(assistant_node.clone()),
        _ => None,
    })
}

/// Run `task.create` for the call `correlation_id` of the turn `tc` (the
/// harness's side): open the child, and say what was opened. An error is the
/// result the model reads.
#[expect(clippy::too_many_lines, reason = "shape budget: split it")]
pub fn create<'a>(
    tc: &TurnCtx<'a>,
    input: &Value,
    correlation_id: &str,
    profiles: &std::collections::BTreeMap<String, TargetRef>,
) -> Result<crate::task_graph::tools::Done<'a>, String> {
    let i = input_of(input)?;
    let Some(brief) = i.brief.as_deref() else {
        // A plan item (39a): a record, and no session.
        return crate::task_graph::tools::create_item(
            tc,
            &crate::task_graph::tools::Item {
                title: i.title.as_deref(),
                objective: i.objective.as_deref(),
                acceptance: &i.acceptance,
                parent: i.parent.as_deref(),
                deps: &i.deps,
                arrangement: i.arrangement.as_ref(),
            },
            correlation_id,
        );
    };
    let parent = tc
        .kernel
        .execution(tc.execution_id)
        .map_err(|e| format!("{e:#}"))?
        .ok_or("this session has no execution")?;
    if parent.parent.is_some() || parent.kind == SessionKind::Task {
        return Err(DEPTH_REFUSAL.into());
    }
    let nodes = tc
        .store
        .transcript(tc.session_id)
        .map_err(|e| format!("Not started: {e:#}"))?;
    // The brief copies the call's input, which the parent's reply holds
    // (12a): the brief is `derived_from` that reply.
    let holder = holder_of(&nodes, correlation_id);
    // A check's pieces are the checked task's and its own, and its claim
    // (M5 28a); every other task's, its arrangement's (M5 27).
    let check = match i.check_of.as_deref() {
        Some(name) => Some(crate::check::prepare(
            tc,
            &crate::check::Ask {
                name,
                profile: i.profile.as_deref(),
                arrangement: i.arrangement.as_ref(),
                brief,
            },
            &nodes,
            holder.as_deref(),
            profiles,
        )?),
        None => None,
    };
    let (pieces, humans) = match &check {
        Some(c) => (c.pieces.clone(), 0),
        None => arranged(tc, &i, &nodes, holder.as_deref())?,
    };
    let left = parent.budget.available();
    let want = match i.budget_usd {
        Some(b) => usd_to_micros(b),
        None => left / DEFAULT_SHARE.1 * DEFAULT_SHARE.0,
    };
    let target = tc.outbox.target(tc.session_id);
    let title = i.title.clone().unwrap_or_else(|| title_from(brief));
    let (_, task_session) = task_ids(correlation_id);
    let text = brief_text(&task_session, tc.session_id, brief);
    // Its record (39a), written with its session: its objective and
    // acceptance are the arrangement's pieces when they say them.
    let graph = graph_record(tc, &i, &pieces, &title, &task_session)?;
    let created =
        crate::task_graph::tools::change(tc, &graph, None, json!({"session": task_session}));
    let created_row = crate::fact::task_graph::row_of(&tc.rec(), "created", &created)
        .map_err(|e| format!("Not started: {e:#}"))?;
    // A task that a session holding external text starts holds it too, from
    // its brief on, since the brief may carry that text (theseus-9bp).
    let parent_hold = tc
        .store
        .get_session::<SessionRecord>(tc.session_id)
        .map_err(|e| format!("Not started: {e:#}"))?
        .and_then(|r| r.external);
    let opened = tc.kernel.open_task(
        tc.guard,
        correlation_id,
        want,
        target.clone(),
        i.wake_parent,
        |task| {
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
            let author = format!("session:{}", tc.session_id);
            let mut arrangement =
                Node::arrangement(&task.session_id, &author, pieces.clone(), i.fidelity_ack);
            // A check runs on its profile, and reads its claim (M5 28a).
            if let Some(c) = &check {
                if let Some(t) = &c.target {
                    rec.last_target = Some(t.clone());
                }
                if let Body::Arrangement { claim, .. } = &mut arrangement.body {
                    *claim = Some(c.claim.clone());
                }
            }
            rec.task = Some(TaskOf {
                parent_session: tc.session_id.into(),
                parent_execution: tc.execution_id.into(),
                by: correlation_id.into(),
                target: target.clone(),
                arrangement: Some(arrangement.id.clone()),
                check: check.as_ref().map(|c| c.basis.clone()),
            });
            let brief = Node::relayed(&task.session_id, None, Origin::Agent, &author, &text);
            let mut records = match &parent_hold {
                Some(h) => {
                    let taken = crate::external::taken(
                        h,
                        tc.session_id,
                        crate::external::VIA_TASK,
                        &brief.id,
                        theseus_protocol::now_unix_ms(),
                    );
                    crate::external::hold(rec, taken, None)?.unwrap_or_default()
                }
                None => vec![NewRecord::json(
                    kinds::SESSION,
                    Some(&rec.session_id),
                    &rec,
                )?],
            };
            records.push(brief.record()?);
            if let Some(reply) = &holder {
                records.push(
                    crate::graph::Edge::new(
                        crate::graph::EdgeKind::DerivedFrom,
                        &brief.id,
                        reply,
                        crate::graph::VIA_BRIEF,
                    )
                    .record()?,
                );
            }
            // The arrangement reads after the brief, and copies each piece's
            // node (M5 27).
            records.push(arrangement.record()?);
            for p in &pieces {
                records.push(
                    crate::graph::Edge::new(
                        crate::graph::EdgeKind::DerivedFrom,
                        &arrangement.id,
                        &p.node,
                        crate::graph::VIA_ARRANGEMENT,
                    )
                    .record()?,
                );
            }
            if let Some(c) = &check {
                records.push(
                    crate::graph::Edge::new(
                        crate::graph::EdgeKind::DerivedFrom,
                        &arrangement.id,
                        &c.claim.node,
                        crate::graph::VIA_CLAIM,
                    )
                    .record()?,
                );
            }
            if let Some(t) = &target {
                records.push(tc.outbox.task_record(&task.session_id, t)?);
            }
            records.push(crate::task_graph::record(&graph)?);
            records.push(created_row.clone());
            Ok(records)
        },
    );
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
        tc.record(&crate::fact::tool::TaskStarted {
            short: &s,
            title: &title,
            session_id: &task.session_id,
            limit,
            available_before: opened.available_before,
            target: target.as_deref(),
            pieces: pieces.len(),
        });
        tc.record(&crate::fact::arrangement::TaskArranged {
            short: &s,
            session_id: &task.session_id,
            pieces: &pieces,
            fidelity_ack: i.fidelity_ack,
            humans,
            brief_chars: brief.trim().chars().count(),
        });
        if let Some(c) = &check {
            tc.record(&crate::fact::check::TaskCheckOpened {
                short: &s,
                session_id: &task.session_id,
                basis: &c.basis,
            });
        }
        crate::fact::task_graph::announce(&tc.rec(), "created", &created);
        if parent_hold.is_some() {
            tc.record(&crate::fact::tool::TaskHoldsExternal { short: &s });
        }
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
    let text = if opened.opened && task.wake_parent {
        format!(
            "Started task {s} (\"{title}\") with a budget of {budget}. It works on its own and \
             reports here when it finishes, and its report starts this conversation's next turn, \
             so you read it then without waiting for the person. Its id is {}.",
            task.session_id
        )
    } else if opened.opened {
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
    let text = format!(
        "{text}\n\nIts arrangement, {}, which it reads whole after the brief:\n{}{}",
        crate::narrative::count(pieces.len() as u64, "piece", "pieces"),
        crate::arrangement::resolved_lines(&pieces),
        if i.fidelity_ack {
            "\nThe fidelity check was acknowledged (`fidelity_ack`), and the task's lines say so."
        } else {
            ""
        }
    );
    let meta = json!({
        "task_id": task.session_id,
        "short": s,
        "execution_id": task.id,
        "limit_usd": micros_to_usd(limit),
        "asked_usd": micros_to_usd(want),
        "left_usd": micros_to_usd(opened.available_before),
        "capped": capped,
        "opened": opened.opened,
        "wake_parent": task.wake_parent,
        "arrangement": crate::arrangement::meta(&pieces),
        "fidelity_ack": i.fidelity_ack,
        "record": graph.id,
    });
    let text = format!(
        "{text}\nIts record in the task graph: {}",
        crate::task_graph::line(&graph)
    );
    let (text, meta) = match &check {
        Some(c) => crate::check::said(text, meta, &c.basis),
        None => (text, meta),
    };
    Ok(crate::task_graph::tools::Done {
        text,
        meta,
        ..Default::default()
    })
}

/// A task session's record (39a): new at version 1, `in_progress`, with
/// its session, under `parent` when the call names one.
fn graph_record(
    tc: &TurnCtx<'_>,
    i: &Input,
    pieces: &[crate::arrangement::Piece],
    title: &str,
    task_session: &str,
) -> Result<crate::task_graph::TaskRecord, String> {
    use crate::task_graph as g;
    let all = g::all(tc.store).map_err(|e| format!("Not started: {e:#}"))?;
    let parent = match i.parent.as_deref() {
        Some(p) => Some(g::resolve(&all, p)?.id.clone()),
        None => None,
    };
    for d in &i.deps {
        if !all.iter().any(|t| &t.id == d) {
            return Err(format!("no task is named `{d}` (deps name tasks by id)"));
        }
    }
    let (objective, acceptance) =
        g::tools::from_pieces(pieces, i.objective.as_deref(), &i.acceptance, title);
    Ok(g::NewTask {
        id: g::of_session(task_session),
        title,
        objective,
        acceptance,
        parent,
        deps: i.deps.clone(),
        session: Some(task_session.into()),
        origin: g::TaskOrigin {
            session: tc.session_id.into(),
            principal: g::principal_of(tc.kernel, tc.execution_id),
        },
        state: g::TaskState::InProgress,
    }
    .build(theseus_protocol::now_unix_ms()))
}

/// The call's arrangement, resolved against the calling session's transcript
/// `nodes`, and the fidelity check passed (M5 27): its pieces, and the
/// operator's messages since the session's last task. A refusal is ledgered
/// (`task.arrangement_refused`), and the model reads why.
fn arranged(
    tc: &TurnCtx<'_>,
    i: &Input,
    nodes: &crate::store::Transcript,
    holder: Option<&str>,
) -> Result<(Vec<crate::arrangement::Piece>, usize), String> {
    use crate::arrangement::{self as arr, Refused};
    let given = i.arrangement.as_ref().map_or(0, |a| a.pieces.len());
    let refuse = |r: Refused| {
        tc.record(&crate::fact::arrangement::TaskArrangementRefused {
            class: r.class,
            pieces: given,
            reason: &r.message,
        });
        r.message
    };
    let Some(a) = &i.arrangement else {
        return Err(refuse(Refused {
            class: "missing",
            message: format!(
                "{} Add `arrangement.pieces`: at least one exact quote, with the role \
                 `objective` or `design`, of the message that asked for it.",
                arr::REFUSAL
            ),
        }));
    };
    a.check().map_err(|message| {
        refuse(Refused {
            class: if message.starts_with(arr::REFUSAL) {
                "no_objective"
            } else {
                "invalid"
            },
            message,
        })
    })?;
    let pieces = arr::resolve(a, nodes, holder).map_err(refuse)?;
    let humans = arr::human_messages_since_last_task(nodes);
    let brief = i.brief.as_deref().unwrap_or_default();
    arr::fidelity(brief, humans, &pieces, i.fidelity_ack).map_err(refuse)?;
    Ok((pieces, humans))
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
    /// Where it reports: where its parent posted when it started.
    pub target: Option<String>,
    /// The task held external text when its report was read (theseus-9bp):
    /// the parent that reads the report holds it too.
    pub external: Option<theseus_protocol::ExternalText>,
    /// A check's basis, as its line (M5 28a): shown beside its report.
    pub check: Option<String>,
}

/// Whether a task's turn that would end it parks instead (37b): its
/// execution still holds a pending wake, whose turn continues the task. Only
/// the task's own turns set its wakes; a cancel that takes the last one as
/// the turn ends leaves the kernel to queue it (`wakes::task_unparked`), so
/// it is never left waiting with nothing to wake it. Unreadable, and it ends
/// as before 37b, its wakes dropped by the end's frame.
pub fn parks_on_wake(kernel: &theseus_kernel::Kernel, execution_id: &str) -> bool {
    kernel
        .execution(execution_id)
        .ok()
        .flatten()
        .is_some_and(|e| !e.wakes.is_empty())
}

/// The line a turn that a report started shows above its reply (W1), as a
/// wake's turn shows its wake's line.
pub fn woke_line(short: &str) -> String {
    format!("📋 task {short} reported")
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
            target: None,
            external: None,
            check: None,
        }
    }

    /// The outbox post: the text by its node, as a reply names its loops'
    /// (§3.16 references, not payloads), and the facts beside it.
    pub fn post_body(&self) -> Value {
        let mut body = json!({
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
        });
        if let Some(c) = &self.check {
            body["check"] = json!(c);
        }
        body
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
        if let Some(c) = &self.check {
            out.push('\n');
            out.push_str(c);
        }
        if let Some(t) = &self.text {
            let (t, _) = crate::toolrun::cap(t, REPORT_NODE_CHARS, |_| {
                format!("the whole message stays in task {}'s session", self.short)
            });
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
    let target = rec.as_ref().and_then(|r| r.task.as_ref()?.target.clone());
    let external = rec.as_ref().and_then(|r| r.external.clone());
    let check = check_line(rec.as_ref());
    Ok(Some(Report {
        target,
        external,
        check,
        ..Report::new(&e, rec.and_then(|r| r.title), last)
    }))
}

/// Whether `n` is a task's brief: the message its parent's session relayed
/// into it, which opens the task's session (`create`).
pub fn is_brief(n: &Node) -> bool {
    matches!(n.body, Body::UserMessage { .. })
        && n.author
            .as_deref()
            .is_some_and(|a| a.starts_with("session:"))
}

/// A task's brief, read from the store: its session's first node, when that
/// is the brief.
pub fn brief(store: &crate::store::Store, task_session: &str) -> anyhow::Result<Option<Node>> {
    Ok(store
        .first_node(task_session)?
        .map(|(_, n)| n)
        .filter(is_brief))
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
        // A task parked on its own wake waits on that, not on input (37b).
        waiting_on: (e.state == ExecState::Waiting)
            .then(|| match &e.wake {
                Some(theseus_kernel::Wake::Input) if !e.wakes.is_empty() => Some("a wake".into()),
                w => w.as_ref().map(wake_word),
            })
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
        wake_parent: e.wake_parent,
        attention: None,
        arrangement: None,
        check: task_of.and_then(|t| t.check.clone()),
    }
}

/// A check's line (M5 28a), from its session's record.
fn check_line(rec: Option<&SessionRecord>) -> Option<String> {
    Some(rec?.task.as_ref()?.check.as_ref()?.line())
}

/// A task's arrangement, as its surfaces show it: read from its node, which
/// its session record names (M5 27). None for a task from before it.
pub fn arrangement_of(
    store: &crate::store::Store,
    rec: Option<&SessionRecord>,
) -> Option<theseus_protocol::TaskArrangement> {
    let id = rec?.task.as_ref()?.arrangement.as_deref()?;
    let (_, node) = store.get_node(id).ok()??;
    crate::arrangement::info(&node)
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
    let check = check_line(rec.as_ref());
    let report = Report {
        check,
        ..Report::new(e, rec.and_then(|r| r.title), None)
    };
    let (post, records) = outbox.stage(&e.session_id, &e.id, &target, report.post_body())?;
    Ok((records, Some(post)))
}
