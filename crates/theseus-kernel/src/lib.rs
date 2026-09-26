//! The durable kernel (spec §3.2a, §3.15, §3.16, §3.17; Part II M2).
//!
//! Executions are durable state machines; actions carry harness-minted
//! correlation ids and a declared retry class; a `Completion` is one envelope
//! from every source, accepted idempotently and settled atomically with the
//! owning execution's next state; the spool makes a finished job survive a
//! harness restart; startup is five idempotent steps; the reconciler polls
//! only overdue work. Everything is synchronous and takes its time from a
//! `Clock`, so the simulator drives it deterministically.

pub mod clock;
pub mod gate;
pub mod job;
pub mod kernel;
pub mod spool;
pub mod types;

#[cfg(test)]
mod tests;

pub use clock::{Clock, RealClock, VirtualClock};
pub use gate::{run_gate, AllowAll, GateResult, Policy, PolicyDecision, Proposal};
pub use kernel::{
    Accepted, Evidence, Kernel, KernelConfig, KernelError, KernelStats, NoEvidence, Probe,
    ReconcileReport, StartupReport, TurnEnd, TurnGuard,
};
pub use spool::Spool;
pub use types::*;
