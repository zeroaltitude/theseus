//! A time as a person types it (theseus-8ux6x): what `wake.create`'s `when`
//! and `wake.snooze`'s `when` take. A duration (`10m`, `in 1h30m`), a calendar
//! time (`tomorrow 9:00`, `fri 9:00`, `17:30`, `2026-10-14 09:00`, the kernel's
//! `when::parse_calendar`), or an RFC 3339 time with its offset, all in the
//! daemon's zone, then `wake.at`'s bounds: at least 1 s ahead, at most
//! `[kernel] max_wake_ahead_days`.

use theseus_kernel::TimeZone;

use crate::wake::{local, parse_after, parse_at, span, MIN_AHEAD_MS};

/// When `text` is, from `now_ms`, or why it cannot be.
pub fn parse_when(
    text: &str,
    now_ms: u64,
    zone: &TimeZone,
    max_ahead_ms: u64,
) -> Result<u64, String> {
    let t = text.trim();
    let bare = t
        .strip_prefix("in ")
        .or_else(|| t.strip_prefix("after "))
        .unwrap_or(t)
        .trim();
    let due = if let Ok(after) = parse_after(bare) {
        now_ms.saturating_add(after)
    } else if let Some(cal) = theseus_kernel::when::parse_calendar(t, now_ms, zone) {
        cal?
    } else if let Ok(at) = parse_at(t) {
        at
    } else {
        return Err(format!(
            "`{text}` is not a time I can read: say `in 10m`, `tomorrow 9:00`, `fri 9:00`, \
             `17:30`, `2026-10-14 09:00`, or an RFC 3339 time with its offset"
        ));
    };
    if due <= now_ms {
        return Err(format!(
            "`{text}` is {} ago; it is {} now. Give a later time",
            span(now_ms - due),
            local(now_ms).full()
        ));
    }
    let ahead = due - now_ms;
    if ahead < MIN_AHEAD_MS {
        return Err("a wake must be at least 1 s from now".into());
    }
    if ahead > max_ahead_ms {
        return Err(format!(
            "a wake may be at most {} days from now, and `{text}` is {}",
            max_ahead_ms / 86_400_000,
            span(ahead)
        ));
    }
    Ok(due)
}

#[cfg(test)]
mod tests {
    use super::*;

    const YEAR: u64 = 365 * 86_400_000;

    fn zone() -> TimeZone {
        TimeZone::posix("MST7").unwrap()
    }

    #[test]
    fn durations_calendar_times_and_rfc_3339_all_read() {
        let now = parse_at("2026-10-12T17:00:00Z").unwrap(); // 10:00 MST, a Monday
        let w = |t: &str| parse_when(t, now, &zone(), YEAR);
        assert_eq!(w("10m"), Ok(now + 600_000));
        assert_eq!(w("in 1h30m"), Ok(now + 5_400_000));
        assert_eq!(
            w("tomorrow 9:00"),
            Ok(parse_at("2026-10-13T16:00:00Z").unwrap())
        );
        assert_eq!(w("fri 9:00"), Ok(parse_at("2026-10-16T16:00:00Z").unwrap()));
        assert_eq!(w("2026-10-12T20:00:00Z"), Ok(now + 3 * 3_600_000));
    }

    #[test]
    fn the_horizon_is_the_configured_one() {
        let now = parse_at("2026-10-12T17:00:00Z").unwrap();
        assert!(parse_when("365d", now, &zone(), YEAR).is_ok());
        let e = parse_when("366d", now, &zone(), YEAR).unwrap_err();
        assert!(e.contains("at most 365 days"), "{e}");
        assert!(parse_when("30d", now, &zone(), 29 * 86_400_000)
            .unwrap_err()
            .contains("at most 29 days"));
    }

    #[test]
    fn what_cannot_be_read_or_is_past_says_so() {
        let now = parse_at("2026-10-12T17:00:00Z").unwrap();
        let e = parse_when("whenever", now, &zone(), YEAR).unwrap_err();
        assert!(e.contains("not a time I can read"), "{e}");
        let e = parse_when("2026-10-12T16:00:00Z", now, &zone(), YEAR).unwrap_err();
        assert!(e.contains("ago"), "{e}");
        assert!(parse_when("0s", now, &zone(), YEAR).is_err());
    }
}
