//! A thread made from the owner's earlier harness (theseus-71ib): one row for
//! each thread an import applied, with the card's call (model, tokens, cost),
//! the episodes its lines cite, and why it has no card when it has none. The
//! cost is the day's model spend (`day_ceiling::SPEND_KINDS`). The row holds
//! handles and node ids, never the episodes' words.

use serde_json::{json, Value};
use theseus_protocol::LedgerKind;
use theseus_protocol::NarrativePart::Session;

use super::{Fact, Say};

/// One thread an import applied.
pub struct ThreadImported<'a> {
    pub slug: &'a str,
    pub from: &'a str,
    pub profile: &'a str,
    pub model: &'a str,
    pub cost_usd: f64,
    pub input_tokens: u64,
    pub output_tokens: u64,
    /// The card's lines, and the lines the answer made that were dropped.
    pub lines: u32,
    pub dropped: u32,
    /// The citations: `{handle, node, session}` each.
    pub cited: &'a [Value],
    /// Why the thread has no card, if it has none.
    pub failed: Option<&'a str>,
}

impl Fact for ThreadImported<'_> {
    const KIND: Option<LedgerKind> = Some(LedgerKind::ThreadImported);

    fn row(&self) -> Value {
        json!({"slug": self.slug, "from": self.from, "profile": self.profile,
               "model": self.model, "cost_usd": self.cost_usd,
               "input_tokens": self.input_tokens, "output_tokens": self.output_tokens,
               "lines": self.lines, "dropped": self.dropped,
               "cited": self.cited, "failed": self.failed})
    }

    fn narrate(&self, say: &mut Say<'_>) {
        let line = match self.failed {
            Some(why) => format!(
                "Thread: the thread {} was made from {} without a card ({why}).",
                self.slug, self.from
            ),
            None => format!(
                "Thread: the thread {} was made from {} with a card of {} lines citing {}, for {}.",
                self.slug,
                self.from,
                self.lines,
                crate::narrative::count(self.cited.len() as u64, "episode", "episodes"),
                crate::narrative::dollars(theseus_judge::price::usd_to_micros(self.cost_usd))
            ),
        };
        say.line(Session, line);
    }
}
