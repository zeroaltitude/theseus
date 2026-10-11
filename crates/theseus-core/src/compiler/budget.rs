//! A compilation's budget (moved out of `compiler.rs` at theseus-3ki0): the
//! tokens a request may hold, and its report.

use theseus_protocol::memory::{BudgetDrop, BudgetOverage, BudgetReport};

use super::{Estimate, RequestSpec};

/// The tokens a request may hold in window `w`: the window less the output
/// cap and a margin.
pub(super) fn request_budget(w: u64, spec: &RequestSpec) -> u64 {
    w.saturating_sub(spec.max_tokens as u64)
        .saturating_sub(4_096)
}

/// A compilation's report: its limit (0 with no window known), what the
/// request is estimated at, the ring's cut, and an overage when the
/// estimate's upper bound still passes the limit.
pub(super) fn budget_report(
    limit: Option<u64>,
    est: &Estimate,
    dropped: impl IntoIterator<Item = BudgetDrop>,
) -> BudgetReport {
    let overage = limit.filter(|l| est.upper > *l).map(|l| BudgetOverage {
        tokens: est.upper - l,
        why: "the request's estimate passes the window with every turn the ring may drop gone"
            .into(),
    });
    BudgetReport {
        limit_tokens: limit.unwrap_or(0),
        used_tokens: est.tokens,
        dropped: dropped.into_iter().collect(),
        overage,
    }
}
