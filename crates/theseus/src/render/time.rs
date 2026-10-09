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

/// This machine's zone, as `TZ` names it (an IANA name, a POSIX string, or a
/// file), else `/etc/localtime`, each read alone: jiff's own `system()` lists
/// the whole tz database to name `/etc/localtime`, a sixth of a millisecond
/// and more on every command that prints a time.
fn machine_zone() -> TimeZone {
    const ZONEINFO: &str = "/usr/share/zoneinfo";
    let tzif = |name: &str, path: &Path| {
        std::fs::read(path)
            .ok()
            .and_then(|bytes| TimeZone::tzif(name, &bytes).ok())
    };
    let found = match std::env::var("TZ") {
        Ok(tz) if !tz.is_empty() => {
            let name = tz.strip_prefix(':').unwrap_or(&tz);
            if name.starts_with('/') {
                tzif(name, Path::new(name))
            } else if name.split('/').any(|part| part == "..") {
                None
            } else {
                tzif(name, &Path::new(ZONEINFO).join(name)).or_else(|| TimeZone::posix(name).ok())
            }
        }
        _ => {
            let local = Path::new("/etc/localtime");
            let target = std::fs::read_link(local).ok();
            let name = target
                .as_deref()
                .and_then(|t| t.to_str())
                .and_then(|t| t.split_once("zoneinfo/").map(|(_, n)| n.to_string()))
                .unwrap_or_else(|| "Local".to_string());
            tzif(&name, local)
        }
    };
    found.unwrap_or_else(TimeZone::system)
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
