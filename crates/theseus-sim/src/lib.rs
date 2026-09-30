//! The simulator's fakes that other crates' tests use: a stand-in for
//! Discord's REST API (theseus-q4v). The `theseus-sim` binary serves it too
//! (`theseus-sim fake-discord`), for scratch daemons.

pub mod fake_discord;
