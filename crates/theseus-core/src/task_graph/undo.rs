//! A change to the owner's task that ran without asking him, and its undo
//! (issue 57).
//!
//! - **Told.** A change to his task's objective or acceptance, or its
//!   abandoning, runs at its tool's own posture, and he is told once, with an
//!   Undo. Its own row (`task.updated` or `task.closed`) says `told`, carries
//!   the record as it was before (`before`, a key of the row's free data), and
//!   is written under a key of its own (`told_key`), so the undo finds it
//!   after a restart. A self-loop item's abandoning is not told: the loop stops
//!   the item's build and closes its trees, which no record written back
//!   restarts, so it keeps its card (`keeps_card`), or follows an explicit
//!   `task.close = "notify"`, as before.
//! - **Undone.** `task.undo` writes that record back as the task's next
//!   version: every field as it was before the change, the version and the
//!   time of the change aside (they only count edits). It does so only while
//!   the task is still at the version the change made: one that moved since is
//!   refused, so a later edit is never lost, and a second undo of the same
//!   change finds the task moved and does nothing.

use anyhow::Result;
use serde_json::Value;
use theseus_store::NewRecord;

use super::TaskRecord;
use crate::store::Store;

/// Every told change's row key begins so.
pub const TOLD: &str = "task.told:";

/// The key of the row of the told change that made `version` of `task`.
pub fn told_key(task: &str, version: u64) -> String {
    format!("{TOLD}{task}:v{version}")
}

/// Whether abandoning `rec` keeps its card whatever the posture: a self-loop
/// item, whose abandoning the loop acts on at once.
pub fn keeps_card(rec: &TaskRecord) -> bool {
    rec.origin.principal == crate::rsi::backlog::PRINCIPAL
}

/// A told change's detail: `told`, and the record before it.
pub(super) fn tell(detail: &mut Value, before: &TaskRecord) -> Result<()> {
    detail["told"] = Value::Bool(true);
    detail["before"] = serde_json::to_value(before)?;
    Ok(())
}

/// Key the change row (`row`, the one `Done::row` just wrote) as told.
pub(super) fn key(row: &mut NewRecord, task: &TaskRecord) {
    row.key = Some(told_key(&task.id, task.version));
}

/// Stage the note that tells the owner of a told change (`title`, what was
/// done), into the change's own frame (`done`): in which conversation, the
/// task by its id (and its title only where the title is his own words), and
/// its undo. It quotes nothing the change wrote.
pub(super) fn tell_owner(
    tc: &crate::toolrun::TurnCtx<'_>,
    rec: &TaskRecord,
    title: &str,
    done: &mut super::tools::Done<'_>,
) -> Result<(), String> {
    let named = match super::title_hand(tc.store, rec) {
        Ok(crate::content::Hand::Owner) => format!(" (\"{}\")", rec.title),
        _ => String::new(),
    };
    let undo = crate::owner_note::told::Undo::TaskUndo {
        task: rec.id.clone(),
        version: rec.version,
    };
    let (post, records) = crate::owner_note::told::stage(
        tc.outbox,
        Some(tc.session_id),
        title,
        &format!(
            "In conversation {}, without asking you",
            crate::narrative::short(tc.session_id)
        ),
        &format!(
            "Task {}{named} is at version {}. The undo puts version {} back, as long as \
             nothing has changed the task since.",
            rec.id,
            rec.version,
            rec.version - 1
        ),
        &undo,
    )
    .map_err(|e| format!("{e:#}"))?;
    done.records.extend(records);
    done.posts.push(post);
    Ok(())
}

/// The record before the told change that made `version` of `task`: none when
/// no told change made it.
pub fn before(store: &Store, task: &str, version: u64) -> Result<Option<TaskRecord>> {
    let Some(r) = store.ledger_by_key(&told_key(task, version))? else {
        return Ok(None);
    };
    let row: crate::ledger::LedgerRow = r.decode()?;
    match row.data.get("before") {
        Some(b) => Ok(Some(serde_json::from_value(b.clone())?)),
        None => Ok(None),
    }
}

/// The record that undoes the told change that made `version`, given the task
/// as it is now (`now`, read under its lock): the record before it, as the
/// next version, and the version it was. Or why not, in words the owner reads.
pub fn restored(
    store: &Store,
    now: &TaskRecord,
    version: u64,
    at_ms: u64,
) -> Result<(TaskRecord, u64), String> {
    let before = before(store, &now.id, version)
        .map_err(|e| format!("{e:#}"))?
        .ok_or_else(|| {
            format!(
                "no change that ran without asking you made version {version} of task {}",
                now.id
            )
        })?;
    if now.version != version {
        return Err(format!(
            "task {} has changed since (it is at version {}, the change made {version}): \
             nothing was undone",
            now.id, now.version
        ));
    }
    let was = before.version;
    let back = TaskRecord {
        version: now.version + 1,
        updated_at_ms: at_ms,
        ..before
    };
    Ok((back, was))
}
