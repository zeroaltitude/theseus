//! Theseus kernel. See `docs/the-ship-of-theseus.md`.
//!
//! M0 First light: config and secrets from 1Password, every hook event defined
//! and registerable with nothing firing, a turn runner with an empty tool
//! list, an Advancer whose only policy is `stop_after_one_loop`, and a small
//! embedded store for session records and ledger rows.

pub mod advancer;
pub mod config;
pub mod github;
pub mod hooks;
pub mod ledger;
pub mod provider;
pub mod rpc;
pub mod secrets;
pub mod session;
pub mod store;
pub mod telemetry;
pub mod trace;
pub mod turn;

pub use config::Config;
pub use rpc::Core;

pub const NAME: &str = "theseus";
pub const VERSION: &str = env!("CARGO_PKG_VERSION");

pub fn new_id(prefix: &str) -> String {
    format!("{prefix}_{}", uuid::Uuid::now_v7().simple())
}
