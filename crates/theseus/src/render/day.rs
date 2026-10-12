//! Health's first line while the day's model spend is past its ceiling
//! (theseus-9e02): what was spent, that the work goes on or has stopped, where
//! the stop is, and how to stop now. Apart from `render.rs`, whose length the
//! shape budget caps (`scripts/long-files.txt`).

use theseus_protocol::DayCeilingBudget;

use super::{push, Line, Tag};

/// Health's first line, when the day is past its ceiling.
pub fn push_first(out: &mut Vec<Line>, d: Option<&DayCeilingBudget>) {
    if let Some(line) = first_line(d) {
        push(out, Tag::Ask, &line);
    }
}

/// Health's first lines: the day's warning while it is past its ceiling, then
/// which features are on (theseus-is8t), in one call so `render.rs` stays
/// inside its ceiling.
pub fn push_head(out: &mut Vec<Line>, h: &theseus_protocol::HealthResult) {
    push_first(out, h.day_spend.as_ref());
    super::features::push_health(out, h.features.as_ref());
}

/// `⚠ model spend today $412.50, 2.1× the $200.00 daily ceiling: work goes on; …`
/// Nothing while the day is under its ceiling.
pub fn first_line(d: Option<&DayCeilingBudget>) -> Option<String> {
    let d = d?;
    if d.reached {
        let line = if d.mode == "notify" {
            d.runaway_usd
        } else {
            d.ceiling_usd
        };
        return Some(format!(
            "⛔ model spend today ${:.2}: the {} line (${line:.2}) stopped model calls, and none \
             will be made until the day turns at {} local time",
            d.spent_usd,
            if d.mode == "notify" {
                "daily runaway"
            } else {
                "daily ceiling"
            },
            d.turns_at
        ));
    }
    if !d.past {
        return None;
    }
    let stop = match d.runaway_usd > 0.0 {
        true => format!(
            "model calls stop at ${:.2} (`[kernel] daily_runaway_usd`)",
            d.runaway_usd
        ),
        false => "no runaway line is set".to_string(),
    };
    Some(format!(
        "⚠ model spend today ${:.2}, {:.1}× the ${:.2} daily ceiling: the work goes on, {stop}; \
         stop now with `theseus stop`",
        d.spent_usd,
        d.spent_usd / d.ceiling_usd.max(f64::MIN_POSITIVE),
        d.ceiling_usd
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn day(spent: f64, mode: &str) -> DayCeilingBudget {
        DayCeilingBudget {
            day: "2026-10-11".into(),
            ceiling_usd: 200.0,
            spent_usd: spent,
            turns_at: "2026-10-12 00:00".into(),
            mode: mode.into(),
            runaway_usd: if mode == "notify" { 2000.0 } else { 0.0 },
            past: spent >= 200.0,
            ..DayCeilingBudget::default()
        }
    }

    /// The first line of health while the day is past its ceiling (theseus-9e02):
    /// nothing under it; a warning that says the work goes on, where the stop is
    /// and how to stop now; and, at the runaway line, the stop.
    #[test]
    fn health_leads_with_the_day_past_its_ceiling() {
        assert_eq!(first_line(None), None);
        assert_eq!(first_line(Some(&day(150.0, "notify"))), None);
        assert_eq!(
            first_line(Some(&day(412.5, "notify"))).unwrap(),
            "⚠ model spend today $412.50, 2.1× the $200.00 daily ceiling: the work goes on, \
             model calls stop at $2000.00 (`[kernel] daily_runaway_usd`); stop now with `theseus stop`"
        );
        let mut stopped = day(2000.0, "notify");
        stopped.reached = true;
        assert_eq!(
            first_line(Some(&stopped)).unwrap(),
            "⛔ model spend today $2000.00: the daily runaway line ($2000.00) stopped model calls, \
             and none will be made until the day turns at 2026-10-12 00:00 local time"
        );
        let mut off = day(412.5, "notify");
        off.runaway_usd = 0.0;
        assert!(first_line(Some(&off))
            .unwrap()
            .contains("no runaway line is set"));
    }
}
