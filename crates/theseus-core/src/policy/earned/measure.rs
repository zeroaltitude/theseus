//! The number (theseus-uttu): interrupts per active hour over the last four
//! weeks, and how many of those the current grants would have saved. It is
//! the proof's measure: the target is 60 % fewer interrupts, and the
//! owner's daemon shows it by itself after a few weeks of grants.
//!
//! An interrupt is a question a person was asked (`tool.confirm_requested`;
//! a budget question is no tool's). An active hour is a clock hour in which
//! a turn began or a question was asked, so an hour of the owner's absence
//! counts for nothing either way. Pure: rows in, numbers out.

use std::collections::BTreeSet;
use std::path::PathBuf;

use theseus_protocol::earned::{EarnedGrant, EarnedWeek};

use super::find::{asks, Ask};
use crate::ledger::LedgerRow;

pub const WEEK_MS: u64 = 7 * 86_400_000;
const HOUR_MS: u64 = 3_600_000;
pub const WEEKS: u64 = 4;

/// What the number says.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Measure {
    pub weeks: Vec<EarnedWeek>,
    pub interrupts: u64,
    pub active_hours: u64,
    pub per_hour: f64,
    pub saved: u64,
    pub per_hour_with_grants: f64,
}

fn rate(n: u64, hours: u64) -> f64 {
    if hours == 0 {
        0.0
    } else {
        n as f64 / hours as f64
    }
}

/// Whether a grant covers an ask as the gate would have: the same match,
/// on what the question recorded.
fn saves(g: &EarnedGrant, a: &Ask) -> bool {
    if !a.posture || a.floor || a.external {
        return false;
    }
    let argv = a.argv.clone().unwrap_or_default();
    let path: Vec<PathBuf> = a.path.iter().cloned().collect();
    super::covers(&g.group, &a.tool, &argv, &path)
}

/// The four weeks that end at `now_ms`, from `confirms` (the questions'
/// rows) and `turns` (when turns began), with `grants` as they stand.
pub fn measure(
    confirms: &[LedgerRow],
    turns: &[u64],
    grants: &[EarnedGrant],
    now_ms: u64,
) -> Measure {
    let start = now_ms.saturating_sub(WEEKS * WEEK_MS);
    let asked = asks(confirms);
    let counted: Vec<&Ask> = asked
        .values()
        .filter(|a| a.at_ms >= start && a.at_ms <= now_ms && !a.tool.starts_with("budget."))
        .collect();
    let mut out = Measure::default();
    let mut all_hours = BTreeSet::new();
    for w in 0..WEEKS {
        let (since, until) = (start + w * WEEK_MS, start + (w + 1) * WEEK_MS);
        let here = |ms: u64| ms >= since && (ms < until || (w == WEEKS - 1 && ms <= until));
        let mut hours = BTreeSet::new();
        let mut n = 0;
        for a in counted.iter().filter(|a| here(a.at_ms)) {
            n += 1;
            hours.insert(a.at_ms / HOUR_MS);
        }
        for t in turns.iter().filter(|t| here(**t)) {
            hours.insert(*t / HOUR_MS);
        }
        out.weeks.push(EarnedWeek {
            since_ms: since,
            until_ms: until,
            interrupts: n,
            active_hours: hours.len() as u64,
            per_hour: rate(n, hours.len() as u64),
        });
        out.interrupts += n;
        all_hours.extend(hours);
    }
    out.active_hours = all_hours.len() as u64;
    out.per_hour = rate(out.interrupts, out.active_hours);
    out.saved = counted
        .iter()
        .filter(|a| grants.iter().any(|g| saves(g, a)))
        .count() as u64;
    out.per_hour_with_grants = rate(out.interrupts - out.saved, out.active_hours);
    out
}
