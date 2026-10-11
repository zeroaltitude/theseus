//! What a task's limits do when they are reached (theseus-usei): its spend
//! limit (`[kernel] spend_limit_mode`) and its loop cap (`[profiles.*]
//! max_loops_mode`). The owner's decision (2026-10-07): for tasks and
//! tokens, notify, not restrict. Long tasks failed at the budget, so by
//! default reaching either posts a notice and the work goes on; today's
//! restriction stays one line away. AWS budgets are managed apart and still
//! restrict (`[aws.accounts.*] monthly_budget_usd`).

use serde::{Deserialize, Serialize};

/// `[kernel] spend_limit_mode`.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SpendLimitMode {
    /// At the limit, and at each multiple of it after, the session's place
    /// hears one notice, and its calls go on: a reservation past the limit
    /// is still made and recorded, never refused.
    #[default]
    Notify,
    /// At the limit the session waits, and asks the owner whether its spend
    /// may go back to $0 (theseus-0sg's question).
    Ask,
}

impl SpendLimitMode {
    pub fn is_default(&self) -> bool {
        *self == Self::default()
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Notify => "notify",
            Self::Ask => "ask",
        }
    }
}

/// `[profiles.*] max_loops_mode` (and `[model] max_loops_mode`, the default
/// profile's).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MaxLoopsMode {
    /// At `max_loops` loops, and at each multiple of it after, the session's
    /// place hears one notice, and the turn goes on.
    #[default]
    Notify,
    /// At `max_loops` the turn ends: "the loop cap is reached".
    End,
}

impl MaxLoopsMode {
    pub fn is_default(&self) -> bool {
        *self == Self::default()
    }
}

pub(super) fn default_spend_limit_usd() -> f64 {
    100.0
}

/// `[kernel] daily_spend_ceiling_usd` (theseus-kp20): the daemon's backstop
/// over every model call it makes in a local day, $200 (the owner,
/// 2026-10-08: "It's OK as long as it's high -- like 200 per day"). No mode:
/// at the ceiling a call is not made, until midnight or a higher ceiling.
pub(super) fn default_daily_ceiling_usd() -> f64 {
    200.0
}

/// `[kernel]`'s dollar amounts: each above zero and finite.
pub(super) fn check_dollars(k: &super::KernelSection) -> anyhow::Result<()> {
    for (key, v) in [
        ("spend_limit_usd", k.spend_limit_usd),
        ("daily_spend_ceiling_usd", k.daily_spend_ceiling_usd),
    ] {
        if !v.is_finite() || v <= 0.0 {
            anyhow::bail!("kernel.{key} = {v} must be a dollar amount above zero");
        }
    }
    Ok(())
}

/// `[tools] parallel_runs` (theseus-d1hi): the most programs one group of a
/// response's calls runs at once, each its own job; the rest wait for a slot
/// in the same group, so the results keep their order. 0 reads as 1.
pub fn default_parallel_runs() -> usize {
    8
}
