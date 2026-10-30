//! DeepSeek's peak hours and what the ledger charges in them (theseus-9f72b,
//! design §1.3), and each model's own default effort (§1.4 step 5).

use theseus_judge::builders::PricePeriod;
use theseus_judge::builders::PricePeriod::{OffPeak, Peak};
use theseus_protocol::time::days_from_civil;
use theseus_protocol::Usage;

use super::*;

/// A UTC instant, in Unix milliseconds.
fn utc(y: i64, m: u32, d: u32, h: u32, mi: u32) -> u64 {
    ((days_from_civil(y, m, d) * 24 + i64::from(h)) * 60 + i64::from(mi)) as u64 * 60_000
}

/// A Phoenix instant (UTC-7, no daylight saving).
fn phx(y: i64, m: u32, d: u32, h: u32, mi: u32) -> u64 {
    utc(y, m, d, h, mi) + 7 * 3_600_000
}

/// 2026-10-26 is a Monday.
#[test]
fn peak_is_0100_to_0400_and_0600_to_1000_utc_on_weekdays_with_both_edges_pinned() {
    let mon = |h, mi| price_period_at(utc(2026, 10, 26, h, mi));
    let cases: [(u32, u32, PricePeriod); 12] = [
        (0, 59, OffPeak),
        (1, 0, Peak),
        (3, 59, Peak),
        (4, 0, OffPeak),
        (5, 59, OffPeak),
        (6, 0, Peak),
        (9, 59, Peak),
        (10, 0, OffPeak),
        (12, 0, OffPeak),
        (23, 59, OffPeak),
        (0, 0, OffPeak),
        (2, 30, Peak),
    ];
    for (h, mi, want) in cases {
        assert_eq!(mon(h, mi), want, "Monday {h:02}:{mi:02} UTC");
    }
}

#[test]
fn every_weekday_has_peak_hours_and_the_weekend_has_none() {
    // 26 Oct 2026 Monday through 1 Nov Sunday, at 02:00 and 07:00 UTC.
    for (day, peak) in [
        (26, true),
        (27, true),
        (28, true),
        (29, true),
        (30, true),
        (31, false),
    ] {
        for h in [2, 7] {
            let want = if peak { Peak } else { OffPeak };
            assert_eq!(
                price_period_at(utc(2026, 10, day, h, 0)),
                want,
                "{day} {h}h"
            );
        }
    }
    for h in [2, 7] {
        assert_eq!(price_period_at(utc(2026, 11, 1, h, 0)), OffPeak, "Sunday");
    }
}

/// In Phoenix: 18:00 to 21:00 and 23:00 to 03:00, Sunday evening through
/// Friday 03:00.
#[test]
fn in_phoenix_peak_runs_sunday_evening_to_friday_0300() {
    // Sunday 1 Nov 2026 17:59 is off-peak; 18:00 is peak.
    assert_eq!(price_period_at(phx(2026, 11, 1, 17, 59)), OffPeak);
    assert_eq!(price_period_at(phx(2026, 11, 1, 18, 0)), Peak);
    assert_eq!(price_period_at(phx(2026, 11, 1, 20, 59)), Peak);
    assert_eq!(price_period_at(phx(2026, 11, 1, 21, 0)), OffPeak);
    assert_eq!(price_period_at(phx(2026, 11, 1, 22, 59)), OffPeak);
    assert_eq!(price_period_at(phx(2026, 11, 1, 23, 0)), Peak);
    // Monday 2 Nov, to 03:00.
    assert_eq!(price_period_at(phx(2026, 11, 2, 2, 59)), Peak);
    assert_eq!(price_period_at(phx(2026, 11, 2, 3, 0)), OffPeak);
    // Friday 30 Oct: 02:59 is the last peak minute; Friday evening is not.
    assert_eq!(price_period_at(phx(2026, 10, 30, 2, 59)), Peak);
    assert_eq!(price_period_at(phx(2026, 10, 30, 3, 0)), OffPeak);
    assert_eq!(price_period_at(phx(2026, 10, 30, 18, 0)), OffPeak);
    assert_eq!(price_period_at(phx(2026, 10, 30, 23, 30)), OffPeak);
    // Saturday small hours are Friday night's: off.
    assert_eq!(price_period_at(phx(2026, 10, 31, 1, 0)), OffPeak);
}

fn usage() -> Usage {
    Usage {
        input_tokens: 1_000_000,
        output_tokens: 1_000_000,
        cache_read_input_tokens: 1_000_000,
        ..Usage::default()
    }
}

fn flash() -> CatalogEntry {
    Catalog::builtin().get("deepseek-flash").unwrap().clone()
}

/// 02:00 Phoenix on Tuesday 27 Oct 2026 is 09:00 UTC.
#[test]
fn a_deepseek_call_at_0200_phoenix_on_a_tuesday_is_priced_peak() {
    let at = phx(2026, 10, 27, 2, 0);
    let e = flash();
    // 0.30 in + 1.20 out + 0.006 cache read, per million of each.
    assert_eq!(e.cost_micros_at(&usage(), at), 1_506_000);
    assert!((e.cost_usd_at(&usage(), at) - 1.506).abs() < 1e-9);
    assert_eq!(e.cost_micros_at(&usage(), at), e.cost_micros(&usage()));
}

