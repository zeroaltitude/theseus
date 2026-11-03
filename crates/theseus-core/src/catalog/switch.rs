//! What a switch costs this message (theseus-h66v4.1, the owner 2026-10-29
//! 22:50): the resident prefix at the cheapest other model's input price,
//! against the same prefix at the warm model's cache-read price. route.v10's
//! `model` instructions read the answer (`switch_cost`) as a fact the daemon
//! computed, so Jev never does the arithmetic and no `context` question is
//! asked.
//!
//! The candidates are route.v10's four Anthropic options, and, in the
//! provider's off-peak hours (the state's `price_period`), DeepSeek Flash at
//! its off-peak prices. The owner decided it on 2026-11-03 00:03 ("4. Yes, Yes,
//! Yes", on the question: keep Flash out, count it always, or count it only
//! off-peak; he took off-peak only), theseus-h66v4.8. Until then Flash was left
//! out on purpose: its block fits one narrow kind of work (a bounded mechanical
//! change), so its cheap input price says nothing about a thank-you or a design
//! question, and counting it makes a long session on Sonnet, Opus 5.5 or Fable
//! 5.1 read `cheaper_to_switch` at off-peak. That is what the owner accepted:
//! off-peak, Flash's input ($0.15 a million) is under their cache read ($0.20 to
//! $0.25) and under Haiku's long tier ($0.50), so for a long prefix the cheapest
//! other model is Flash, and `cheaper_to_switch` is true there. At peak its
//! input ($0.30) is above every warm cache read and it stays out: nothing
//! moves. A prompt past a model's long tier pays the tier's prices, on both
//! sides.

use theseus_judge::builders::{PricePeriod, SwitchCost};

use super::Catalog;

/// The models a switch is weighed against in every period: route.v10's four
/// Anthropic options.
pub const CANDIDATES: [&str; 4] = [
    "claude-haiku-5-5",
    "claude-sonnet-5-5",
    "claude-opus-5-5",
    "claude-fable-5-1",
];

/// The one model counted only in the provider's off-peak hours (theseus-h66v4.8,
/// the owner 2026-11-03 00:03).
pub const OFF_PEAK_CANDIDATES: [&str; 1] = ["deepseek-flash"];

/// The models a switch is weighed against in `period`: the four always, and,
/// off-peak, DeepSeek Flash.
pub fn candidates(period: PricePeriod) -> impl Iterator<Item = &'static str> {
    let extra: &[&str] = match period {
        PricePeriod::OffPeak => &OFF_PEAK_CANDIDATES,
        PricePeriod::Peak => &[],
    };
    CANDIDATES.into_iter().chain(extra.iter().copied())
}

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
pub fn switch_cost(
    catalog: &Catalog,
    current: Option<&str>,
    prefix_tokens: u64,
    period: PricePeriod,
) -> SwitchCost {
    let Some((current, warm)) = current.and_then(|m| Some((m, catalog.get(m)?))) else {
        return SwitchCost::AboutTheSame;
    };
    // Per million tokens: the same prefix, so the million cancels out of the ratio.
    let stay = warm.cache_read_price_in(prefix_tokens, period);
    let cheapest = candidates(period)
        .filter(|m| *m != current)
        .filter_map(|m| catalog.get(m))
        .map(|e| e.input_price_in(prefix_tokens, period))
        .min_by(f64::total_cmp);
    match cheapest {
        Some(switch) => classify(stay, switch),
        None => SwitchCost::AboutTheSame,
    }
}
