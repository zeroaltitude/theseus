//! The kernel's terms (theseus-lv2): what the store's index keeps beside each
//! execution and action, so the kernel's readers ask for what they need by
//! state instead of reading every record of the kind. Ten thousand parked
//! conversations are ten thousand executions waiting on input, and none of
//! them is anything the start, the driver's tick, the reconcile, or health
//! has to read.
//!
//! An execution's terms:
//! - `s:<state>`, always (`s:waiting`, `s:queued`, …);
//! - `legacy`, stored before theseus-0sg (a unit budget), which the start
//!   rewrites once;
//! - `due`, waiting, and a due time or a task's report may wake it
//!   (`wakes::due_now`'s cases): what the driver's tick and the reconcile
//!   look at, beside the queued ones;
//! - `w`, it holds wakes of its own (`pending_wakes`);
//! - `l:<limit>`, open, and its limit follows the config's
//!   (`follows_limit`), the limit in 16 hex digits, so a changed config finds
//!   the executions with another limit in two ranges;
//! - `t:<parent>`, a task, under its parent (`tasks`).
//!
//! An action's: `s:<state>`, and `x:<execution>` while it is not settled,
//! so a stop, a cancel, or a task's end finds its execution's unsettled
//! actions without reading every action (theseus-2qt).
//!
//! The index is a projection of the WAL, and so are these: the store builds
//! them again from the records when its terms are not whole (a store an
//! older build wrote last). A change to what they say renames
//! [`PROJECTION`], so every store builds them again once.

use serde::Deserialize;
use theseus_store::{kinds, Projection, RecordKind};

use crate::types::{ActionState, ExecState, Wake, SCHEMA};

/// The kernel's projection: the store keeps these terms with every
/// execution and action it appends.
pub static PROJECTION: Projection = Projection {
    name: "terms.kernel.1",
    kinds: &[kinds::EXECUTION, kinds::ACTION],
    terms: of,
};

/// A record's terms, from its payload; none for a kind with none. A payload
/// that does not decode gets `s:?`, so a count still sees it.
pub fn of(kind: RecordKind, payload: &[u8]) -> Vec<String> {
    match kind {
        kinds::EXECUTION => execution(payload),
        kinds::ACTION => action(payload),
        _ => Some(Vec::new()),
    }
    .unwrap_or_else(|| vec![UNREADABLE.to_string()])
}

/// The term of a record whose payload does not decode.
pub const UNREADABLE: &str = "s:?";

/// Whether `terms` has one in any of `ranges` (`lo..hi`).
pub fn any_in(terms: &[String], ranges: &[(String, String)]) -> bool {
    terms
        .iter()
        .any(|t| ranges.iter().any(|(lo, hi)| lo <= t && t < hi))
}

/// The range of exactly one term.
pub fn one(term: &str) -> (String, String) {
    (term.to_string(), format!("{term}\u{1}"))
}

/// The range of every term that starts with `prefix`, which ends in an
/// ASCII character.
pub fn prefix(prefix: &str) -> (String, String) {
    let mut hi = prefix.to_string();
    let last = hi.pop().expect("a prefix");
    hi.push(char::from(last as u8 + 1));
    (prefix.to_string(), hi)
}

/// An execution's state term.
pub fn state(s: ExecState) -> String {
    format!("s:{}", s.as_str())
}

/// An action's state term.
pub fn action_state(s: ActionState) -> String {
    format!("s:{}", s.as_str())
}

/// An open execution's limit term, which follows the config's.
pub fn limit(micros: u64) -> String {
    format!("l:{micros:016x}")
}

/// The ranges of every limit term but `micros`'s: the executions whose limit
/// follows the config's and is not `micros`.
pub fn limits_other_than(micros: u64) -> [(String, String); 2] {
    let at = limit(micros);
    [
        ("l:".to_string(), at.clone()),
        (format!("{at}\u{1}"), "l;".to_string()),
    ]
}

/// The task term of a parent's tasks.
pub fn tasks_of(parent: &str) -> String {
    format!("t:{parent}")
}

/// An unsettled action's term under its execution.
pub fn unsettled_of(execution_id: &str) -> String {
    format!("x:{execution_id}")
}

/// What an execution's terms read: the fields that decide them, and no
/// others, so a stored budget in units (schema 1) decodes too.
#[derive(Deserialize)]
struct ExecutionTerms {
    #[serde(default)]
    schema: u16,
    state: ExecState,
    #[serde(default)]
    wake: Option<Wake>,
    #[serde(default)]
    wakes: Vec<serde::de::IgnoredAny>,
    #[serde(default)]
    report_wakes: Vec<serde::de::IgnoredAny>,
    #[serde(default)]
    budget: BudgetTerms,
    #[serde(default)]
    parent: Option<String>,
}

#[derive(Deserialize, Default)]
struct BudgetTerms {
    #[serde(default)]
    limit_micros: u64,
    #[serde(default)]
    pinned: bool,
}

fn execution(payload: &[u8]) -> Option<Vec<String>> {
    let e: ExecutionTerms = serde_json::from_slice(payload).ok()?;
    let mut t = vec![state(e.state)];
    let legacy = e.schema < SCHEMA;
    if legacy {
        t.push("legacy".into());
    }
    if e.state == ExecState::Waiting
        && (matches!(e.wake, Some(Wake::DueAt { .. }))
            || !e.wakes.is_empty()
            || !e.report_wakes.is_empty())
    {
        t.push("due".into());
    }
    if !e.wakes.is_empty() {
        t.push("w".into());
    }
    // A legacy budget is read with the config's limit (`from_stored`), so
    // it follows nothing until the start rewrites it.
    if !e.state.is_terminal() && !e.budget.pinned && !legacy {
        t.push(limit(e.budget.limit_micros));
    }
    if let Some(p) = e.parent {
        t.push(tasks_of(&p));
    }
    Some(t)
}

#[derive(Deserialize)]
struct ActionTerms {
    state: ActionState,
    execution_id: String,
}

fn action(payload: &[u8]) -> Option<Vec<String>> {
    let a: ActionTerms = serde_json::from_slice(payload).ok()?;
    let mut t = vec![action_state(a.state)];
    if !a.state.is_settled() {
        t.push(unsettled_of(&a.execution_id));
    }
    Some(t)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ranges_hold_their_terms_and_no_others() {
        let r = [one("s:waiting")];
        assert!(any_in(&["s:waiting".into()], &r));
        assert!(!any_in(&["s:waiting_x".into(), "s:waitin".into()], &r));
        let p = [prefix("t:")];
        assert!(any_in(&["t:exe_1".into()], &p));
        assert!(!any_in(&["s:t".into(), "u:".into()], &p));
        let other = limits_other_than(100);
        assert!(!any_in(&[limit(100)], &other));
        assert!(any_in(&[limit(99)], &other));
        assert!(any_in(&[limit(101)], &other));
        assert!(any_in(&[limit(u64::MAX)], &other));
        assert!(!any_in(&["s:waiting".into(), "t:x".into()], &other));
    }
}
