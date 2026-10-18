//! The sealed holdout sets' facts (theseus-pw1q.5): `self.exam.counted`, an
//! exam item counted after its independent review, and `self.sealed.scored`,
//! a set scored against a candidate. Neither names any text of an item: a
//! sealed item's row carries its opaque id and family, a score's its counts,
//! rates and opaque failure ids.

use serde_json::{json, Value};
use theseus_protocol::LedgerKind;

use super::Fact;
use crate::rsi::sealed::Scores;

/// An exam item counted. `sealed` rows carry the item id alone.
pub struct SelfExamCounted<'a> {
    pub item: &'a str,
    pub family: &'a str,
    pub sealed: bool,
    pub reviewer: &'a str,
}

impl Fact for SelfExamCounted<'_> {
    const KIND: Option<LedgerKind> = Some(LedgerKind::SelfExamCounted);

    fn row(&self) -> Value {
        json!({"item": self.item, "family": self.family, "sealed": self.sealed,
               "reviewer": self.reviewer,
               "what": format!("exam item {} counted ({}{})", self.item,
                   if self.sealed { "sealed, " } else { "" }, self.family),
               "why": null, "numbers": null, "undo": null})
    }
}

/// A sealed set scored against a candidate's build (theseus-pw1q.21.5): one
/// row for each (set, branch, head), from the scorer's output, which has
/// passed the filter. `cost_usd` is what the model relay metered.
pub struct SelfSealedScored<'a> {
    pub branch: &'a str,
    pub head: &'a str,
    pub scores: &'a Scores,
    pub cost_usd: f64,
}

impl Fact for SelfSealedScored<'_> {
    const KIND: Option<LedgerKind> = Some(LedgerKind::SelfSealedScored);

    fn row(&self) -> Value {
        let s = self.scores;
        json!({"set": s.set, "branch": self.branch, "head": self.head,
               "items": s.items, "passed": s.passed, "failed": s.failed, "errors": s.errors,
               "rate": s.rate, "cost_usd": self.cost_usd,
               "families": s.families, "failures": s.failures,
               "what": format!("sealed set {} scored for {}: {}/{} passed ({} errors)",
                   s.set, self.branch, s.passed, s.items, s.errors),
               "why": null,
               "numbers": {"rate": s.rate, "cost_usd": self.cost_usd},
               "undo": null})
    }
}
