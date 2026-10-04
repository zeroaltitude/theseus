//! Compaction's facts (M6 step 30c): where the ring would cut, a summary
//! written (`compaction`) or the ring kept and why (`ring`), as one
//! `context.compacted` row, a `compaction` span under the loop (the
//! `theseus.compactions` metric reads it), and a narrative line; and the
//! core overage (`context.overage`): the newest exchange alone does not fit,
//! and the turn fails before any call.

use serde_json::{json, Value};
use theseus_protocol::memory::BudgetReport;
use theseus_protocol::{LedgerKind, NarrativePart::Context, Span};

use super::{Fact, Say};
use crate::narrative;
use crate::trace::Trace;

/// What became of a ring's cut: a summary in its place, or the ring.
pub struct Compacted<'a> {
    /// `compaction`, or `ring` (the summary was not written, or not used).
    pub outcome: &'a str,
    /// Why the ring ran instead.
    pub why: Option<&'a str>,
    pub profile: &'a str,
    pub model: Option<&'a str>,
    /// The messages summarized (a folded summary's included), and the
    /// positions of the range's first and last.
    pub messages: u64,
    pub first: Option<u64>,
    pub last: Option<u64>,
    /// The summary folded into this one, when there was one.
    pub folded: Option<&'a str>,
    /// The `Summary` node, its output tokens, and what its call cost.
    pub node_id: Option<&'a str>,
    pub summary_tokens: u64,
    pub input_tokens: u64,
    pub cost_usd: Option<f64>,
    pub settled_micros: Option<u64>,
    pub reserved_micros: Option<u64>,
    pub correlation_id: Option<&'a str>,
    /// The recall notes in the range, dropped and never summarized.
    pub recalls_dropped: u64,
    pub loop_index: u32,
    pub t0: u64,
    pub t1: u64,
}

impl Fact for Compacted<'_> {
    const KIND: Option<LedgerKind> = Some(LedgerKind::ContextCompacted);

    fn row(&self) -> Value {
        json!({"outcome": self.outcome, "why": self.why, "profile": self.profile,
               "model": self.model, "messages": self.messages, "first": self.first,
               "last": self.last, "folded": self.folded, "node_id": self.node_id,
               "summary_tokens": self.summary_tokens, "input_tokens": self.input_tokens,
               "cost_usd": self.cost_usd, "settled_micros": self.settled_micros,
               "reserved_micros": self.reserved_micros, "correlation_id": self.correlation_id,
               "recalls_dropped": self.recalls_dropped, "loop": self.loop_index})
    }

    fn span(&self, trace: &mut Trace) {
        trace.push(Span {
            name: "compaction".into(),
            kind: "compaction".into(),
            start_us: self.t0,
            end_us: Some(self.t1),
            attrs: json!({"outcome": self.outcome, "why": self.why, "profile": self.profile,
                "model": self.model, "messages": self.messages,
                "summary_tokens": self.summary_tokens, "cost_usd": self.cost_usd,
                "node_id": self.node_id}),
            children: Vec::new(),
        });
    }

    fn narrate(&self, say: &mut Say<'_>) {
        say.line(Context, words(self));
    }
}

/// "Compaction summarized 212 messages into 380 tokens with glm, for
/// $0.0011." Or why the ring ran instead.
pub fn words(c: &Compacted<'_>) -> String {
    if c.outcome == "compaction" {
        let folded = if c.folded.is_some() {
            ", the earlier summary folded in"
        } else {
            ""
        };
        return format!(
            "Compaction summarized {} into {} with {}{folded}, for {}.",
            narrative::count(c.messages, "message", "messages"),
            narrative::count(c.summary_tokens, "token", "tokens"),
            c.profile,
            narrative::money(c.cost_usd),
        );
    }
    format!(
        "Compaction did not run ({}): the ring dropped the earliest turns instead.",
        c.why.unwrap_or("no reason given")
    )
}

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
