//! The context's own facts: the core overage (`context.overage`): the newest exchange alone does not fit
//! the model's window, and the turn fails before any call; and a clearing pass (`context.cleared`,
//! theseus-3ki0): old results stubbed, one row a pass.

use serde_json::{json, Value};
use theseus_protocol::memory::BudgetReport;
use theseus_protocol::{LedgerKind, NarrativePart::Context};

use super::{Fact, Say};
use crate::narrative;

/// The core overage (§2.5, theseus-3nk): even the newest exchange alone
/// does not fit the model's window, so the turn fails before any call.
pub struct Overage<'a> {
    pub model: &'a str,
    pub window: Option<u64>,
    /// The request's estimate, and its upper bound, the ring's check.
    pub estimate: u64,
    pub upper: u64,
    pub budget: &'a BudgetReport,
    pub loop_index: u32,
}

impl Fact for Overage<'_> {
    const KIND: Option<LedgerKind> = Some(LedgerKind::ContextOverage);

    fn row(&self) -> Value {
        json!({"model": self.model, "window": self.window, "limit": self.budget.limit_tokens,
               "estimate": self.estimate, "upper": self.upper,
               "over": self.budget.overage.as_ref().map(|o| o.tokens),
               "budget": self.budget, "loop": self.loop_index})
    }

    fn narrate(&self, say: &mut Say<'_>) {
        say.line(Context, format!("Context: {}", overage_words(self)));
    }
}

/// The overage in words: the window, the limit, and the estimate.
pub fn overage_words(o: &Overage<'_>) -> String {
    let n = narrative::thousands;
    let window = o
        .window
        .map_or_else(|| "window".to_string(), |w| format!("window of {}", n(w)));
    format!(
        "the newest exchange alone does not fit {}'s {window}: its request is estimated at {} \
         tokens ({} at the estimate's upper bound) against the {} the window leaves after the \
         output cap, {} over, with every earlier turn dropped. Nothing was sent.",
        o.model,
        n(o.estimate),
        n(o.upper),
        n(o.budget.limit_tokens),
        n(o.budget.overage.as_ref().map_or(0, |v| v.tokens)),
    )
}

/// A clearing pass (theseus-3ki0): past the clearing line, old results
/// became stubs in a new compilation. One a pass, never one a turn.
pub struct Cleared<'a> {
    pub compilation_id: &'a str,
    pub pass: crate::compiler::Pass,
    /// The window's budget the lines are read against.
    pub limit: u64,
    pub loop_index: u32,
}

impl Fact for Cleared<'_> {
    const KIND: Option<LedgerKind> = Some(LedgerKind::ContextCleared);

    fn row(&self) -> Value {
        let p = &self.pass;
        json!({"compilation_id": self.compilation_id, "results": p.results, "chars": p.chars,
               "kept": p.kept, "tokens_before": p.tokens_before, "tokens_after": p.tokens_after,
               "limit": self.limit, "loop": self.loop_index})
    }

    fn narrate(&self, say: &mut Say<'_>) {
        let n = narrative::thousands;
        let p = &self.pass;
        say.line(
            Context,
            format!(
                "Context: {} cleared to save context ({} characters), {} tokens to {}.",
                narrative::count(p.results, "old result", "old results"),
                n(p.chars),
                n(p.tokens_before),
                n(p.tokens_after),
            ),
        );
    }
}
