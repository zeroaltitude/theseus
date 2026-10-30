//! DeepSeek's peak hours, as the clock reads them (theseus-9f72b, design §1.3).
//!
//! DeepSeek charges its peak prices from 01:00 to 04:00 and from 06:00 to
//! 10:00 UTC, Monday to Friday, except on Chinese public holidays (vendor
//! pricing page, read 2026-10-17). In Phoenix (UTC-7, no daylight saving) that
//! is 18:00 to 21:00 and 23:00 to 03:00, Sunday evening through Friday 03:00.
//!
//! The holidays are deliberately not modelled: a holiday reads as peak, the
//! safe over-estimate. The instant is a parameter, so the rule is tested at its
//! edges without a clock.

use theseus_judge::builders::PricePeriod;

/// Whether `at_ms` (Unix milliseconds) falls in a provider's peak hours.
pub fn price_period_at(at_ms: u64) -> PricePeriod {
    let secs = at_ms / 1_000;
    // 1970-01-01 was a Thursday; Monday is 0.
    let weekday = (secs / 86_400 + 3) % 7;
    let hour = (secs % 86_400) / 3_600;
    if weekday < 5 && matches!(hour, 1..=3 | 6..=9) {
        PricePeriod::Peak
    } else {
        PricePeriod::OffPeak
    }
}
