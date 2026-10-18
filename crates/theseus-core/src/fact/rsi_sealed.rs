//! The sealed holdout sets' facts (theseus-pw1q.5): `self.exam.counted`, an
//! exam item counted after its independent review, and `self.sealed.scored`,
//! a set scored against a candidate. Neither names any text of an item: a
//! sealed item's row carries its opaque id and family, a score's its counts,
//! rates and opaque failure ids.

use serde_json::{json, Value};
use theseus_protocol::LedgerKind;
use theseus_protocol::NarrativePart::Approval;

use super::{Fact, Say};
use crate::rsi::sealed::hold::{Loss, Verdict};
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

/// A self branch's sealed check before its merge (theseus-pw1q.21.7): the
/// verdict of `hold::judge`, or why the check did not run. `numbers` has
/// passed the sealed filter; nothing here is an item's text.
pub struct SelfSealedChecked<'a> {
    pub branch: &'a str,
    pub head: &'a str,
    /// `passed`, `held` or `skipped`.
    pub outcome: &'a str,
    /// Why it was skipped; none otherwise.
    pub reason: Option<&'a str>,
    pub verdict: &'a Verdict,
    /// Main's score was in the cache.
    pub cached: bool,
}

impl SelfSealedChecked<'_> {
    fn what(&self) -> String {
        match self.reason {
            Some(r) => format!("sealed check of {} skipped: {r}", self.branch),
            None => format!(
                "{} for {}: {}",
                self.verdict.line(),
                self.branch,
                self.outcome
            ),
        }
    }
}

impl Fact for SelfSealedChecked<'_> {
    const KIND: Option<LedgerKind> = Some(LedgerKind::SelfSealedChecked);

    fn row(&self) -> Value {
        let v = self.verdict;
        json!({"branch": self.branch, "head": self.head, "outcome": self.outcome,
               "before": v.before, "after": v.after, "total": v.total,
               "cached": self.cached, "holds": v.holds, "reported": v.reported,
               "broken": v.broken, "what": self.what(),
               "why": self.reason.map(str::to_string).or_else(|| v.holds().then(|| v.why())),
               "numbers": {"before": v.before, "after": v.after, "total": v.total,
                           "holds": v.holds.len(), "reported": v.reported.len()},
               "undo": null})
    }
}

/// The installed main scored worse than its baseline in one family (or a
/// set's total): one row each, from the weekly drift check.
pub struct SelfSealedDrift<'a> {
    pub loss: &'a Loss,
    /// The baseline's commit label and the one scored now.
    pub baseline: &'a str,
    pub now: &'a str,
}

impl SelfSealedDrift<'_> {
    fn what(&self) -> String {
        let l = self.loss;
        let at = l.family.as_deref().unwrap_or("total");
        format!(
            "sealed drift in {at} of set {}: {}/{} → {}/{} since the baseline",
            l.set, l.before, l.items, l.after, l.items
        )
    }
}

impl Fact for SelfSealedDrift<'_> {
    const KIND: Option<LedgerKind> = Some(LedgerKind::SelfSealedDrift);

    fn row(&self) -> Value {
        let l = self.loss;
        json!({"set": l.set, "family": l.family, "before": l.before, "after": l.after,
               "items": l.items, "baseline": self.baseline, "now": self.now,
               "what": self.what(),
               "why": "drift is cumulative and its attribution weak, so nothing is reverted",
               "numbers": {"before": l.before, "after": l.after, "items": l.items},
               "undo": null})
    }

    fn narrate(&self, say: &mut Say<'_>) {
        say.line(Approval, format!("Sealed drift: {}.", self.what()));
    }
}
