//! What becomes of a routine's run when it ends (theseus-52tut): the pure and
//! shared parts. A run is told once it is over:
//!
//! - **Complete** — nothing but the report the task posts (and `speak` rules).
//! - **Failed** or **blocked** — one Interrupt a routine a local day, with the
//!   streak (`notices::routine_line`), however many runs fail that day; the
//!   task's own failure report is not posted (`replaced`), the routine's notice
//!   tells it. Blocked is a call outside the grant that nobody answered by the
//!   run's deadline: the run is cancelled with `routine.blocked:<task>` naming
//!   the tool (`Core::routine_block`, `rpc/routines_watch.rs`).
//! - **Cancelled** by hand — nothing: the owner did it.
//!
//! The ping book (`RoutinePing`, META `routine.ping:<routine>`) remembers the
//! last run settled and the day of the last Interrupt, so a restart tells
//! nothing twice.

use theseus_kernel::{ExecState, Execution};
use theseus_protocol::routines::{
    RoutineBlocked, RoutinePing, RoutineSpec, BLOCKED_PREFIX, PING_PREFIX, RUN_PREFIX,
};

use super::Env;
use crate::store::Store;

/// How the newest runs are read for a streak: no routine fails further back
/// than this and is still counted.
const STREAK_LOOK: usize = 30;

/// What a run came to.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Outcome {
    Running,
    Complete,
    Failed(String),
    Blocked(RoutineBlocked),
    /// Stopped by hand (or the routine deleted): not a failure.
    Cancelled,
}

impl Outcome {
    /// A failure or a block: the two a streak counts.
    pub fn is_trouble(&self) -> bool {
        matches!(self, Outcome::Failed(_) | Outcome::Blocked(_))
    }
}

/// The run `e` (a routine's task) came to.
pub fn outcome(store: &Store, e: &Execution) -> Outcome {
    if let Ok(Some(b)) =
        store.get_meta::<RoutineBlocked>(&format!("{BLOCKED_PREFIX}{}", e.session_id))
    {
        return Outcome::Blocked(b);
    }
    match e.state {
        ExecState::Complete => Outcome::Complete,
        ExecState::Cancelled => Outcome::Cancelled,
        ExecState::Failed | ExecState::BudgetExhausted => Outcome::Failed(
            e.ended_reason
                .clone()
                .filter(|r| !r.trim().is_empty())
                .unwrap_or_else(|| "no reason given".to_string()),
        ),
        _ => Outcome::Running,
    }
}

/// How many of the routine's latest runs in a row failed or were blocked. A
/// run going, and one stopped by hand, are passed over; a complete one ends
/// the row.
pub fn streak(env: &Env<'_>, spec: &RoutineSpec) -> u32 {
    let Ok(tasks) = env.kernel.tasks(Some(&spec.execution_id)) else {
        return 0;
    };
    let mut n = 0;
    for t in tasks.iter().rev().take(STREAK_LOOK) {
        match outcome(env.store, t) {
            Outcome::Complete => break,
            Outcome::Failed(_) | Outcome::Blocked(_) => n += 1,
            Outcome::Running | Outcome::Cancelled => {}
        }
    }
    n
}

/// Whether the run `task_session` is a routine's (it has its mark).
pub fn is_run(store: &Store, task_session: &str) -> bool {
    store
        .get_meta::<theseus_protocol::routines::RunMark>(&format!("{RUN_PREFIX}{task_session}"))
        .ok()
        .flatten()
        .is_some()
}

/// A routine run's report that the routine's own notice replaces: a run that
/// did not complete is told once a day by the routine (`Core::routine_settle`),
/// not by a report each time it fails.
pub fn replaced(store: &Store, task_session: &str, outcome: &str) -> bool {
    outcome != "complete" && is_run(store, task_session)
}

/// The ping book of a routine.
pub fn ping(store: &Store, routine: &str) -> RoutinePing {
    store
        .get_meta(&format!("{PING_PREFIX}{routine}"))
        .ok()
        .flatten()
        .unwrap_or_default()
}

/// The ping book as a record, to write in the frame that tells the run.
pub fn ping_record(routine: &str, p: &RoutinePing) -> anyhow::Result<theseus_store::NewRecord> {
    theseus_store::NewRecord::json(
        theseus_store::kinds::META,
        Some(&format!("{PING_PREFIX}{routine}")),
        p,
    )
}

/// The words of a blocked run's reason.
pub fn blocked_reason(tool: &str) -> String {
    format!("{tool} needs approval (not in its grant)")
}

/// How long a question of `session` may wait: a routine's run waits its run
/// deadline (`[routines] run_deadline_secs`), anything else the confirm
/// lifetime. A session that is no task's costs no read.
pub fn ask_ttl_ms(
    store: &Store,
    run_deadline_secs: u64,
    confirm_ttl_ms: u64,
    session: &str,
    is_task: bool,
) -> u64 {
    if is_task && is_run(store, session) {
        run_deadline_secs * 1000
    } else {
        confirm_ttl_ms
    }
}

impl crate::turn::TurnRunner {
    /// `ask_ttl_ms` for a turn's own session.
    pub(crate) fn ask_ttl_ms(&self, store: &Store, session: &str, is_task: bool) -> u64 {
        ask_ttl_ms(
            store,
            self.tools.routines.cfg.run_deadline_secs,
            self.kernel.config().confirm_ttl_ms,
            session,
            is_task,
        )
    }
}
