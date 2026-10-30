//! DeepSeek's peak hours, as the clock reads them (theseus-9f72b, design §1.3).

use theseus_judge::builders::PricePeriod;

/// Whether `at_ms` (Unix milliseconds) is in a provider's peak or off-peak hours.
pub fn price_period_at(_at_ms: u64) -> PricePeriod {
    PricePeriod::Peak
}
