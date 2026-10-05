//! Consolidation's citation check (M6 step 31b; design `m6-memory.md`
//! §2.7): `citation.v1` asks Jev, of each sentence of a synthesis and each
//! source it cites, whether the source supports the sentence: one Noul a
//! pair. Off every turn, at a point of its own (`consolidation`), awaited by
//! the run that asks it; its judgment is the sink's `judge.call` row, scoped
//! `judge:citation`, and its spend the judge's day budget's, as every
//! judgment's. A new pack: `WIRED` in shadow. Its verdict qualifies a
//! synthesis for the `+synthesis` arm alone (see `consolidate`).

use std::collections::BTreeMap;

use serde_json::{json, Value};
use theseus_judge::builders::{pair_key, CitationInput};
use theseus_judge::{Ask, DecisionPoint, Input, Judge, Judgment, Outcome, Urgency};

use super::{spend, JudgeService, Prepared, ScrubWith};
use crate::config::PackMode;

/// Consolidation's citation check (M6 31b).
pub const CITATION_PACK: &str = "citation.v1";

/// A pair under this rejects the synthesis (§2.7).
pub const SUPPORTED: f64 = 0.5;

/// What Jev said of a synthesis's citations.
#[derive(Debug, Clone, PartialEq)]
pub struct Checked {
    pub judgment: String,
    /// The pack's mode when it answered.
    pub mode: String,
    /// Jev's probability per pair (`s<sentence>:<source>`).
    pub pairs: BTreeMap<String, f64>,
    /// Pairs asked that came back with no probability.
    pub unanswered: Vec<String>,
}

impl Checked {
    /// The least probability, when every pair asked was answered.
    pub fn least(&self) -> Option<f64> {
        if !self.unanswered.is_empty() || self.pairs.is_empty() {
            return None;
        }
        self.pairs.values().copied().reduce(f64::min)
    }

    /// The pairs under [`SUPPORTED`].
    pub fn unsupported(&self) -> Vec<&str> {
        self.pairs
            .iter()
            .filter(|(_, p)| **p < SUPPORTED)
            .map(|(k, _)| k.as_str())
            .collect()
    }
}

impl JudgeService {
    /// `citation.v1` on one synthesis, awaited: Jev's answer per pair, or
    /// why there is none (the judge or the pack off, the day's budget, no
    /// client, a failed call).
    pub async fn check_citations(
        &self,
        session: &str,
        synthesis: &str,
        input: CitationInput,
    ) -> Result<Checked, String> {
        let given = self.mode_for(CITATION_PACK, session);
        if given.mode == PackMode::Off {
            return Err("the judge or citation.v1 is off".into());
        }
        let Some(pack) = theseus_judge::pack::by_name(CITATION_PACK) else {
            return Err("this build has no citation.v1".into());
        };
        let want: Vec<String> = input
            .pairs()
            .into_iter()
            .take(theseus_judge::builders::CITATION_PAIRS)
            .map(|(s, c)| pair_key(s, c))
            .collect();
        if input.pairs().len() > want.len() {
            return Err(format!(
                "{} cited pairs, past the {} one check asks",
                input.pairs().len(),
                want.len()
            ));
        }
        let today = spend::local_day(theseus_protocol::now_unix_ms());
        let context = json!({"session": session, "synthesis": synthesis,
            "purpose": "consolidation", "baseline": "none", "on_path_ms": 0});
        let me = self.me.clone();
        let day = today.clone();
        let prepared = tokio::task::spawn_blocking(move || {
            me.upgrade()
                .ok_or_else(|| "the judge stopped".to_string())
                .and_then(|svc| svc.prepare_citation(pack, input, context, &day))
        })
        .await
        .map_err(|e| format!("the check's state was not built: {e}"))??;
        let Prepared { built, ask, need } = prepared;
        let mode = ask.mode;
        let judgments = built
            .judge
            .judge(DecisionPoint {
                asks: vec![ask],
                urgency: Urgency::Shadow,
            })
            .await;
        for j in &judgments {
            let (called, failed, unknown) = match &j.outcome {
                Outcome::Answered => (true, false, false),
                Outcome::Failed { usage_unknown, .. } => (true, true, *usage_unknown),
                Outcome::Skipped { .. } => (false, false, false),
            };
            let spent = j.cost_micros.unwrap_or(if unknown { need } else { 0 });
            self.budget.settle(&today, need, spent, called, failed);
        }
        let j = judgments
            .into_iter()
            .next()
            .ok_or_else(|| "Jev returned no judgment".to_string())?;
        checked(&j, &want, mode_word(mode))
    }

    /// The blocking half: the state, its blob, and the reservation.
    fn prepare_citation(
        &self,
        pack: std::sync::Arc<theseus_judge::Pack>,
        input: CitationInput,
        mut context: Value,
        today: &str,
    ) -> Result<Prepared, String> {
        let scrub = ScrubWith(self.scrubber.clone());
        let state = theseus_judge::prepare(&pack, &Input::Citation(input), &scrub)
            .map_err(|e| e.to_string())?;
        let blob = self
            .store
            .blobs()
            .put(state.state.json.as_bytes())
            .map_err(|e| format!("the check's state was not written: {e}"))?;
        let built = self
            .built()
            .map_err(|e| format!("the Jev client was not built: {e:#}"))?;
        context["blob"] = json!(blob);
        let mode = self.ask_mode(&pack.name(), &mut context);
        let ask = Ask::new(pack, &state, mode, context);
        let need = built
            .judge
            .inner()
            .reserve_micros(std::slice::from_ref(&ask))
            .unwrap_or(0);
        if !self.reserve(today, need) {
            return Err("the judge's day budget is spent".into());
        }
        Ok(Prepared { built, ask, need })
    }
}

fn mode_word(m: theseus_judge::Mode) -> &'static str {
    match m {
        theseus_judge::Mode::Live => "live",
        _ => "shadow",
    }
}

/// A judgment's answers per pair, or why it gave none.
fn checked(j: &Judgment, want: &[String], mode: &str) -> Result<Checked, String> {
    if let Some(why) = super::rerank::fallback(j) {
        return Err(format!("Jev did not answer: {why}"));
    }
    let pairs = super::rerank::probabilities(j);
    let unanswered = want
        .iter()
        .filter(|k| !pairs.contains_key(*k))
        .cloned()
        .collect();
    Ok(Checked {
        judgment: j.id.clone(),
        mode: mode.into(),
        pairs,
        unanswered,
    })
}
