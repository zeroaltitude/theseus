//! `switch_cost` (theseus-h66v4.1): what a switch costs this message against
//! staying on the warm model, from the resident prefix and the catalog's
//! prices. The owner's three cases are here as arithmetic: a thank-you on a
//! 200K Sonnet session stays (a cheaper model is dearer for this message), a
//! fresh session has nothing to lose, and the band is plus or minus 20%.

use theseus_judge::builders::PricePeriod::{self, OffPeak, Peak};
use theseus_judge::builders::SwitchCost::{AboutTheSame, CheaperToStay, CheaperToSwitch};

use super::switch::classify;
use super::{switch_candidates, switch_cost, Catalog};

const SONNET: &str = "claude-sonnet-5-5";
const HAIKU: &str = "claude-haiku-5-5";
const OPUS: &str = "claude-opus-5-5";
const FABLE: &str = "claude-fable-5-1";
const FLASH: &str = "deepseek-flash";

/// The cost at peak, where the candidates are the four Anthropic models.
fn at(current: &str, tokens: u64) -> theseus_judge::builders::SwitchCost {
    at_in(current, tokens, Peak)
}

fn at_in(
    current: &str,
    tokens: u64,
    period: PricePeriod,
) -> theseus_judge::builders::SwitchCost {
    switch_cost(&Catalog::builtin(), Some(current), tokens, period)
}

#[test]
fn a_long_sonnet_session_is_cheaper_to_stay() {
    // 200K: Haiku's long tier is $0.50 an input million, Sonnet's cache read $0.20.
    assert_eq!(at(SONNET, 200_000), CheaperToStay);
    assert_eq!(at(OPUS, 200_000), CheaperToStay);
    assert_eq!(at(FABLE, 200_000), CheaperToStay);
}

#[test]
fn a_short_session_is_cheaper_to_switch() {
    // 40K: Haiku's input is $0.10 a million, a cache read on Sonnet or Opus $0.20.
    assert_eq!(at(SONNET, 40_000), CheaperToSwitch);
    assert_eq!(at(OPUS, 40_000), CheaperToSwitch);
    assert_eq!(at(FABLE, 40_000), CheaperToSwitch);
}

#[test]
fn haikus_long_tier_starts_above_100k_tokens() {
    assert_eq!(at(SONNET, 100_000), CheaperToSwitch);
    assert_eq!(at(SONNET, 100_001), CheaperToStay);
}

#[test]
fn nothing_is_cheaper_than_haiku_so_a_haiku_session_stays() {
    for n in [1_000, 40_000, 150_000, 400_000] {
        assert_eq!(at(HAIKU, n), CheaperToStay, "{n}");
    }
}

/// theseus-h66v4.8, the owner 2026-11-03 00:03 ("4. Yes, Yes, Yes": DeepSeek
/// Flash joins the candidates OFF-PEAK ONLY). Flash's off-peak input is $0.15 a
/// million, under the $0.20 of a cache read on Sonnet, Opus 5.5 or Fable 5.1
/// (less 20%: $0.16), so a long session on any of them reads `cheaper_to_switch`
/// off-peak, where Haiku's long tier ($0.50) no longer makes it `cheaper_to_stay`.
#[test]
fn deepseek_flash_counts_as_a_candidate_off_peak() {
    assert!(switch_candidates(OffPeak).any(|m| m == FLASH));
    // 200K: Haiku's long tier is $0.50, so before Flash these stayed.
    assert_eq!(at_in(SONNET, 200_000, OffPeak), CheaperToSwitch);
    assert_eq!(at_in(OPUS, 200_000, OffPeak), CheaperToSwitch);
    assert_eq!(at_in(FABLE, 200_000, OffPeak), CheaperToSwitch);
    // The same 120K and 180K sessions as route.v10's W12 and W11.
    assert_eq!(at_in(SONNET, 120_000, OffPeak), CheaperToSwitch);
    assert_eq!(at_in(SONNET, 180_000, OffPeak), CheaperToSwitch);
}

/// At peak Flash's input is $0.30, above every warm Anthropic cache read, so it
/// is no candidate and a long session stays, as the arithmetic of the four
/// Anthropic models says (the owner 2026-10-29 22:50: a thank-you on a big
/// session stays).
#[test]
fn deepseek_flash_is_not_a_candidate_at_peak() {
    assert!(!switch_candidates(Peak).any(|m| m == FLASH));
    assert_eq!(switch_candidates(Peak).count(), 4);
    assert_eq!(at_in(SONNET, 200_000, Peak), CheaperToStay);
    assert_eq!(at_in(OPUS, 200_000, Peak), CheaperToStay);
    assert_eq!(at_in(FABLE, 200_000, Peak), CheaperToStay);
    assert_eq!(at_in(SONNET, 150_000, Peak), CheaperToStay);
}

/// Flash enters off-peak only where it is the cheapest: a short session still
/// weighs Haiku's $0.10, and a Haiku or Flash session has nothing cheaper.
#[test]
fn off_peak_flash_changes_nothing_where_it_is_not_the_cheapest() {
    assert_eq!(at_in(SONNET, 40_000, OffPeak), CheaperToSwitch);
    assert_eq!(at_in(SONNET, 100_000, OffPeak), CheaperToSwitch);
    for n in [1_000, 40_000, 150_000, 400_000] {
        assert_eq!(at_in(HAIKU, n, OffPeak), CheaperToStay, "{n}");
        assert_eq!(at_in(FLASH, n, OffPeak), CheaperToStay, "{n}");
    }
}

#[test]
fn a_fresh_or_unpriced_session_is_about_the_same() {
    let c = Catalog::builtin();
    assert_eq!(switch_cost(&c, None, 3_000, Peak), AboutTheSame);
    assert_eq!(
        switch_cost(&c, Some("no-such-model"), 3_000, OffPeak),
        AboutTheSame
    );
}

#[test]
fn the_band_is_plus_or_minus_twenty_percent_inclusive() {
    assert_eq!(classify(1.0, 1.2), AboutTheSame);
    assert_eq!(classify(1.0, 0.8), AboutTheSame);
    assert_eq!(classify(1.0, 1.0), AboutTheSame);
    assert_eq!(classify(1.0, 1.2001), CheaperToStay);
    assert_eq!(classify(1.0, 0.7999), CheaperToSwitch);
    // No stay price to compare with: no claim.
    assert_eq!(classify(0.0, 5.0), AboutTheSame);
}

/// theseus-h66v4.1: every case's `switch_cost` in the route.v10 test set is
/// what the daemon's own rule says for that case's state. The judge crate
/// cannot see the catalog, so the file is held to the function here.
#[test]
fn every_cases_switch_cost_is_what_the_daemons_rule_says() {
    let pack = theseus_judge::pack::by_name("route.v10").unwrap();
    let set =
        theseus_judge::route_cases::CaseSet::parse(theseus_judge::route_cases::V10, &pack).unwrap();
    let catalog = Catalog::builtin();
    for c in &set.cases {
        let s = &c.state;
        let current = Some(s.current_model.as_str()).filter(|m| !m.is_empty());
        let period = match s.price_period.as_str() {
            "off_peak" => OffPeak,
            _ => Peak,
        };
        let got = switch_cost(&catalog, current, s.est_tokens, period);
        assert_eq!(got.as_str(), s.switch_cost, "{}: {s:?}", c.id);
    }
}
