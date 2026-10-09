//! Times as people read them: on this machine's clock in every human output
//! of the CLI and the terminal UI, and UTC only under `--json`, which prints
//! the daemon's numbers as they are (the owner's decision D5, theseus-0n1v).
//! The zone is read once, when the first time is printed, never at a start;
//! jiff reads `TZ` (an IANA name or a POSIX string), then `/etc/localtime`.

use std::sync::OnceLock;

use jiff::tz::{Offset, TimeZone};
use jiff::Timestamp;

static ZONE: OnceLock<TimeZone> = OnceLock::new();

/// The zone times are written in: the machine's, read at the first time
/// printed. This crate's own tests write every time at a fixed UTC−7, so no
/// test depends on the machine's zone.
fn zone() -> &'static TimeZone {
    ZONE.get_or_init(|| {
        if cfg!(test) {
            fixed_for_tests()
        } else {
            TimeZone::system()
        }
    })
}

/// The zone the tests write times in: UTC−7, all year.
fn fixed_for_tests() -> TimeZone {
    TimeZone::fixed(Offset::constant(-7))
}

/// Write every time at the tests' fixed UTC−7 from now on, when no time has
/// been written yet: the pin of a test harness in another crate (the
/// terminal UI's rig), so no test depends on the machine's zone.
pub fn pin_for_tests() {
    let _ = ZONE.set(fixed_for_tests());
}

fn at(unix_ms: u64, pattern: &str) -> String {
    let ts = i64::try_from(unix_ms)
        .ok()
        .and_then(|ms| Timestamp::from_millisecond(ms).ok())
        .unwrap_or(Timestamp::UNIX_EPOCH);
    ts.to_zoned(zone().clone()).strftime(pattern).to_string()
}

/// A time of day to the millisecond: `18:16:01.600`.
pub fn fmt_time(unix_ms: u64) -> String {
    at(unix_ms, "%H:%M:%S%.3f")
}

/// A time of day to the minute: `18:16`.
pub fn fmt_hm(unix_ms: u64) -> String {
    at(unix_ms, "%H:%M")
}

/// A date and a time to the minute: `2026-03-14 02:12`.
pub fn fmt_date(unix_ms: u64) -> String {
    at(unix_ms, "%Y-%m-%d %H:%M")
}

/// A date and a time to the second: `2026-03-14 02:12:05`.
pub fn fmt_stamp(unix_ms: u64) -> String {
    at(unix_ms, "%Y-%m-%d %H:%M:%S")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn times_are_written_in_the_pinned_zone() {
        // 2026-03-14 09:12:05.042 UTC is 02:12 at UTC−7.
        let ms = 1_773_479_525_042;
        assert_eq!(fmt_time(ms), "02:12:05.042");
        assert_eq!(fmt_hm(ms), "02:12");
        assert_eq!(fmt_date(ms), "2026-03-14 02:12");
        assert_eq!(fmt_stamp(ms), "2026-03-14 02:12:05");
        assert_eq!(fmt_time(0), "17:00:00.000");
    }
}