/// 10:00 Phoenix on the same Tuesday is 17:00 UTC.
#[test]
fn a_deepseek_call_at_1000_phoenix_on_a_tuesday_is_priced_off_peak() {
    let at = phx(2026, 10, 27, 10, 0);
    let e = flash();
    assert_eq!(e.cost_micros_at(&usage(), at), 753_000);
    assert!((e.cost_usd_at(&usage(), at) - 0.753).abs() < 1e-9);
    let c = Catalog::builtin();
    let usd = c.cost_usd_at("deepseek-flash", &usage(), at).unwrap();
    assert!((usd - 0.753).abs() < 1e-9);
}

#[test]
fn off_peak_is_half_of_each_of_input_output_and_cache_read() {
    for id in ["deepseek-flash", "deepseek-v4-pro"] {
        let e = Catalog::builtin().get(id).unwrap().clone();
        let o = e
            .off_peak
            .unwrap_or_else(|| panic!("{id} has an off-peak row"));
        assert!(
            (o.input_per_mtok - e.input_per_mtok / 2.0).abs() < 1e-12,
            "{id}"
        );
        assert!(
            (o.output_per_mtok - e.output_per_mtok / 2.0).abs() < 1e-12,
            "{id}"
        );
        assert!(
            (o.cache_read_per_mtok - e.cache_read_per_mtok / 2.0).abs() < 1e-12,
            "{id}"
        );
    }
}

/// A cache write costs DeepSeek's input price, so off-peak it costs the
/// off-peak input price.
#[test]
fn cache_writes_follow_the_input_price_off_peak() {
    let u = Usage {
        cache_creation_input_tokens: 1_000_000,
        ..Usage::default()
    };
    let e = flash();
    assert_eq!(e.cost_micros_at(&u, phx(2026, 10, 27, 2, 0)), 300_000);
    assert_eq!(e.cost_micros_at(&u, phx(2026, 10, 27, 10, 0)), 150_000);
}

/// The catalog keeps peak prices for reserves and budgets, and the plain
/// cost is the peak one whatever the hour: the safe direction.
#[test]
fn reserves_and_the_plain_cost_stay_at_peak_prices() {
    let e = flash();
    assert_eq!(e.input_per_mtok, 0.30);
    assert_eq!(e.output_per_mtok, 1.20);
    assert_eq!(e.cache_read_per_mtok, 0.006);
    assert_eq!(e.cost_micros(&usage()), 1_506_000);
    assert_eq!(e.reserve_micros(1_000_000, 1_000_000), 1_500_000);
    assert_eq!(e.reserve_parts(1_000_000, 1_000_000), (1_200_000, 300_000));
}

/// Only a model with an off-peak row changes price with the hour.
#[test]
fn a_model_without_an_off_peak_row_is_priced_the_same_at_any_hour() {
    let c = Catalog::builtin();
    for id in ["claude-sonnet-5-5", "claude-haiku-5-5", "glm-5.3-flash"] {
        let e = c.get(id).unwrap();
        assert!(e.off_peak.is_none(), "{id}");
        assert_eq!(
            e.cost_micros_at(&usage(), phx(2026, 10, 27, 2, 0)),
            e.cost_micros_at(&usage(), phx(2026, 10, 27, 10, 0)),
            "{id}"
        );
    }
}

#[test]
fn each_options_default_effort_is_its_vendors_and_flashs_is_low() {
    let c = Catalog::builtin();
    for (id, want) in [
        ("claude-haiku-5-5", Effort::Medium),
        ("claude-sonnet-5-5", Effort::High),
        ("claude-opus-5-5", Effort::Medium),
        ("claude-fable-5-1", Effort::High),
        ("deepseek-flash", Effort::Low),
    ] {
        assert_eq!(c.get(id).unwrap().default_effort, Some(want), "{id}");
    }
}

#[test]
fn a_config_table_can_set_the_default_effort_and_the_off_peak_prices() {
    let row: CatalogRow = toml::from_str(
        "default_effort = \"max\"\n[off_peak]\ninput_per_mtok = 0.1\noutput_per_mtok = 0.4\n\
         cache_read_per_mtok = 0.002\n",
    )
    .unwrap();
    let base = Catalog::builtin().get("deepseek-flash").cloned();
    let e = row.over(base.as_ref()).unwrap();
    assert_eq!(e.default_effort, Some(Effort::Max));
    assert_eq!(e.off_peak.unwrap().input_per_mtok, 0.1);
    // A table that names neither keeps the built-in row's.
    let keep = CatalogRow::default().over(base.as_ref()).unwrap();
    assert_eq!(keep.default_effort, Some(Effort::Low));
    assert_eq!(keep.off_peak, base.unwrap().off_peak);
}
