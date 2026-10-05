//! The durable kernel (spec §3.2a, §3.15, §3.16, §3.17; Part II M2).
//!
//! Executions are durable state machines; actions carry harness-minted
//! correlation ids and a declared retry class; a `Completion` is one envelope
//! from every source, accepted idempotently and settled atomically with the
//! owning execution's next state; the spool makes a finished job survive a
//! harness restart; startup is five idempotent steps; the reconciler polls
//! only overdue work. Everything is synchronous and takes its time from a
//! `Clock`, so the simulator drives it deterministically.

mod cancels;
pub mod children;
pub mod clock;
pub mod gate;
pub mod job;
mod job_egress;
mod job_l1;
mod job_wait;
pub mod kernel;
mod locks;
pub mod mcp_l1;
pub mod outbox;
pub mod place_limit;
pub mod redact;
mod reopen;
pub mod repeat;
pub mod spend;
pub mod spool;
pub mod stops;
pub mod tasks;
pub mod terms;
pub mod tree;
mod tx;
pub mod types;
pub mod umask;
pub mod wakes;

#[cfg(test)]
mod tests;
#[cfg(test)]
mod tests_budgets;
#[cfg(test)]
mod tests_frames;
#[cfg(test)]
mod tests_place_limit;
#[cfg(test)]
mod tests_repeat;
#[cfg(test)]
mod tests_spool;
#[cfg(test)]
mod tests_stops;
#[cfg(test)]
mod tests_tasks;
#[cfg(test)]
mod tests_terms;
#[cfg(test)]
mod tests_tx;
#[cfg(test)]
mod tests_wakes;

pub use clock::{Clock, RealClock, VirtualClock};
pub use gate::{digest_json, digest_proposal, Proposal};
pub use kernel::{
    Accepted, Cancel, Committed, Ending, Evidence, Kernel, KernelConfig, KernelError, KernelStats,
    LegacySpend, LimitFollowed, NoEvidence, Observer, Probe, ReconcileReport, StartupReport,
    TurnEnd, TurnGuard,
};
pub use outbox::{Post, Settled, OUTBOX_TOOL};
pub use repeat::{Day, Every, Repeat, TimeZone};
pub use spool::Spool;
pub use stops::Stop;
pub use tasks::{carve_key, task_ids, TakenReports, TaskOpen};
pub use types::*;
pub use wakes::{wake_id, FiredWake, WakeSet, MAX_PENDING};
