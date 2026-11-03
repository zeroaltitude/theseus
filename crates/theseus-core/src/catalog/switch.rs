//! What a switch costs this message (theseus-h66v4.1, the owner 2026-10-29
//! 22:50): the resident prefix at the cheapest other model's input price,
//! against the same prefix at the warm model's cache-read price. route.v10's
//! `model` instructions read the answer (`switch_cost`) as a fact the daemon
//! computed, so Jev never does the arithmetic and no `context` question is
//! asked.
//!
//! The candidates are route.v10's four Anthropic options. DeepSeek Flash is
//! left out on purpose: its block fits one narrow kind of work (a bounded
//! mechanical change), so its cheap input price says nothing about a thank-you
//! or a design question, and counting it would make every long Sonnet session
//! read `cheaper_to_switch` at off-peak. A prompt past a model's long tier pays
//! the tier's prices, on both sides.

use theseus_judge::builders::SwitchCost;

use super::Catalog;

/// The models a switch is weighed against.
pub const CANDIDATES: [&str; 4] = [
    "claude-haiku-5-5",
    "claude-sonnet-5-5",
    "claude-opus-5-5",
    "claude-fable-5-1",
];

/// `about_the_same` is within this share of the stay cost, either way.
const BAND: f64 = 0.20;

/// The cost of a switch against the cost of staying, as a word.
pub(super) fn classify(stay: f64, switch: f64) -> SwitchCost {
    if stay <= 0.0 {
        // Nothing to compare with: no claim either way.
        SwitchCost::AboutTheSame
    } else if switch > stay * (1.0 + BAND) {
        SwitchCost::CheaperToStay
    } else if switch < stay * (1.0 - BAND) {
        SwitchCost::CheaperToSwitch
    } else {
        SwitchCost::AboutTheSame
    }
}

/// What switching costs `prefix_tokens` of resident prefix, from the `current`
/// warm model: `about_the_same` where none is warm, or the catalog does not
/// price it, or no other candidate is priced.
pub fn switch_cost(catalog: &Catalog, current: Option<&str>, prefix_tokens: u64) -> SwitchCost {
    let Some((current, warm)) = current.and_then(|m| Some((m, catalog.get(m)?))) else {
        return SwitchCost::AboutTheSame;
    };
    // Per million tokens: the same prefix, so the million cancels out of the ratio.
    let stay = warm.cache_read_price_at(prefix_tokens);
    let cheapest = CANDIDATES
        .iter()
        .filter(|m| **m != current)
        .filter_map(|m| catalog.get(m))
        .map(|e| e.input_price_at(prefix_tokens))
        .min_by(f64::total_cmp);
    match cheapest {
        Some(switch) => classify(stay, switch),
        None => SwitchCost::AboutTheSame,
    }
}
