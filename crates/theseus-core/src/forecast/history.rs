//! The table's history (theseus-zz752): the newest finished tasks, read once
//! after serving from what the store already holds: the task executions (one
//! scan of the `t:` terms) and the task graph's records. Nothing here writes,
//! and a task that ended while the daemon ran arrives through the board
//! instead (`Book::ended`), so a task is a row once.

use std::collections::HashMap;

use theseus_kernel::{micros_to_usd, ExecState};

use super::{class_of, terms_of, Row, KEEP, MAX_MINUTES};
use crate::Core;

/// A finished task's row, from its execution and its record. None for a
/// task that was cancelled (it did not finish), has no end yet, or ran past
/// `MAX_MINUTES`.
pub fn row_of(
    e: &theseus_kernel::Execution,
    t: &theseus_protocol::tasks::TaskRecord,
) -> Option<Row> {
    if !matches!(
        e.state,
        ExecState::Complete | ExecState::Failed | ExecState::BudgetExhausted
    ) {
        return None;
    }
    let min = e.updated_at_ms.saturating_sub(e.created_at_ms) as f64 / 60_000.0;
    if min > MAX_MINUTES {
        return None;
    }
    Some(Row {
        task: t.id.clone(),
        class: class_of(t).to_string(),
        terms: terms_of(&t.title, &t.objective),
        usd: micros_to_usd(e.budget.spent_micros),
        min,
        loops: None,
        started_ms: e.created_at_ms,
        ended_ms: e.updated_at_ms,
    })
}

/// The newest `KEEP` finished tasks. Blocking: call it on the blocking pool.
/// A store that cannot be read gives none, and the table is ready all the
/// same, so a forecast says "no forecast yet" rather than waiting.
pub fn read_history(core: &Core) -> Vec<Row> {
    let Ok(tasks) = core.kernel.tasks(None) else {
        return Vec::new();
    };
    let records: HashMap<String, theseus_protocol::tasks::TaskRecord> =
        crate::task_graph::all(&core.store)
            .unwrap_or_default()
            .into_iter()
            .filter_map(|t| Some((crate::task_graph::execution_of(&t)?, t)))
            .collect();
    let mut rows: Vec<Row> = tasks
        .iter()
        .filter_map(|e| row_of(e, records.get(&e.id)?))
        .collect();
    rows.sort_by_key(|r| r.ended_ms);
    let from = rows.len().saturating_sub(KEEP);
    rows.split_off(from)
}
