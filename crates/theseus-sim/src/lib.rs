//! The simulator's fakes that other crates' tests use: a stand-in for
//! Discord's REST API (theseus-q4v) and its gateway (theseus-6g62). The
//! `theseus-sim` binary serves them too (`theseus-sim fake-discord`), for
//! scratch daemons.

pub mod discord_proof;
pub mod fake_discord;
pub mod fake_gateway;
