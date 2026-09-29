//! Theseus kernel. See `docs/the-ship-of-theseus.md`.
//!
//! M0 First light: config and secrets from 1Password, a turn runner with an
//! empty tool list, an Advancer whose only policy is `stop_after_one_loop`.
//! (M0's hook events, defined but never handled, came out in theseus-hco.)
//! M1 Keel: the WAL store. M2 Kernel: executions, actions, completions, the
//! spool, admission, budgets, the harness loop (`theseus-kernel` + `harness`).

pub mod advancer;
pub mod bus;
pub mod catalog;
pub mod compiler;
pub mod config;
pub mod github;
pub mod harness;
pub mod ledger;
pub mod node;
pub mod policy;
pub mod provider;
pub mod restore;
pub mod rpc;
pub mod scrub;
pub mod secrets;
pub mod session;
pub mod store;
pub mod telemetry;
pub mod toolrun;
pub mod trace;
pub mod turn;

pub use config::Config;
pub use rpc::Core;

pub const NAME: &str = "theseus";
pub const VERSION: &str = env!("CARGO_PKG_VERSION");

pub fn new_id(prefix: &str) -> String {
    format!("{prefix}_{}", uuid::Uuid::now_v7().simple())
}

#[cfg(test)]
mod tests_m3;
