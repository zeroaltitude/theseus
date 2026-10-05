//! Consolidation's facts (M6 step 31b): a synthesis proposed (its cited
//! text, sources, profile, model, and cost), checked (the deterministic
//! checks and `citation.v1`'s verdict), and scored in shadow (would recall
//! have selected it). Each row is keyed by the synthesis's id and scoped
//! `memory`, so its day's spend and the clusters synthesized before read
//! back from the record. No notification: the rows and the lines are how a
//! surface sees them.

use serde_json::{json, Value};
use theseus_protocol::{LedgerKind, NarrativePart::Context};

use super::{Fact, Say};

/// The scope consolidation's rows are kept under.
pub const SCOPE: &str = "memory";

/// A cluster's synthesis, as the profile wrote it.
pub struct SynthesisProposed<'a> {
    pub synthesis_id: &'a str,
    pub cluster: &'a str,
    pub sources: &'a [String],
    pub turns: u64,
    pub text: &'a str,
    pub profile: &'a str,
    pub model: &'a str,
    pub cost_usd: f64,
    pub trigger: &'a str,
}

impl Fact for SynthesisProposed<'_> {
    const KIND: Option<LedgerKind> = Some(LedgerKind::SynthesisProposed);

    fn row(&self) -> Value {
        json!({"synthesis_id": self.synthesis_id, "cluster": self.cluster,
               "sources": self.sources, "turns": self.turns, "text": self.text,
               "profile": self.profile, "model": self.model,
               "cost_usd": self.cost_usd, "trigger": self.trigger})
    }

    fn narrate(&self, say: &mut Say<'_>) {
        say.line(
            Context,
            format!(
                "Consolidation: {} wrote a synthesis of {} notes ({}), for {}.",
                self.profile,
                self.sources.len(),
                self.synthesis_id,
                crate::narrative::dollars(theseus_judge::price::usd_to_micros(self.cost_usd))
            ),
        );
    }
}

/// A synthesis's check: `supported`, `unchecked`, or `rejected`, and why.
pub struct SynthesisChecked<'a> {
    pub synthesis_id: &'a str,
    pub cluster: &'a str,
    pub verdict: &'a str,
    /// The least of Jev's probabilities, when it answered every pair.
    pub least: Option<f64>,
    pub judgment: Option<&'a str>,
    /// The pack's mode when it answered.
    pub mode: Option<&'a str>,
    pub why: Option<&'a str>,
    /// The pairs under 0.5 (`s<sentence>:<source>`).
    pub unsupported: &'a [String],
    /// The leading heading set aside before the checks, as written: the
    /// node's text is the answer without it.
    pub heading: Option<&'a str>,
}

impl Fact for SynthesisChecked<'_> {
    const KIND: Option<LedgerKind> = Some(LedgerKind::SynthesisChecked);

    fn row(&self) -> Value {
        json!({"synthesis_id": self.synthesis_id, "cluster": self.cluster,
               "verdict": self.verdict, "least": self.least, "judgment": self.judgment,
               "mode": self.mode, "why": self.why, "unsupported": self.unsupported,
               "heading": self.heading})
    }

    fn narrate(&self, say: &mut Say<'_>) {
        let why = self.why.map(|w| format!(": {w}")).unwrap_or_default();
        say.line(
            Context,
            format!(
                "Consolidation: {} is {}{why}.",
                self.synthesis_id, self.verdict
            ),
        );
    }
}

/// A synthesis's shadow score over the recent recalls that admitted two or
/// more of its sources: would the pack have selected it, and at what rank.
pub struct SynthesisScored<'a> {
    pub synthesis_id: &'a str,
    /// Each recall scored: its id, `would_select`, and `rank`.
    pub recalls: &'a [Value],
}

impl Fact for SynthesisScored<'_> {
    const KIND: Option<LedgerKind> = Some(LedgerKind::SynthesisScored);

    fn row(&self) -> Value {
        let selected = self
            .recalls
            .iter()
            .filter(|r| r["would_select"] == json!(true))
            .count();
        json!({"synthesis_id": self.synthesis_id, "recalls": self.recalls,
               "scored": self.recalls.len(), "would_select": selected,
               "basis": "best_admitted_source"})
    }

    fn narrate(&self, say: &mut Say<'_>) {
        let selected = self
            .recalls
            .iter()
            .filter(|r| r["would_select"] == json!(true))
            .count();
        say.line(
            Context,
            format!(
                "Consolidation: recall would have selected {} in {selected} of {} recent recalls.",
                self.synthesis_id,
                self.recalls.len()
            ),
        );
    }
}
