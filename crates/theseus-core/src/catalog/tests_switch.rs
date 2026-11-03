//! `switch_cost` (theseus-h66v4.1, theseus-h66v4.8): what a switch costs this
//! message against staying on the warm model, and which model the cheaper one
//! is, from the resident prefix and the catalog's prices at the hour. The
//! owner's three cases are here as arithmetic: a thank-you on a 200K Sonnet
//! session is told that Flash is the cheaper model (it is, and a thank-you is
//! not its work), a fresh session has nothing to lose, and the band is plus or
//! minus 20%.

use theseus_judge::builders::PricePeriod::{OffPeak, Peak};
use theseus_judge::builders::RestrictedModels::{Allowed, NotAllowed};
use theseus_judge::builders::SwitchCost::{AboutTheSame, CheaperToStay, CheaperToSwitch};
use theseus_judge::builders::{PricePeriod, RestrictedModels, SwitchCost};

use super::switch::{classify, Switch};
use super::{switch_cost, Catalog};

const SONNET: &str = "claude-sonnet-5-5";
const HAIKU: &str = "claude-haiku-5-5";
const OPUS: &str = "claude-opus-5-5";
const FABLE: &str = "claude-fable-5-1";
const FLASH: &str = "deepseek-flash";

/// The word and the name, at a period and under the data rule.
fn full(current: &str, tokens: u64, period: PricePeriod, r: RestrictedModels) -> Switch {
    switch_cost(&Catalog::builtin(), Some(current), tokens, period, r)
}

/// The word, at a period, with restricted models allowed.
fn word(current: &str, tokens: u64, period: PricePeriod) -> SwitchCost {
    full(current, tokens, period, Allowed).cost
}

/// The word off-peak (the cases file's default, and the commoner hour).
fn at(current: &str, tokens: u64) -> SwitchCost {
    word(current, tokens, OffPeak)
}

/// The name the state would carry, or "-" for none.
fn name(current: &str, tokens: u64, period: PricePeriod) -> String {
    full(current, tokens, period, Allowed)
        .cheaper_model
        .map_or_else(|| "-".to_string(), String::from)
}

#[test]
fn a_long_session_off_peak_is_cheaper_to_switch_and_the_cheaper_one_is_flash() {
    // 200K off-peak: Flash's input is $0.15 a million against Sonnet's $0.20
    // cache read, and Haiku's long tier is $0.50. The word was
    // `cheaper_to_stay` while Flash was left out of the candidates, which put
    // Flash's own block out of reach through the price (theseus-h66v4.8).
    for m in [SONNET, OPUS] {
        assert_eq!(at(m, 200_000), CheaperToSwitch, "{m}");
        assert_eq!(
            name(m, 200_000, OffPeak),
            "deepseek_flash (off-peak)",
            "{m}"
        );
    }
    // Fable's cache read is $0.25, so its gap is wider still.
    assert_eq!(at(FABLE, 200_000), CheaperToSwitch);
}

#[test]
fn a_long_session_at_peak_is_cheaper_to_stay_and_names_nothing() {
    // At peak Flash's input is $0.30 and Haiku's long tier $0.50, both above
    // the $0.20 cache read of staying: nothing is cheaper, nothing is named.
    // W13 is this case, and it is why W13's label did not move.
    for m in [SONNET, OPUS] {
        assert_eq!(word(m, 150_000, Peak), CheaperToStay, "{m}");
        assert_eq!(name(m, 150_000, Peak), "-", "{m}");
    }
}

#[test]
fn a_short_session_is_cheaper_to_switch_and_the_cheaper_one_is_haiku() {
    // 40K: Haiku's input is $0.10 a million, under Flash's $0.15 off-peak, and
    // a cache read on Sonnet or Opus is $0.20.
    for m in [SONNET, OPUS, FABLE] {
        assert_eq!(at(m, 40_000), CheaperToSwitch, "{m}");
        assert_eq!(name(m, 40_000, OffPeak), "haiku", "{m}");
    }
    // At peak Haiku is cheapest too, and then the name carries no off-peak mark.
    assert_eq!(name(SONNET, 40_000, Peak), "haiku");
}

#[test]
fn haikus_long_tier_moves_the_cheaper_model_from_haiku_to_flash() {
    assert_eq!(at(SONNET, 100_000), CheaperToSwitch);
    assert_eq!(name(SONNET, 100_000, OffPeak), "haiku");
    // One token past the tier Haiku's input is $0.50 and Flash is the cheaper
    // one. The word does not change; what Jev is told to weigh does.
    assert_eq!(at(SONNET, 100_001), CheaperToSwitch);
    assert_eq!(name(SONNET, 100_001, OffPeak), "deepseek_flash (off-peak)");
}

