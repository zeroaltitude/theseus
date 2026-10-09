//! Times as people read them: on this machine's clock in every human output
//! of the CLI and the terminal UI, and UTC only under `--json`, which prints
//! the daemon's numbers as they are (the owner's decision D5, theseus-0n1v).
//! The zone is read once, when the first time is printed, never at a start;
//! jiff reads `TZ` (an IANA name or a POSIX string), then `/etc/localtime`.

use std::path::Path;
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
            machine_zone()
        }
    })
}

/// This machine's zone, from `TZ` and `/etc/localtime`.
fn machine_zone() -> TimeZone {
    zone_from(
        std::env::var("TZ").ok().as_deref(),
        Path::new("/etc/localtime"),
    )
}

/// The zone `tz` (the `TZ` variable) names, else the one `localtime` holds,
/// each read alone, else UTC, as the C library takes them: an empty `TZ` is
/// UTC, and so is one that names no zone, or a missing `localtime`
/// (theseus-0n1v's review). Never jiff's own `system()`: it lists the whole
/// tz database to name a zone, a sixth of a millisecond and more on every
/// command that prints a time.
fn zone_from(tz: Option<&str>, localtime: &Path) -> TimeZone {
    const ZONEINFO: &str = "/usr/share/zoneinfo";
    let tzif = |name: &str, path: &Path| {
        std::fs::read(path)
            .ok()
            .and_then(|bytes| TimeZone::tzif(name, &bytes).ok())
    };
    let found = match tz {
        Some("") => None,
        Some(tz) => {
            let name = tz.strip_prefix(':').unwrap_or(tz);
            if name.starts_with('/') {
                tzif(name, Path::new(name))
            } else if name.split('/').any(|part| part == "..") {
                None
            } else {
                tzif(name, &Path::new(ZONEINFO).join(name)).or_else(|| TimeZone::posix(name).ok())
            }
        }
        None => {
            let target = std::fs::read_link(localtime).ok();
            let name = target
                .as_deref()
                .and_then(|t| t.to_str())
                .and_then(|t| t.split_once("zoneinfo/").map(|(_, n)| n.to_string()))
                .unwrap_or_else(|| "Local".to_string());
            tzif(&name, localtime)
        }
    };
    found.unwrap_or(TimeZone::UTC)
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

    /// Each `TZ` and `/etc/localtime` as the C library reads them, and never
    /// jiff's `system()` (the review of theseus-0n1v).
    #[test]
    fn the_zone_is_read_as_the_c_library_reads_it() {
        let at = |z: &TimeZone, ms: i64| {
            Timestamp::from_millisecond(ms)
                .unwrap()
                .to_zoned(z.clone())
                .strftime("%H:%M %Z")
                .to_string()
        };
        let none = Path::new("/nonexistent/localtime");
        let file = Path::new("/usr/share/zoneinfo/Asia/Kolkata");
        // `localtime` names a zone of its own where the machine has one, so a
        // `TZ` that names none is seen not to fall back to it.
        let local = if file.exists() { file } else { none };
        let noon = 1_793_534_400_000; // 2026-11-01 12:00 UTC
        assert_eq!(at(&zone_from(Some(""), local), noon), "12:00 UTC");
        assert_eq!(
            at(&zone_from(Some("Mars/Olympus"), local), noon),
            "12:00 UTC"
        );
        assert_eq!(
            at(&zone_from(Some("../../etc/passwd"), local), noon),
            "12:00 UTC"
        );
        assert_eq!(at(&zone_from(None, none), noon), "12:00 UTC");
        assert_eq!(at(&zone_from(Some("<-07>7"), none), noon), "05:00 -07");
        // The repeated hour: 07:30 and 08:30 UTC are both 01:30 here, once
        // in daylight time and once not.
        let us = zone_from(Some("MST7MDT,M3.2.0,M11.1.0"), none);
        assert_eq!(at(&us, 1_793_518_200_000), "01:30 MDT");
        assert_eq!(at(&us, 1_793_521_800_000), "01:30 MST");
        // A zone file, named by `TZ` or as `/etc/localtime`.
        if file.exists() {
            let named = format!(":{}", file.display());
            assert_eq!(at(&zone_from(Some(&named), none), noon), "17:30 IST");
            assert_eq!(at(&zone_from(None, file), noon), "17:30 IST");
        }
    }
}
