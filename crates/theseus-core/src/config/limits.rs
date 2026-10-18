//! What a task's limits do when they are reached (theseus-usei): its spend
//! limit (`[kernel] spend_limit_mode`) and its loop cap (`[profiles.*]
//! on_max_loops`). The owner's decision (2026-10-07): for tasks and
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

/// `[kernel] daily_spend_ceiling_mode` (theseus-9e02). The owner, 2026-10-11:
/// "unless we're in runaway territory, we should default to flag, not stop
/// [...] tokens = less cautious, aws = more cautious."
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DailyCeilingMode {
    /// Reaching the ceiling warns, at the ceiling and at each
    /// `daily_notice_step_usd` after it, and every call is made; the day
    /// stops at `daily_runaway_usd`.
    #[default]
    Notify,
    /// Reaching the ceiling refuses every model call until the day turns
    /// (theseus-kp20's behaviour; a benchmark's own budget).
    Stop,
}

impl DailyCeilingMode {
    pub fn is_default(&self) -> bool {
        *self == Self::default()
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Notify => "notify",
            Self::Stop => "stop",
        }
    }
}

/// `[profiles.*] on_max_loops` (and `[model] on_max_loops`, the default
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

/// `[kernel] spend_limit_hard_multiple` (theseus-jse4): under notify, a
/// session's calls stop anyway at 10 times its limit, so a stuck loop has a
/// bound no owner has to watch for. 0 never stops.
pub(super) fn default_hard_multiple() -> u64 {
    theseus_kernel::default_hard_multiple()
}

/// `[kernel] daily_spend_ceiling_usd` (theseus-kp20): the daemon's backstop
/// over every model call it makes in a local day, $200 (the owner,
/// 2026-10-08: "It's OK as long as it's high -- like 200 per day"). Under
/// `daily_spend_ceiling_mode = "notify"` (theseus-9e02) it warns; under
/// `"stop"` a call past it is not made, until midnight or a higher ceiling.
pub(super) fn default_daily_ceiling_usd() -> f64 {
    200.0
}

/// `[kernel] daily_runaway_usd` (theseus-9e02): where a notifying day stops,
/// $2,000 (the owner, 2026-10-11: "we've occasionally spent upwards of 1k in
/// a day on purpose"). 0 never stops it.
pub(super) fn default_daily_runaway_usd() -> f64 {
    theseus_kernel::micros_to_usd(theseus_kernel::DEFAULT_RUNAWAY_MICROS)
}

/// `[kernel] daily_notice_step_usd`: a warning at the ceiling and every $200
/// after it. 0 warns at the ceiling alone.
pub(super) fn default_daily_notice_step_usd() -> f64 {
    theseus_kernel::micros_to_usd(theseus_kernel::DEFAULT_STEP_MICROS)
}

/// `[kernel] runaway_usd_per_10min`: a session's burn-rate line, $200 in ten
/// minutes, sized so a deliberate $1,000 day never trips it. 0 never.
pub(super) fn default_runaway_per_10min_usd() -> f64 {
    theseus_kernel::micros_to_usd(theseus_kernel::DEFAULT_BURN_MICROS)
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
    for (key, v) in [
        ("daily_runaway_usd", k.daily_runaway_usd),
        ("daily_notice_step_usd", k.daily_notice_step_usd),
        ("runaway_usd_per_10min", k.runaway_usd_per_10min),
    ] {
        if !v.is_finite() || v < 0.0 {
            anyhow::bail!("kernel.{key} = {v} must be a dollar amount, or 0");
        }
    }
    Ok(())
}

/// `[tools] parallel_runs` (theseus-d1hi): the most programs one group of a
/// response's calls runs at once, each its own job; the rest wait for a slot
/// in the same group, so the results keep their order. 0 reads as 1. Sixteen since
/// theseus-0hufe: a response of sixteen `sleep 2` ends in one wave, not two.
pub fn default_parallel_runs() -> usize {
    16
}

/// `[tools] parallel_reads` (theseus-0hufe): the most reads one group runs at
/// once; the rest wait for a slot, the results in order. 0 reads as 1.
pub fn default_parallel_reads() -> usize {
    32
}

/// `[tools] parallel_writes` (theseus-0hufe): the most writes one group runs
/// at once. 0 reads as 1.
pub fn default_parallel_writes() -> usize {
    16
}

/// `[tools] job_wait_max_secs` (theseus-7m7w.3): the longest one `job_wait`
/// waits, however long the model asks. The wait still returns at once when
/// the job ends, a person's message arrives, or the turn is stopped, so the
/// cap only bounds a wait nothing ends.
pub fn default_job_wait_max_secs() -> u64 {
    1800
}

/// `[tools] job_wait_max_secs` is at least `proc_sync_secs` (a wait is never
/// shorter than a foreground run's) and at most what the tool's deadline
/// covers (`theseus_tools::jobs::WAIT_SECS_MAX`).
pub(super) fn check_job_wait(t: &super::ToolsConfig) -> anyhow::Result<()> {
    let max = theseus_tools::jobs::WAIT_SECS_MAX;
    if t.job_wait_max_secs < t.proc_sync_secs.max(1) || t.job_wait_max_secs > max {
        anyhow::bail!(
            "tools.job_wait_max_secs = {} must be from tools.proc_sync_secs ({}) to {max}",
            t.job_wait_max_secs,
            t.proc_sync_secs
        );
    }
    Ok(())
}
