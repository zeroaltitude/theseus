//! Jev's judgments (M5, step 23a): what health says of the judge. Every
//! judgment is a `judge.call` ledger row (keyed by its id, scoped
//! `judge:<pack>`); `theseus judge log` reads them through `ledger.tail`.
//! A notified call's score follows its notice as `judge.scored` (step 24).

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

/// `judge.scored` (M5 step 24, design §2.8b): a notified call's
/// `security.v1` judgment landed, after its notice. In shadow the score is
/// uncalibrated and acts on nothing; a notice line shows it as
/// `risk 12% (shadow)`. Live progress, best effort: the judgment's
/// `judge.call` row is the record.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[serde(default)]
pub struct JudgeScored {
    pub session_id: String,
    pub turn_id: String,
    /// The call its notice named.
    pub tool_use_id: String,
    pub correlation_id: String,
    pub tool: String,
    /// `security.v1`.
    pub pack: String,
    /// The judgment's id, its `judge.call` row's key.
    pub judgment: String,
    /// `shadow`.
    pub mode: String,
    /// `risky`'s probability, 0 to 1.
    pub risky: f64,
    /// The same, as a whole percent.
    pub percent: u8,
}

impl JudgeScored {
    /// What a notice line adds: `risk 12% (shadow)`.
    pub fn line(&self) -> String {
        format!("risk {}% ({})", self.percent, self.mode)
    }
}

/// A probability as a whole percent, 0 to 100.
pub fn percent(p: f64) -> u8 {
    (p.clamp(0.0, 1.0) * 100.0).round() as u8
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_score_reads_as_a_whole_percent_in_its_mode() {
        assert_eq!(
            (percent(0.124), percent(0.995), percent(-1.0), percent(2.0)),
            (12, 100, 0, 100)
        );
        let s = JudgeScored {
            percent: percent(0.12),
            mode: "shadow".into(),
            ..Default::default()
        };
        assert_eq!(s.line(), "risk 12% (shadow)");
    }
}
