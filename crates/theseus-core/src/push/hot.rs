//! The executions the seed reads (theseus-id8d): every one a surface shows
//! as needing you or working, found by the store's terms, and the most
//! recently written, never every execution. Two hundred thousand
//! conversations parked on input are two hundred thousand executions no
//! surface lists but by recency, so the board leaves the older ones cold:
//! `total` still counts them (from the terms), a session's own wait loads
//! its execution when it asks (`Push::view_or_load`), and a frame that
//! touches one loads it (`Board::apply`).
//!
//! What needs you or works, and its term: queued, running, blocked, failed,
//! out of budget, or unreadable (`s:<state>`); waiting with a due time or
//! wakes of its own (`due`, `w`); a task not ended (`ot`); an execution with
//! a call in flight or a question asked (an action not settled, which the
//! seed reads anyway); and the parent of each of those that has one, so a
//! task's view names its parent's session.

use std::collections::BTreeMap;

use theseus_kernel::{terms, Action, ExecState, Execution};

use crate::Core;

/// The most recently written executions the seed reads besides those its
/// terms find: more than any surface lists (`executions.watch`'s default
/// limit is 200).
pub(super) const RECENT: usize = 1_000;

/// At most this many execution records are read to find the recent ones: a
/// store whose executions were each written many times finds fewer.
const RECENT_WALK: usize = 64 * RECENT;

/// The states a surface shows as needing you or working, whatever waits.
const ACTIVE: [ExecState; 5] = [
    ExecState::Queued,
    ExecState::Running,
    ExecState::Blocked,
    ExecState::Failed,
    ExecState::BudgetExhausted,
];

/// The seed's executions, with the WAL position of each record, in id
/// order: those `actions` (the actions not settled) name, those the terms
/// find, the `recent` most recently written, and the parents of all of them.
pub(super) fn executions(
    core: &Core,
    actions: &[(u64, Action)],
    recent: usize,
) -> anyhow::Result<Vec<(u64, Execution)>> {
    let k = &core.kernel;
    let mut ranges: Vec<(String, String)> = ACTIVE
        .iter()
        .map(|s| terms::one(&terms::state(*s)))
        .collect();
    for t in [terms::UNREADABLE, "due", "w", terms::OPEN_TASK] {
        ranges.push(terms::one(t));
    }
    let mut by_id: BTreeMap<String, (u64, Execution)> = k
        .executions_by_at(&ranges)?
        .into_iter()
        .map(|(p, e)| (e.id.clone(), (p, e)))
        .collect();
    let mut walk = (4 * recent).clamp(1, RECENT_WALK);
    let newest = loop {
        let (read, newest) = k.newest_executions_at(walk)?;
        if newest.len() >= recent || read < walk || walk >= RECENT_WALK {
            break newest;
        }
        walk = (walk * 4).min(RECENT_WALK);
    };
    for (p, e) in newest.into_iter().take(recent) {
        by_id.entry(e.id.clone()).or_insert((p, e));
    }
    let mut named: Vec<String> = actions
        .iter()
        .map(|(_, a)| a.execution_id.clone())
        .collect();
    named.extend(by_id.values().filter_map(|(_, e)| e.parent.clone()));
    for id in named {
        if by_id.contains_key(&id) {
            continue;
        }
        if let Some((p, e)) = k.execution_at(&id)? {
            if let Some(parent) = e.parent.as_deref() {
                if let Some(read) = (!by_id.contains_key(parent))
                    .then(|| k.execution_at(parent))
                    .transpose()?
                    .flatten()
                {
                    by_id.insert(parent.to_string(), read);
                }
            }
            by_id.insert(id, (p, e));
        }
    }
    Ok(by_id.into_values().collect())
}
