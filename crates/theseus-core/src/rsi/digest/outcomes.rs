//! What the week produced, counted from the rows that say it (theseus-1q2ho).
//!
//! The digest used to headline `rows.len()`, a count of ledger rows, as
//! "changes": a week of budget reserve and settle pairs read as work. An
//! outcome is counted only from the row that is that outcome: a branch built
//! is a `self.branch.built` row, judged or not by [`super::super::infra`]'s
//! one classifier, and a join is a `self.joined` row; a join taken back is a
//! `self.reverted` or `self.vetoed` row. These rows are few, so
//! they are read whole, never through the digest's capped page.

use crate::ledger::LedgerRow;
use crate::rsi::infra::{of_row, Outcome};

/// The week's outcomes: exact counts, not a function of the page cap.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Outcomes {
    /// `self.branch.built` rows of the week, whatever their gate said. A row,
    /// not a branch: a retried item is built twice and counted twice.
    pub built: u64,
    /// The gate ran and said green.
    pub green: u64,
    /// The gate ran and said red (the keel guard's finding too).
    pub red: u64,
    /// The gate never started: the code is unjudged.
    pub infrastructure: u64,
    /// `self.joined` rows of the week.
    pub joined: u64,
    /// `self.reverted` and `self.vetoed` rows of the week. A row of this kind
    /// may take back a join of an earlier week, so it is not netted against
    /// `joined`; it is said beside it.
    pub undone: u64,
}

impl Outcomes {
    /// Count `rows` (any order) that fall in `[since, until]`.
    pub fn of_rows(rows: &[LedgerRow], since: u64, until: u64) -> Self {
        let mut o = Self::default();
        for r in rows
            .iter()
            .filter(|r| r.at_unix_ms >= since && r.at_unix_ms <= until)
        {
            match r.kind.as_str() {
                "self.branch.built" => {
                    o.built += 1;
                    match of_row(&r.data) {
                        Some(Outcome::Green) => o.green += 1,
                        Some(Outcome::Red) => o.red += 1,
                        Some(Outcome::Infrastructure) => o.infrastructure += 1,
                        None => {}
                    }
                }
                "self.joined" => o.joined += 1,
                "self.reverted" | "self.vetoed" => o.undone += 1,
                _ => {}
            }
        }
        o
    }

    /// Branches a gate judged.
    pub fn judged(&self) -> u64 {
        self.green + self.red
    }

    /// Branches with a verdict about their gate at all (judged or not).
    pub fn attempted(&self) -> u64 {
        self.judged() + self.infrastructure
    }

    /// Nothing happened that this counts.
    pub fn is_empty(&self) -> bool {
        self.built == 0 && self.joined == 0
    }

    /// The lines that lead the digest.
    pub fn lines(&self, cost: f64, cost_is_floor: bool) -> String {
        let mut out = String::from("Outcomes this week:\n");
        out.push_str(&format!(
            "- Branch builds: {} ({} with a gate verdict).\n",
            self.built,
            self.attempted()
        ));
        if self.judged() == 0 {
            // Nothing was judged, so there is no pass rate to print, whether
            // or not a gate ever started: `0/0` claims a rate over nothing.
            if self.infrastructure > 0 {
                out.push_str(&format!(
                    "- WARNING: {n} of {n} gates never started. Nothing was judged: the code is \
                     unjudged, nothing may be concluded about it, and there is no pass rate.\n",
                    n = self.infrastructure
                ));
            }
            out.push_str("- Judged: 0. Green: 0.\n");
        } else {
            out.push_str(&format!(
                "- Judged: {} (green {}, red {}); pass rate {}/{}.\n",
                self.judged(),
                self.green,
                self.red,
                self.green,
                self.judged()
            ));
            if self.infrastructure > 0 {
                out.push_str(&format!(
                    "- {} of {} gates never started: unjudged, not counted red.\n",
                    self.infrastructure,
                    self.attempted()
                ));
            }
        }
        if self.undone > 0 {
            out.push_str(&format!(
                "- Joined: {} ({} reverted or vetoed this week; a revert may undo an earlier \
                 week's join, so the two are not netted).\n",
                self.joined, self.undone
            ));
        } else {
            out.push_str(&format!("- Joined: {}.\n", self.joined));
        }
        let floor = if cost_is_floor { "at least " } else { "" };
        out.push_str(&format!("- Cost: {floor}${cost:.2}.\n"));
        out
    }
}
