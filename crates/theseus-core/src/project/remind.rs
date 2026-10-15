//! A milestone's date reminds (theseus-n5ui2): each open milestone with a date
//! holds two notice wakes (`Purpose::Remind`, `WakeRun::Notice`), their ids
//! made from the milestone's, so a sync finds them again with no record of
//! its own:
//!
//! - **due** at the date: `⚑ due today: Harbour survey, milestone 2: Chart the
//!   moorings`, an Interrupt like any reminder, with its four answers;
//! - **overdue** a grace later (`[projects] overdue_after_minutes`, a day),
//!   while the milestone is still open: `⚑ overdue: …`, one Interrupt with no
//!   sound (`notices::Why::Overdue`), once: a one-shot wake.
//!
//! `sync` makes the wakes match the records: a date that moved moves them (a
//! changed title or ordinal rewrites them), a closed milestone, or one whose
//! project closed, cancels them, and a date already past sets none (it fired,
//! or was set too late to mean anything). Each is one kernel frame under the
//! execution's lock, so a plain turn, which has no milestone, writes none.
//!
//! The wakes live in the owner's Home when the owner's verb sets them
//! (`Core::project_place`), else in the conversation whose call changed the
//! milestone: a reminder reaches the owner wherever its conversation posts,
//! and Home's DM when it posts nowhere (`wake_home.rs`).

use anyhow::Result;
use theseus_kernel::{wake_id, Kernel, NewWake, Purpose, WakeRun};
use theseus_protocol::notices::{MILESTONE_MARK, OVERDUE_MARK};
use theseus_protocol::tasks::{TaskKind, TaskRecord};

use super::progress::milestones;
use super::project_of;
use crate::wake::MAX_NOTE_CHARS;

/// Where a sync sets a wake that does not exist yet: an execution, and the
/// place its note posts to (`Outbox::target`), when it has one.
#[derive(Debug, Clone)]
pub struct Place {
    pub execution_id: String,
    pub target: Option<String>,
}

/// The due wake's id for a milestone: `wak_` and the milestone's id's tail,
/// then `d`.
pub fn due_id(milestone: &str) -> String {
    wake_id(&format!("act_{}d", tail(milestone)))
}

/// The overdue wake's id.
pub fn overdue_id(milestone: &str) -> String {
    wake_id(&format!("act_{}o", tail(milestone)))
}

fn tail(id: &str) -> &str {
    id.split_once('_').map_or(id, |(_, t)| t)
}

/// A reminder's note: `⚑ due today: …` or `⚑ overdue: …`. The milestone's
/// ordinal is its place among the project's milestones in the order they were
/// made, which a moved date does not change.
pub fn note(tasks: &[TaskRecord], project: &TaskRecord, m: &TaskRecord, overdue: bool) -> String {
    let mut made: Vec<&TaskRecord> = milestones(tasks, &project.id);
    made.sort_by(|a, b| (a.created_at_ms, &a.id).cmp(&(b.created_at_ms, &b.id)));
    let n = made.iter().position(|x| x.id == m.id).map_or(1, |i| i + 1);
    let lead = if overdue {
        OVERDUE_MARK
    } else {
        concat!("⚑", " due today")
    };
    let text = format!("{lead}: {}, milestone {n}: {}", project.title, m.title);
    debug_assert!(text.starts_with(MILESTONE_MARK));
    text.chars().take(MAX_NOTE_CHARS).collect()
}

/// What a sync did, for the caller's row or test.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct Synced {
    pub set: u32,
    pub moved: u32,
    pub cancelled: u32,
}

