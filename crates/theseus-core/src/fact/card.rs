//! The owner card's facts (theseus-slvl.6): a row for each model call the
//! job made (what it cost, what it proposed), and a row for each answer to a
//! proposal, with the episodes the proposal rests on. The cost is the day's
//! model spend (`day_ceiling::SPEND_KINDS`). A row holds ids and sizes, never
//! the episodes' words or the proposal's text, which the proposal's record
//! holds.

use serde_json::{json, Value};
use theseus_protocol::LedgerKind;
use theseus_protocol::NarrativePart::Session;

use super::{Fact, Say};

/// One model call of the job (`card.proposed`).
pub struct CardProposed<'a> {
    pub run: &'a str,
    pub profile: &'a str,
    pub model: &'a str,
    pub cost_usd: f64,
    pub input_tokens: u64,
    pub output_tokens: u64,
    /// Clusters sent, and statements the answer made that were dropped.
    pub clusters: u32,
    pub dropped: u32,
    /// The proposals this call made.
    pub proposals: &'a [String],
    pub failed: Option<&'a str>,
}

impl Fact for CardProposed<'_> {
    const KIND: Option<LedgerKind> = Some(LedgerKind::CardProposed);

    fn row(&self) -> Value {
        json!({"run": self.run, "profile": self.profile, "model": self.model,
               "cost_usd": self.cost_usd, "input_tokens": self.input_tokens,
               "output_tokens": self.output_tokens, "clusters": self.clusters,
               "dropped": self.dropped, "proposals": self.proposals, "failed": self.failed})
    }

    fn narrate(&self, say: &mut Say<'_>) {
        let line = match self.failed {
            Some(why) => format!(
                "Card: a call over {} clusters proposed nothing ({why}).",
                self.clusters
            ),
            None => format!(
                "Card: {} clusters read, {} proposed, for {}.",
                self.clusters,
                self.proposals.len(),
                crate::narrative::dollars(theseus_judge::price::usd_to_micros(self.cost_usd))
            ),
        };
        say.line(Session, line);
    }
}

/// A proposal was answered (`card.answered`): kept, edited, declined, or
/// expired to "Not this".
pub struct CardAnswered<'a> {
    pub id: &'a str,
    /// `keep`, `edit`, `decline` or `expire`.
    pub option: &'a str,
    pub by: &'a str,
    pub via: &'a str,
    /// The episodes the proposal rests on.
    pub episodes: &'a [String],
    pub standing_id: Option<&'a str>,
}

impl Fact for CardAnswered<'_> {
    const KIND: Option<LedgerKind> = Some(LedgerKind::CardAnswered);

    fn row(&self) -> Value {
        json!({"id": self.id, "option": self.option, "by": self.by, "via": self.via,
               "episodes": self.episodes, "standing": self.standing_id})
    }

    fn narrate(&self, say: &mut Say<'_>) {
        say.line(
            Session,
            format!(
                "Card: {} answered {} with {}.",
                self.by, self.id, self.option
            ),
        );
    }
}

/// One call of the exchange-end observer (`card.observed`, theseus-6f8t0): what
/// it cost and what it found. A row holds ids and counts, never the owner's
/// words, which the `card.obs.` records hold. Its cost is the day's model
/// spend (`day_ceiling::SPEND_KINDS`). The observer (a later row) writes it.
pub struct CardObserved<'a> {
    pub session: &'a str,
    pub model: &'a str,
    pub cost_usd: f64,
    pub input_tokens: u64,
    pub output_tokens: u64,
    /// Observations the model returned, and those kept.
    pub returned: u32,
    pub kept: u32,
    /// Those kept that restated a live standing record or an earlier
    /// observation.
    pub restated: u32,
}

impl Fact for CardObserved<'_> {
    const KIND: Option<LedgerKind> = Some(LedgerKind::CardObserved);

    fn row(&self) -> Value {
        json!({"session": self.session, "model": self.model, "cost_usd": self.cost_usd,
               "input_tokens": self.input_tokens, "output_tokens": self.output_tokens,
               "returned": self.returned, "kept": self.kept, "restated": self.restated})
    }

    fn narrate(&self, say: &mut Say<'_>) {
        say.line(
            Session,
            format!(
                "Card: an exchange read, {} found, {} kept, for {}.",
                self.returned,
                self.kept,
                crate::narrative::dollars(theseus_judge::price::usd_to_micros(self.cost_usd))
            ),
        );
    }
}
