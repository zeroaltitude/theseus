//! The durable kernel (spec §3.2a, §3.15, §3.16, §3.17; Part II M2).
//!
//! Executions are durable state machines; actions carry harness-minted
//! correlation ids and a declared retry class; a `Completion` is one envelope
//! from every source, accepted idempotently and settled atomically with the
//! owning execution's next state; the spool makes a finished job survive a
//! harness restart; startup is five idempotent steps; the reconciler polls
//! only overdue work. Everything is synchronous and takes its time from a
//! `Clock`, so the simulator drives it deterministically.

mod budget_question;
mod cancels;
pub mod cgroup;
pub mod children;
pub mod clock;
pub mod cron;
pub mod day_ceiling;
mod earlier;
pub mod gate;
pub mod gone;
mod held;
pub mod job;
pub mod job_egress;
mod job_l1;
mod job_line;
mod job_wait;
pub mod kernel;
mod locks;
pub mod mcp_l1;
pub mod outbox;
mod overdraw;
pub mod patience;
pub mod place_limit;
pub mod probe_run;
pub mod redact;
mod reopen;
pub mod repeat;
pub mod retry;
pub mod routines;
mod spawn;
pub mod spend;
pub mod spool;
pub mod stops;
pub mod task_depth;
pub mod task_edit;
pub mod tasks;
pub mod terms;
pub mod tree;
mod tx;
pub mod types;
pub mod umask;
mod waiting;
pub mod wake_kinds;
pub mod wake_owner;
pub mod wakes;
pub mod watch;
pub mod when;

#[cfg(test)]
mod tests;
#[cfg(test)]
mod tests_answered;
#[cfg(test)]
mod tests_budgets;
#[cfg(test)]
mod tests_cron;
#[cfg(test)]
mod tests_day_ceiling;
#[cfg(test)]
mod tests_deferred;
#[cfg(test)]
mod tests_earlier;
#[cfg(test)]
mod tests_enosys;
#[cfg(test)]
mod tests_frames;
#[cfg(test)]
mod tests_gone;
#[cfg(test)]
mod tests_held;
#[cfg(test)]
mod tests_overdraw;
#[cfg(test)]
mod tests_patience;
#[cfg(test)]
mod tests_place_limit;
#[cfg(test)]
mod tests_repeat;
#[cfg(test)]
mod tests_retry;
#[cfg(test)]
mod tests_routines;
#[cfg(test)]
mod tests_runaway;
#[cfg(test)]
mod tests_spool;
#[cfg(test)]
mod tests_stops;
#[cfg(test)]
mod tests_task_edit;
#[cfg(test)]
mod tests_tasks;
#[cfg(test)]
mod tests_terms;
#[cfg(test)]
mod tests_tx;
#[cfg(test)]
mod tests_waiting;
#[cfg(test)]
mod tests_wake_owner;
#[cfg(test)]
mod tests_wakes;

pub use clock::{Clock, RealClock, VirtualClock};
pub use day_ceiling::{
    BurnStop, DayCeiling, Hold, Reached, Seed, Today, Warning, DEFAULT_BURN_MICROS,
    DEFAULT_RUNAWAY_MICROS, DEFAULT_STEP_MICROS,
};
pub use earlier::EARLIER_PROCESS;
pub use gate::{digest_json, digest_proposal, Proposal};
pub use kernel::{
    Accepted, Cancel, Committed, Ending, Evidence, Kernel, KernelConfig, KernelError, KernelStats,
    LimitFollowed, NoEvidence, Observer, Probe, ReconcileReport, StartupReport, TurnEnd, TurnGuard,
};
pub use outbox::{Post, Settled, OUTBOX_TOOL};
pub use overdraw::{default_hard_multiple, HardStop};
pub use repeat::{Day, Every, Repeat, TimeZone};
pub use routines::{Reschedule, RoutineDue, RoutineFired, RoutinePlan, RunPlan};
pub use spool::Spool;
pub use stops::Stop;
pub use tasks::{carve_key, task_ids, Opening, TakenReports, TaskOpen};
pub use types::*;
pub use waiting::WaitingByWake;
pub use wake_kinds::{NewWake, Purpose, TaskRun, WakeLimits, WakeProbe, WakeRun};
pub use wake_owner::{NoticeFired, NoticePost};
pub use wakes::{wake_id, FiredWake, MoveTo, WakeMoved, WakeSet, MAX_PENDING};
pub use watch::{ProbeApplied, ProbeNews, ProbeVerdict, WatchState};