/// Make milestone `m`'s wakes match the records at `now_ms`.
pub fn sync_milestone(
    kernel: &Kernel,
    tasks: &[TaskRecord],
    m: &TaskRecord,
    (place, by): (&Place, &str),
    overdue_after_ms: u64,
) -> Result<Synced> {
    let mut done = Synced::default();
    let project = project_of(tasks, m).filter(|p| p.id != m.id);
    let live = project.is_some_and(|p| !p.state.is_closed()) && !m.state.is_closed();
    let due = m.due_at_ms.filter(|_| live);
    let now = kernel.now_ms();
    let pending = kernel.pending_wakes()?;
    for (id, at, overdue) in [
        (due_id(&m.id), due, false),
        (overdue_id(&m.id), due.map(|d| d + overdue_after_ms), true),
    ] {
        let existing = pending.iter().find(|(_, w)| w.id == id);
        let want = match (at, project) {
            (Some(at), Some(p)) => Some((at, note(tasks, p, m, overdue))),
            _ => None,
        };
        match (existing, want) {
            (Some((e, w)), Some((at, text))) => {
                if w.note != text {
                    kernel.cancel_wake(&e.id, &id, by)?;
                    set(kernel, place, (&id, at, &text), by)?;
                    done.moved += 1;
                } else if w.due_at_ms != at {
                    kernel.snooze_wake(&e.id, &id, at, false, by)?;
                    done.moved += 1;
                }
            }
            (None, Some((at, text))) if at > now => {
                set(kernel, place, (&id, at, &text), by)?;
                done.set += 1;
            }
            (Some((e, _)), None) => {
                kernel.cancel_wake(&e.id, &id, by)?;
                done.cancelled += 1;
            }
            _ => {}
        }
    }
    Ok(done)
}

fn set(
    kernel: &Kernel,
    place: &Place,
    (id, due_at_ms, note): (&str, u64, &str),
    by: &str,
) -> Result<()> {
    kernel.create_wake(
        &place.execution_id,
        Some(id.to_string()),
        NewWake {
            due_at_ms,
            note: note.to_string(),
            target: place.target.clone(),
            repeat: None,
            purpose: Purpose::Remind,
            run: WakeRun::Notice,
        },
        by,
    )?;
    Ok(())
}

/// After a change to `changed`: sync its wakes if it is a milestone, or every
/// milestone of the project if it is one (it may have closed). Any other task
/// costs a kind test and nothing else.
pub fn sync_after(
    kernel: &Kernel,
    store: &crate::store::Store,
    changed: &TaskRecord,
    (place, by): (&Place, &str),
    overdue_after_ms: u64,
) -> Result<Synced> {
    if changed.kind == TaskKind::Item {
        return Ok(Synced::default());
    }
    let tasks = crate::task_graph::all(store)?;
    let fresh = |t: &TaskRecord| tasks.iter().find(|x| x.id == t.id).unwrap_or(t).clone();
    let mut total = Synced::default();
    let mut add = |s: Synced| {
        total.set += s.set;
        total.moved += s.moved;
        total.cancelled += s.cancelled;
    };
    match changed.kind {
        TaskKind::Milestone => {
            add(sync_milestone(
                kernel,
                &tasks,
                &fresh(changed),
                (place, by),
                overdue_after_ms,
            )?);
        }
        _ => {
            for m in milestones(&tasks, &changed.id) {
                add(sync_milestone(
                    kernel,
                    &tasks,
                    m,
                    (place, by),
                    overdue_after_ms,
                )?);
            }
        }
    }
    Ok(total)
}

/// After a call's frame: sync the milestones the call changed, in the
/// calling conversation (the owner's verbs set theirs in Home). A plain item
/// costs a kind test.
pub fn after(
    rt: &crate::toolrun::ToolRuntime,
    tc: &crate::toolrun::TurnCtx<'_>,
    changes: &[(&'static str, crate::fact::task_graph::Change)],
) {
    let mut long = changes
        .iter()
        .filter(|(_, c)| c.task.kind != TaskKind::Item)
        .peekable();
    if long.peek().is_none() {
        return;
    }
    let place = Place {
        execution_id: tc.execution_id.to_string(),
        target: tc.outbox.target(tc.session_id),
    };
    let by = format!("model:{}", tc.session_id);
    for (_, c) in long {
        if let Err(e) = sync_after(
            tc.kernel,
            tc.store,
            &c.task,
            (&place, &by),
            rt.projects.overdue_after_ms(),
        ) {
            tracing::warn!(error = %format!("{e:#}"), task = %c.task.id, "a milestone's reminder was not synced");
        }
    }
}
