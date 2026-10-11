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

/// The nightly sweep's run (theseus-j8qb): the private sessions with human
/// text in its window that no extraction read, how many it passed, and its
/// spend under the day's cap. Its extractions' rows carry their own cost (so
/// this row is no spend of the day's ceiling's).
pub struct PeopleSwept<'a> {
    /// `nightly` or `missed`.
    pub trigger: &'a str,
    /// Human text since this time (unix ms) was read.
    pub from_ms: u64,
    /// Sessions due, and those passed.
    pub due: u64,
    pub swept: u64,
    pub candidates: u64,
    pub judged: u64,
    pub spent_usd: f64,
    /// The day's cap, and what earlier runs that day spent.
    pub cap_usd: f64,
    pub spent_before_usd: f64,
    /// Why it stopped before the last session (the cap, the daemon's stop).
    pub stopped: Option<&'a str>,
}

impl Fact for PeopleSwept<'_> {
    const KIND: Option<LedgerKind> = Some(LedgerKind::PeopleSwept);

    fn row(&self) -> Value {
        json!({"trigger": self.trigger, "from_ms": self.from_ms, "due": self.due,
               "swept": self.swept, "candidates": self.candidates, "judged": self.judged,
               "spent_usd": self.spent_usd, "cap_usd": self.cap_usd,
               "spent_before_usd": self.spent_before_usd, "stopped": self.stopped})
    }

    fn narrate(&self, say: &mut Say<'_>) {
        let mut line = format!(
            "People's sweep ({}): {} of {} due, {} found, {} judged by Jev, for {} of the day's {}.",
            self.trigger,
            crate::narrative::count(self.swept, "session", "sessions"),
            self.due,
            crate::narrative::count(self.candidates, "candidate", "candidates"),
            self.judged,
            crate::narrative::dollars(theseus_judge::price::usd_to_micros(self.spent_usd)),
            crate::narrative::dollars(theseus_judge::price::usd_to_micros(self.cap_usd)),
        );
        if let Some(why) = self.stopped {
            line.push_str(&format!(" Stopped: {why}."));
        }
        say.line(Context, line);
    }
}
