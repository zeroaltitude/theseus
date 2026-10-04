//! Jev's judgments (M5, step 23a): what health says of the judge. Every
//! judgment is a `judge.call` ledger row (keyed by its id, scoped
//! `judge:<pack>`); `theseus judge log` reads them through `ledger.tail`.

use serde::{Deserialize, Serialize};

/// Health's `judge` block.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct JudgeHealth {
    /// `[judge] enabled`. Off: nothing below is counted.
    pub enabled: bool,
    /// `[judge] max_mode`: the ceiling on every pack.
    pub max_mode: String,
    /// Each wired pack and the mode it runs in now (`loop.v1: shadow`).
    pub packs: Vec<String>,
    /// The circuit breaker: `closed`, `open (Ns left)`, `half_open`, or
    /// `idle` before the first judgment builds the client.
    pub breaker: String,
    pub in_flight: u64,
    /// The local day the counts below are of.
    pub day: String,
    /// Judgments that reached Jev today, and those that failed there.
    pub calls_today: u64,
    pub failed_today: u64,
    /// Judgments skipped today without a call: the shadow budget's pause,
    /// shedding, the breaker, an unsettled key.
    pub skipped_today: u64,
    /// What shadow judgments spent today, and the day's limit, in dollars.
    pub spend_today_usd: f64,
    pub shadow_limit_usd: f64,
    /// The shadow budget's limit is reached: shadow is paused until midnight.
    pub paused: bool,
}
