//! The self budget's facts (theseus-pw1q.3): `self.budget.reserved`,
//! `self.budget.settled` and `self.budget.stopped`, each written in the frame
//! that moves the account (`crate::rsi::budget`). Every `self.*` row carries
//! `what`, `why`, `numbers` and `undo` beside its own fields
//! (`theseus_protocol::rsi::SELF_ROWS`), so `self.log` shows it as written.

use serde_json::{json, Value};
use theseus_protocol::LedgerKind;

use super::Fact;

fn on(branch: Option<&str>) -> String {
    branch.map_or_else(|| "the day".to_string(), |b| format!("branch {b}"))
}

/// A self step held its worst case on the account.
pub struct BudgetReserved<'a> {
    pub reservation: &'a str,
    pub branch: Option<&'a str>,
    pub kind: &'a str,
    pub reserved_usd: f64,
    /// The day's settled and reserved spend, with this hold.
    pub day_usd: f64,
}

impl Fact for BudgetReserved<'_> {
    const KIND: Option<LedgerKind> = Some(LedgerKind::SelfBudgetReserved);

    fn row(&self) -> Value {
        json!({"reservation": self.reservation, "branch": self.branch, "kind": self.kind,
               "reserved_usd": self.reserved_usd, "day_usd": self.day_usd,
               "what": format!("a {} step reserved ${:.4} on {}", self.kind, self.reserved_usd, on(self.branch)),
               "why": null,
               "numbers": {"reserved_usd": self.reserved_usd, "day_usd": self.day_usd},
               "undo": null})
    }
}

/// A reservation settled at what the step spent.
pub struct BudgetSettled<'a> {
    pub reservation: &'a str,
    pub branch: Option<&'a str>,
    pub kind: &'a str,
    pub spent_usd: f64,
    pub released_usd: f64,
    /// A restart settled it at its worst case: the process that held it ended.
    pub recovered: bool,
}

impl Fact for BudgetSettled<'_> {
    const KIND: Option<LedgerKind> = Some(LedgerKind::SelfBudgetSettled);

    fn row(&self) -> Value {
        let why = self.recovered.then_some(
            "the daemon restarted with this reservation open, so it counts at its worst case",
        );
        json!({"reservation": self.reservation, "branch": self.branch, "kind": self.kind,
               "spent_usd": self.spent_usd, "released_usd": self.released_usd,
               "recovered": self.recovered,
               "what": format!("a {} step on {} spent ${:.4}", self.kind, on(self.branch), self.spent_usd),
               "why": why,
               "numbers": {"spent_usd": self.spent_usd, "released_usd": self.released_usd},
               "undo": null})
    }
}

/// A self step stopped at a budget line.
pub struct BudgetStopped<'a> {
    pub branch: Option<&'a str>,
    /// `branch` or `day`.
    pub line: &'a str,
    pub line_usd: f64,
    pub spent_usd: f64,
    pub asked_usd: f64,
    pub kind: &'a str,
}

impl Fact for BudgetStopped<'_> {
    const KIND: Option<LedgerKind> = Some(LedgerKind::SelfBudgetStopped);

    fn row(&self) -> Value {
        json!({"branch": self.branch, "line": self.line, "line_usd": self.line_usd,
               "spent_usd": self.spent_usd, "asked_usd": self.asked_usd, "kind": self.kind,
               "what": format!("{} stopped at the self budget's {} line, ${:.2}", on(self.branch), self.line, self.line_usd),
               "why": format!("${:.2} was spent or reserved and a {} step asked for ${:.4} more",
                              self.spent_usd, self.kind, self.asked_usd),
               "numbers": {"line_usd": self.line_usd, "spent_usd": self.spent_usd},
               "undo": "the owner raises [self.budget] and restarts the daemon"})
    }
}
