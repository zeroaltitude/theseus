//! `switch_cost` (theseus-h66v4.1): what a switch costs this message against
//! staying on the warm model, from the resident prefix and the catalog's
//! prices. The owner's three cases are here as arithmetic: a thank-you on a
//! 200K Sonnet session stays (a cheaper model is dearer for this message), a
//! fresh session has nothing to lose, and the band is plus or minus 20%.

use theseus_judge::builders::SwitchCost::{AboutTheSame, CheaperToStay, CheaperToSwitch};

use super::switch::classify;
use super::{switch_cost, Catalog};

const SONNET: &str = "claude-sonnet-5-5";
const HAIKU: &str = "claude-haiku-5-5";
const OPUS: &str = "claude-opus-5-5";
const FABLE: &str = "claude-fable-5-1";

fn at(current: &str, tokens: u64) -> theseus_judge::builders::SwitchCost {
    switch_cost(&Catalog::builtin(), Some(current), tokens)
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

#[test]
fn deepseek_flash_is_not_a_candidate() {
    // Flash's off-peak input ($0.15) is under Sonnet's cache read ($0.20), yet a
    // 200K Sonnet session stays: a thank-you is no work Flash is for.
    assert_eq!(at(SONNET, 200_000), CheaperToStay);
    assert!(!super::SWITCH_CANDIDATES.contains(&"deepseek-flash"));
}

#[test]
fn a_fresh_or_unpriced_session_is_about_the_same() {
    let c = Catalog::builtin();
    assert_eq!(switch_cost(&c, None, 3_000), AboutTheSame);
    assert_eq!(switch_cost(&c, Some("no-such-model"), 3_000), AboutTheSame);
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
        let got = switch_cost(&catalog, current, s.est_tokens);
        assert_eq!(got.as_str(), s.switch_cost, "{}: {s:?}", c.id);
    }
}
