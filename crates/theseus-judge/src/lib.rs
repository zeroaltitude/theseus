//! Judgments with TypeSafe Jev (spec §3.7; design `theseus-design-m5` §2).
//!
//! A LANE crate: no kernel, store, or core types. The core decides where a
//! judgment happens and what it touches (the ledger, spend, the turn); this
//! crate holds the Jev-specific rules in one place:
//!
//! - [`client`]: the typed wire shapes (verified against the live API),
//!   strict parsing, classified errors, timeouts, and the in-flight semaphore
//!   that sheds shadow work;
//! - [`judge`]: the `Judge` trait, `JevJudge`, and `Recording`, which hands
//!   every judgment to a `JudgmentSink`;
//! - [`pack`]: packs as versioned TOML data, and the loader's rules;
//! - [`state`] and [`builders`]: states capped by construction, built from
//!   plain inputs;
//! - [`eval`]: the planted-injection eval sets, with their expected answers;
//! - [`decision`]: what a pack's deciding questions say together;
//! - [`band`], [`batch`], [`breaker`]: the three-band gate, batching by
//!   shared state, and the circuit breaker;
//! - [`price`]: the catalog-shaped price of the pinned model;
//! - [`learn`]: the learning math (calibration, holdouts, canary arms,
//!   rollback rules), pure functions;
//! - [`replay`]: what a candidate changes beside its incumbent, and a
//!   thresholds-only candidate re-banded (M5 25d);
//! - [`prove`]: the exit report, a pure generator over plain task records
//!   (`theseus-judge prove`);
//! - [`fake`] (feature `fake`): a fake Jev on 127.0.0.1.
//!
//! `jev-probe` (feature `probe`) makes real calls on synthetic states.

pub mod band;
pub mod batch;
pub mod breaker;
pub mod builders;
pub mod client;
pub mod decision;
pub mod eval;
pub mod judge;
pub mod learn;
pub mod pack;
pub mod price;
pub mod prove;
pub mod replay;
pub mod state;

#[cfg(any(test, feature = "fake"))]
pub mod fake;

#[cfg(feature = "probe")]
pub mod probe;

#[cfg(test)]
mod tests;
#[cfg(test)]
mod tests_prove;

pub use band::{Band, Banded, Thresholds};
pub use builders::{prepare, Input, Prepared};
pub use client::{
    Answer, CallError, ClientConfig, JevClient, JevError, KeySource, Request, Response, StaticKey,
    Urgency, Usage,
};
pub use decision::{decide, Decision, Verdict};
pub use judge::{
    new_id, Ask, DecisionPoint, JevJudge, Judge, Judgment, JudgmentSink, MemorySink, Mode, Outcome,
    Recording, Skip,
};
pub use pack::{Pack, Point};
pub use price::JevPrice;
pub use state::{BuiltState, NoScrub, Scrub, StateBuilder};
