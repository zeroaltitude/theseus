//! Budgets stored in units before theseus-0sg, served in dollars: the reader,
//! and the reopen of an execution the unit budget ended whose dollar spend is
//! under its limit (theseus-3ebd).

use theseus_store::{kinds, NewRecord};

use crate::kernel::*;
use crate::tests::*;
use crate::types::*;

/// An execution as a unit budget left it (before theseus-0sg), with `rest`
/// its state and budget.
fn legacy_execution(id: &str, session: &str, rest: &str) -> NewRecord {
    let json = format!(
        r#"{{"id":"{id}","schema":1,"session_id":"{session}","kind":"conversation","authority":{{"principal":"operator","ceilings":{{}}}},"outstanding":[],"queued_results":[],"turns":15,"interrupted":0,"resume_pending":false,"created_at_ms":1790000000000,"updated_at_ms":1790000500000,{rest}}}"#
    );
    let v: serde_json::Value = serde_json::from_str(&json).unwrap();
    NewRecord::json(kinds::EXECUTION, Some(id), &v)
        .unwrap()
        .scoped(session)
}

/// What the old unit budget left when it ended an execution: the owner's
/// Discord session, 2026-09-29.
const UNITS_EXHAUSTED: &str = r#""state":"budget_exhausted","ended_reason":"action provider.messages needs 172068 units, 112317 available","budget":{"limit":1000000,"spent":877683,"reserved":0,"held_unknown":0,"control_reserve":10000,"reservations":{}}"#;

/// Executions stored with unit budgets (before theseus-0sg) serve under this
/// binary: M3.5's rule is that a new on-disk format lands with the reader for
/// the one it replaces. The reader gives each a dollar budget: the configured
/// limit, nothing reserved or held, and the unit figures kept as they were.
#[test]
fn executions_stored_with_unit_budgets_read_in_dollars() {
    let waiting = r#"{"id":"exe_old_waiting","schema":1,"session_id":"ses_old_b","kind":"conversation","state":"waiting","authority":{"principal":"operator","ceilings":{}},"budget":{"limit":20000000,"spent":154321,"reserved":0,"held_unknown":0,"control_reserve":10000,"reservations":{}},"wake":{"on":"input"},"outstanding":[],"queued_results":[],"turns":3,"interrupted":0,"resume_pending":false,"created_at_ms":1790000000000,"updated_at_ms":1790000500000}"#;
    let running = r#"{"id":"exe_old_running","schema":1,"session_id":"ses_old_c","kind":"task","state":"running","authority":{"principal":"operator","ceilings":{}},"budget":{"limit":1000000,"spent":5000,"reserved":133351,"held_unknown":2000,"control_reserve":10000,"reservations":{"rsv_old":133351}},"outstanding":[],"queued_results":[],"turns":2,"interrupted":0,"resume_pending":false,"created_at_ms":1790000000000,"updated_at_ms":1790000500000}"#;
    let w = world();
    let raw = |key: &str, session: &str, json: &str| {
        let v: serde_json::Value = serde_json::from_str(json).unwrap();
        NewRecord::json(kinds::EXECUTION, Some(key), &v)
            .unwrap()
            .scoped(session)
    };
    w.kernel
        .store()
        .append(&[
            legacy_execution("exe_old_exhausted", "ses_old_a", UNITS_EXHAUSTED),
            raw("exe_old_waiting", "ses_old_b", waiting),
            raw("exe_old_running", "ses_old_c", running),
        ])
        .unwrap();
    let read = w.kernel.execution("exe_old_exhausted").unwrap().unwrap();
    assert_eq!(read.state, ExecState::BudgetExhausted);
    assert_eq!(read.budget.limit_micros, 100 * MICROS_PER_USD);
    assert_eq!(read.budget.units_before.as_ref().unwrap().spent, 877_683);
    let y = w.kernel.execution("exe_old_waiting").unwrap().unwrap();
    assert_eq!(y.state, ExecState::Waiting);
    let z = w.kernel.execution("exe_old_running").unwrap().unwrap();
    assert_eq!(
        (z.budget.reserved_micros, z.budget.held_unknown_micros),
        (0, 0),
        "units are never read as dollars"
    );
    assert!(z.budget.reservations.is_empty());
    assert_eq!(z.budget.units_before.as_ref().unwrap().held_unknown, 2000);
}

/// The owner's store as it stands (theseus-3ebd): an earlier build migrated the
/// execution the unit budget ended, writing `budget.migrated` with $0.42 of
/// $100 spent, and left it `budget_exhausted`. This build's next start
/// reopens it, once.
#[test]
fn an_exhausted_execution_an_earlier_build_migrated_reopens_at_the_next_start() {
    let w = world();
    let mut e = Execution::from_stored(
        &legacy_execution("exe_dm", "ses_dm", UNITS_EXHAUSTED).payload,
        100 * MICROS_PER_USD,
    )
    .unwrap();
    // As the earlier build's startup wrote it: in dollars, still ended.
    e.schema = SCHEMA;
    e.budget.spent_micros = 421_600;
    w.kernel
        .store()
        .append(&[exec_record(&e).unwrap().scoped("ses_dm")])
        .unwrap();
    let (w, rep) = crash(w, KernelConfig::default());
    assert_eq!(rep.reopened, ["exe_dm"]);
    let x = w.kernel.execution("exe_dm").unwrap().unwrap();
    assert_eq!(
        (x.state, x.wake.clone(), x.ended_reason.clone()),
        (ExecState::Waiting, Some(Wake::Input), None)
    );
    assert_eq!(x.budget.spent_micros, 421_600, "its spend is kept");
    assert_eq!(rows(&w, "ses_dm", "budget.reopened").len(), 1);
    // It takes a turn now, as any waiting execution does; a crash during
    // that turn requeues it, and the start after reopens nothing again.
    let _turn = w.kernel.admit_input("exe_dm").unwrap();
    let (w, rep) = crash(w, KernelConfig::default());
    assert!(rep.reopened.is_empty());
    assert_eq!(rep.requeued_interrupted, ["exe_dm"]);
    assert_eq!(rows(&w, "ses_dm", "budget.reopened").len(), 1);
}