#[test]
fn nothing_is_cheaper_than_a_warm_haiku_or_a_warm_flash() {
    // Haiku's cache read is $0.01 under its tier and $0.05 above it, Flash's
    // $0.003 off-peak: no candidate's input price comes near either.
    for n in [1_000, 40_000, 150_000, 400_000] {
        assert_eq!(at(HAIKU, n), CheaperToStay, "haiku {n}");
        assert_eq!(name(HAIKU, n, OffPeak), "-", "haiku {n}");
        assert_eq!(at(FLASH, n), CheaperToStay, "flash {n}");
    }
}

/// The data rule in the state is read here too (theseus-h66v4.8): a state that
/// keeps restricted-origin models off must not name one as the cheaper choice,
/// or it forbids and recommends the same model in one breath. W14 is this case,
/// and it is why W14's state did not move. The floor that refuses the pick is
/// `routing::model`'s and is untouched.
#[test]
fn a_restricted_session_is_never_told_flash_is_cheaper() {
    let s = full(SONNET, 180_000, OffPeak, NotAllowed);
    // Without Flash the cheapest left at 180K is Haiku's long tier, $0.50,
    // against staying at $0.20: there is nothing cheaper to name.
    assert_eq!(s.cost, CheaperToStay);
    assert_eq!(s.cheaper_model, None);
    // The same message with the labels allowing it is Flash's.
    let s = full(SONNET, 180_000, OffPeak, Allowed);
    assert_eq!(s.cost, CheaperToSwitch);
    assert_eq!(
        s.cheaper_model.as_deref(),
        Some("deepseek_flash (off-peak)")
    );
    // Under 100K Haiku is cheapest either way, so the rule changes nothing.
    for r in [Allowed, NotAllowed] {
        let s = full(SONNET, 40_000, OffPeak, r);
        assert_eq!(s.cost, CheaperToSwitch, "{r:?}");
        assert_eq!(s.cheaper_model.as_deref(), Some("haiku"), "{r:?}");
    }
}

#[test]
fn a_fresh_or_unpriced_session_is_about_the_same_and_names_nothing() {
    let c = Catalog::builtin();
    for m in [None, Some("no-such-model")] {
        let s = switch_cost(&c, m, 3_000, OffPeak, Allowed);
        assert_eq!(s.cost, AboutTheSame, "{m:?}");
        assert_eq!(s.cheaper_model, None, "{m:?}");
    }
}

/// A name in the state always means money: the daemon names no model where
/// switching saves nothing, so Jev never reads "cheaper" of a dearer model.
#[test]
fn a_name_is_carried_only_with_cheaper_to_switch() {
    let c = Catalog::builtin();
    for m in [SONNET, HAIKU, OPUS, FABLE, FLASH] {
        for n in [1_000, 40_000, 100_001, 200_000, 400_000] {
            for p in [OffPeak, Peak] {
                for r in [Allowed, NotAllowed] {
                    let s = switch_cost(&c, Some(m), n, p, r);
                    assert_eq!(
                        s.cheaper_model.is_some(),
                        s.cost == CheaperToSwitch,
                        "{m} {n} {p:?} {r:?}: {s:?}"
                    );
                }
            }
        }
    }
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

/// The price rule's candidates and route.v10's `model` options are one list: an
/// option this rule cannot see is one Jev is never told is cheaper, and a
/// candidate the pack does not offer is a name Jev cannot act on.
#[test]
fn the_candidates_are_exactly_route_v10s_model_options() {
    assert_eq!(super::SWITCH_CANDIDATES, crate::routing::model::OPTIONS);
}

/// theseus-h66v4.1: every case's `switch_cost` in the route.v10 test set is
/// what the daemon's own rule says for that case's state, and since
/// theseus-h66v4.8 its `cheaper_model` too. The judge crate cannot see the
/// catalog, so the file is held to the function here.
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
            "peak" => Peak,
            _ => OffPeak,
        };
        let restricted = match s.restricted_models.as_str() {
            "allowed" => Allowed,
            _ => NotAllowed,
        };
        let got = switch_cost(&catalog, current, s.est_tokens, period, restricted);
        assert_eq!(got.cost.as_str(), s.switch_cost, "{}: {s:?}", c.id);
        assert_eq!(
            got.cheaper_model.as_deref().unwrap_or(""),
            s.cheaper_model,
            "{}: {s:?}",
            c.id
        );
    }
}
