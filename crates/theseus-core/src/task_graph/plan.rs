//! A plan is the task graph's plan items (theseus-lyxy; the owner's decision
//! 2: plan mode's steps are the work board's plan items, never a second list
//! of steps).
//!
//! `plan_present`'s approval records each step as a plan item (a task record
//! with no session, no parent and a title, as `task_create` without a brief
//! makes one), in the frame that settles the call; `plan_step` moves one by
//! its place in the plan: `doing` is `in_progress`, `done` closes it `done`
//! and `skipped` closes it `abandoned`, which is what `task_close` does too,
//! so the model may use either. The checklist every surface shows, and the
//! `plan 2/5` of its status line, are read from these records
//! (`view`), as the work board's `progress` counts them: nothing else holds
//! a step.

use anyhow::Result;
use serde_json::json;
use theseus_protocol::modes::{checklist, plan_line, PlanItem, PlanState};

use super::tools::{change, locked, Done};
use super::{id_from, principal_of, NewTask, TaskEvidence, TaskOrigin, TaskRecord, TaskState};
use crate::store::Store;
use crate::toolrun::TurnCtx;

/// The most steps and characters a plan has: a plan is a list a person reads
/// in one glance, never a transcript.
pub const MAX_STEPS: usize = 20;
pub const MAX_STEP_CHARS: usize = 200;

/// The conversation's plan items, in the order they were listed: the records
/// its calls made that no session works on and no task holds.
pub fn items_of(store: &Store, session_id: &str) -> Result<Vec<TaskRecord>> {
    Ok(super::all(store)?
        .into_iter()
        .filter(|t| t.session.is_none() && t.parent.is_none() && t.origin.session == session_id)
        .collect())
}

/// What a surface shows of a plan item.
pub fn item_of(t: &TaskRecord) -> PlanItem {
    PlanItem {
        id: t.id.clone(),
        title: t.title.clone(),
        state: PlanState::of_task(t.state),
    }
}

/// The conversation's plan as a surface shows it.
pub fn view(store: &Store, session_id: &str) -> Vec<PlanItem> {
    items_of(store, session_id)
        .map(|v| v.iter().map(item_of).collect())
        .unwrap_or_default()
}

/// Whether a record is an item of a conversation's plan.
pub fn is_item(t: &TaskRecord) -> bool {
    t.session.is_none() && t.parent.is_none()
}

/// Record the approved steps as plan items, one record each, a millisecond
/// apart so they keep their order. A call made again (a retry after a crash)
/// finds its first item and records nothing twice. Returns the call's result
/// and the items as the conversation's plan now stands.
pub fn create<'a>(
    tc: &TurnCtx<'a>,
    steps: &[String],
    correlation_id: &str,
    mode_words: &str,
) -> Result<(Done<'a>, Vec<PlanItem>), String> {
    let id = |n: usize| id_from(&format!("{correlation_id}p{n}"));
    let had = super::get(tc.store, &id(1)).map_err(|e| format!("{e:#}"))?;
    let mut plan = view(tc.store, tc.session_id);
    let mut done = Done::default();
    if had.is_some() {
        done.text = format!(
            "This plan was recorded already. {}\n{}",
            plan_line(&plan).unwrap_or_default(),
            checklist(&plan).join("\n")
        );
        done.meta = json!({"steps": steps.len(), "opened": false});
        return Ok((done, plan));
    }
    let now = theseus_protocol::now_unix_ms();
    let principal = principal_of(tc.kernel, tc.execution_id);
    for (n, title) in steps.iter().enumerate() {
        let rec = NewTask {
            id: id(n + 1),
            title,
            objective: String::new(),
            acceptance: vec![],
            parent: None,
            deps: vec![],
            session: None,
            origin: TaskOrigin {
                session: tc.session_id.into(),
                principal: principal.clone(),
                by_model: true,
            },
            state: TaskState::Accepted,
        }
        .build(now + n as u64);
        plan.push(item_of(&rec));
        done.row(tc, "created", change(tc, &rec, None, json!({"plan": true})))?;
    }
    done.text = format!(
        "Plan approved: {} steps are recorded as plan items, and {mode_words}. Do them in order, \
         and call plan_step as each one begins (doing) and ends (done, or skipped).\n{}",
        steps.len(),
        checklist(&plan).join("\n")
    );
    done.meta = json!({"steps": steps.len(), "opened": true});
    Ok((done, plan))
}

/// Move the plan's `index`th item (1 is the first) to `state`: a write of its
/// record, versioned as any edit, in the frame that settles the call.
pub fn step<'a>(tc: &TurnCtx<'a>, index: usize, state: PlanState) -> Result<Done<'a>, String> {
    let items = items_of(tc.store, tc.session_id).map_err(|e| format!("{e:#}"))?;
    if items.is_empty() {
        return Err("No plan is recorded: present one with plan_present first.".into());
    }
    let Some(target) = index.checked_sub(1).and_then(|k| items.get(k)) else {
        return Err(format!(
            "The plan has {} steps: {index} is not one.",
            items.len()
        ));
    };
    let mut done = Done::default();
    let (lock, mut rec) = locked(tc, &target.id)?;
    done.locks.push(lock);
    if rec.state.is_closed() {
        return Err(format!(
            "Step {index} is {} already: a closed step stays as it is.",
            rec.state.as_str()
        ));
    }
    let now = theseus_protocol::now_unix_ms();
    let from = rec.version;
    let (verb, added) = match state {
        PlanState::Doing => {
            rec.state = TaskState::InProgress;
            ("updated", 0)
        }
        PlanState::Done | PlanState::Skipped => {
            let (to, note) = match state {
                PlanState::Done => (TaskState::Done, "marked done with plan_step"),
                _ => (TaskState::Abandoned, "skipped with plan_step"),
            };
            rec.state = to;
            rec.evidence.push(TaskEvidence {
                node: None,
                identity: format!("plan_step:{index}"),
                note: Some(note.into()),
                by: tc.session_id.into(),
                at_ms: now,
            });
            ("closed", 1)
        }
        PlanState::Todo => return Err("state is doing, done or skipped".into()),
    };
    rec.version += 1;
    rec.updated_at_ms = now;
    let plan: Vec<PlanItem> = items
        .iter()
        .map(|t| {
            if t.id == rec.id {
                item_of(&rec)
            } else {
                item_of(t)
            }
        })
        .collect();
    done.row(
        tc,
        verb,
        change(tc, &rec, Some(from), json!({"added": added})),
    )?;
    done.text = format!(
        "Step {index} is {}. {}\n{}",
        state.name(),
        plan_line(&plan).unwrap_or_default(),
        checklist(&plan).join("\n")
    );
    done.meta = json!({"index": index, "state": state.name(), "task": rec.id});
    Ok(done)
}
