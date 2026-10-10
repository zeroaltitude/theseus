//! People proposed from text (theseus-wy7y): one extraction of a session's
//! candidate people by a generative profile, its cost, tokens and model, and
//! what became of its candidates. Scoped `judge:people` beside `people.v1`'s
//! judgments, which the sink writes as every judgment's; its cost is the
//! day's model spend (`day_ceiling::SPEND_KINDS`).

use serde_json::{json, Value};
use theseus_protocol::{LedgerKind, NarrativePart::Context};

use super::{Fact, Say};

/// One extraction.
pub struct PeopleExtracted<'a> {
    pub id: &'a str,
    /// `live` (an exchange's end) or `backfill:<tag>`.
    pub purpose: &'a str,
    pub profile: &'a str,
    pub model: &'a str,
    pub cost_usd: f64,
    pub input_tokens: u64,
    pub output_tokens: u64,
    /// The candidates it returned, and those excluded before any judgment.
    pub candidates: &'a [String],
    pub excluded: &'a [String],
    /// Jev's judgments asked, one a candidate kept.
    pub judgments: &'a [String],
    /// Why it returned nothing usable, if it failed.
    pub failed: Option<&'a str>,
}

impl Fact for PeopleExtracted<'_> {
    const KIND: Option<LedgerKind> = Some(LedgerKind::PeopleExtracted);

    fn row(&self) -> Value {
        json!({"id": self.id, "purpose": self.purpose, "profile": self.profile,
               "model": self.model, "cost_usd": self.cost_usd,
               "input_tokens": self.input_tokens, "output_tokens": self.output_tokens,
               "candidates": self.candidates, "excluded": self.excluded,
               "judgments": self.judgments, "failed": self.failed})
    }

    fn narrate(&self, say: &mut Say<'_>) {
        let line = match self.failed {
            Some(why) => format!(
                "People: {}'s extraction failed ({why}), for {}.",
                self.profile,
                crate::narrative::dollars(theseus_judge::price::usd_to_micros(self.cost_usd))
            ),
            None => format!(
                "People: {} found {} ({} excluded), {} judged by Jev, for {}.",
                self.profile,
                crate::narrative::count(self.candidates.len() as u64, "candidate", "candidates"),
                self.excluded.len(),
                self.judgments.len(),
                crate::narrative::dollars(theseus_judge::price::usd_to_micros(self.cost_usd))
            ),
        };
        say.line(Context, line);
    }
}
