//! The durable kernel (spec §3.2a, §3.15, §3.16, §3.17; Part II M2).
//!
//! Executions are durable state machines; actions carry harness-minted
//! correlation ids and a declared retry class; a `Completion` is one envelope
//! from every source, accepted idempotently and settled atomically with the
//! owning execution's next state; the spool makes a finished job survive a
//! harness restart; startup is five idempotent steps; the reconciler polls
//! only overdue work. Everything is synchronous and takes its time from a
//! `Clock`, so the simulator drives it deterministically.

pub mod children;
pub mod clock;
pub mod gate;
pub mod job;
pub mod kernel;
mod locks;
pub mod outbox;
pub mod spool;
pub mod tasks;
pub mod types;

#[cfg(test)]
mod tests;
#[cfg(test)]
mod tests_tasks;

pub use clock::{Clock, RealClock, VirtualClock};
pub use gate::{digest_json, digest_proposal, Proposal};
pub use kernel::{
    Accepted, Evidence, Kernel, KernelConfig, KernelError, KernelStats, LegacySpend, LimitFollowed,
    NoEvidence, Probe, ReconcileReport, StartupReport, TurnEnd, TurnGuard,
};
pub use outbox::{Post, Settled, OUTBOX_TOOL};
pub use spool::Spool;
pub use tasks::{carve_key, task_ids, TaskOpen};
pub use types::*;
