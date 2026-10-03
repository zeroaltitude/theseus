//! Theseus kernel. See `docs/the-ship-of-theseus.md`.
//!
//! M0 First light: config and secrets from 1Password, a turn runner with an
//! empty tool list, an Advancer whose only policy is `stop_after_one_loop`.
//! (M0's hook events, defined but never handled, came out in theseus-hco.)
//! M1 Keel: the WAL store. M2 Kernel: executions, actions, completions, the
//! spool, admission, budgets, the harness loop (`theseus-kernel` + `harness`).

pub mod advancer;
pub mod approval;
pub mod attach;
pub mod aws;
pub mod binary;
pub mod blobs;
pub mod broker;
pub mod bus;
pub mod cancel;
pub mod catalog;
pub mod compiler;
pub mod config;
pub mod config_copy;
pub mod config_gate;
pub mod config_overlay;
pub mod context_files;
pub mod cpu;
pub mod crash;
pub mod disk;
pub mod external;
pub mod fact;
pub mod github;
pub mod graph;
pub mod harness;
pub mod labels;
pub mod ledger;
pub mod narrative;
pub mod node;
pub mod outbound;
pub mod outbox;
pub mod peer;
pub mod policy;
pub mod provider;
pub mod push;
pub mod reach;
pub mod restore;
pub mod rpc;
pub mod sandbox;
pub mod scrub;
pub mod secrets;
pub mod session;
pub mod startup;
pub mod store;
pub mod sweep;
pub mod task;
pub mod telemetry;
pub mod tender;
pub mod tighten;
pub mod toolrun;
pub mod trace;
pub mod turn;
pub mod wake;
pub mod web;
pub mod webui;

pub use config::Config;
pub use rpc::Core;

pub const NAME: &str = "theseus";
pub const VERSION: &str = env!("CARGO_PKG_VERSION");

pub fn new_id(prefix: &str) -> String {
    format!("{prefix}_{}", uuid::Uuid::now_v7().simple())
}

#[cfg(test)]
mod tests_books;
#[cfg(test)]
mod tests_cancel;
#[cfg(test)]
mod tests_config;
#[cfg(test)]
mod tests_continuations;
#[cfg(test)]
mod tests_external;
#[cfg(test)]
mod tests_failures;
#[cfg(test)]
mod tests_labels;
#[cfg(test)]
mod tests_m3;
#[cfg(test)]
mod tests_output;
#[cfg(test)]
mod tests_outside_text;
#[cfg(test)]
mod tests_overflow;
#[cfg(test)]
mod tests_push;
#[cfg(test)]
mod tests_reach;
#[cfg(test)]
mod tests_refused;
#[cfg(test)]
mod tests_registry;
#[cfg(test)]
mod tests_sandbox;
#[cfg(test)]
mod tests_schemas;
#[cfg(test)]
mod tests_tasks;
#[cfg(test)]
mod tests_tender;
#[cfg(test)]
mod tests_wakes;
