//! Initiative with a character (theseus-slvl.9): what Theseus says first is
//! written in its persona, a promise to follow up is measured against the
//! wake that should keep it, the persona's `initiative` sets what pings, and
//! the owner's 👍 or 👎 on a proactive message is recorded.
//!
//! - **The persona contract** (`speak`): a daemon-initiated message in words
//!   is one short model call whose system holds the persona's file (the
//!   built-in default when it has none) and the standing block, on
//!   `[initiative] profile`, off any owner turn's path, under `[initiative]
//!   day_usd`. A wake's result and a task's report are turns of the session,
//!   which speak as its persona already. A pure status line stays a template
//!   (`lines`: one table, reading the persona's name and emoji).
//! - **Promises kept** (`promise`, `turn/promise_step.rs`): a reply that
//!   promises a follow-up by the matcher's table, in a turn that scheduled no
//!   wake, task or watch, is a `promise.unkept` row; one that did is
//!   `promise.kept`.
//! - **The level** is `Rules::initiative` of `theseus_protocol::notices`: one
//!   field of the ping rule, read by `policy()` and by the daemon's own
//!   proactive posts through `Initiative::lets`.
//! - **The verdict** (`rate`): `initiative.rate` writes `initiative.rated`;
//!   `week` is the measure `theseus persona` shows.
//!
//! Tests: `tests_initiative.rs`.

pub mod consolidation;
pub mod digest;
pub mod lines;
pub mod morning;
pub mod promise;
pub mod rate;
pub mod speak;
pub mod week;
pub use morning::State;
