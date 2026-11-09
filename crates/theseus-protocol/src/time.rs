//! The clock and the civil calendar, once: the wall clock in milliseconds, and
//! days since 1970-01-01 to and from a proleptic Gregorian date (Howard
//! Hinnant's algorithm). Total over every input, so a date a person typed
//! needs no check first.

use std::sync::atomic::{AtomicI64, Ordering};

/// How far a test has moved the clock, in milliseconds (`move_clock_to`): zero until one does, and no daemon does.
static MOVED_MS: AtomicI64 = AtomicI64::new(0);

/// The wall clock, in milliseconds since the Unix epoch (0 before it): the real one, unless a test has moved it.
pub fn now_unix_ms() -> u64 {
    wall_ms().saturating_add_signed(MOVED_MS.load(Ordering::Relaxed))
}

/// The real wall clock, which nothing moves.
fn wall_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

/// How far `at_ms` is from `wall_ms`, signed.
fn moved_by(wall_ms: u64, at_ms: u64) -> i64 {
    let by = i128::from(at_ms) - i128::from(wall_ms);
    i64::try_from(by).unwrap_or(if by < 0 { i64::MIN } else { i64::MAX })
}

/// A test's clock: after this `now_unix_ms` reads `at_ms` and runs on from there at the wall clock's rate, so a test of
/// the hour's or the local day's line starts where its seconds cross neither (theseus-4vsrz: a suite that walked through
/// midnight, or the hour, failed the tests that took the period from the real clock). Every reader of `now_unix_ms` moves
/// with it, the kernel's `RealClock` and the store's stamps among them. It moves the whole process: nextest, which the
/// gate runs, gives each test a process of its own, and under `cargo test`'s threads a move reaches the tests beside it.
pub fn move_clock_to(at_ms: u64) {
    MOVED_MS.store(moved_by(wall_ms(), at_ms), Ordering::Relaxed);
}

/// The clock back on the wall clock.
pub fn reset_clock() {
    MOVED_MS.store(0, Ordering::Relaxed);
}

/// Days since 1970-01-01 of a civil date.
pub fn days_from_civil(y: i64, m: u32, d: u32) -> i64 {
    let y = if m <= 2 { y - 1 } else { y };
    let era = y.div_euclid(400);
    let yoe = y - era * 400;
    let mp = (i64::from(m) + 9) % 12;
    let doy = (153 * mp + 2) / 5 + i64::from(d) - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146_097 + doe - 719_468
}

/// The civil date `(year, month, day)` of a day count since 1970-01-01.
pub fn civil_from_days(z: i64) -> (i64, u32, u32) {
    let z = z + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let m = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    (yoe + era * 400 + i64::from(m <= 2), m, d)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn civil_dates_are_pinned_at_a_leap_day_and_the_year_boundaries() {
        for (days, ymd) in [
            (-1, (1969, 12, 31)),
            (0, (1970, 1, 1)),
            (11_016, (2000, 2, 29)),
            (11_017, (2000, 3, 1)),
            (19_782, (2024, 2, 29)),
            (20_088, (2024, 12, 31)),
            (20_089, (2025, 1, 1)),
            (-719_528, (0, 1, 1)),
        ] {
            assert_eq!(civil_from_days(days), ymd, "day {days}");
            assert_eq!(days_from_civil(ymd.0, ymd.1, ymd.2), days, "date {ymd:?}");
        }
    }

    #[test]
    fn the_two_directions_agree_over_four_centuries() {
        for d in (-146_097..146_097).step_by(7) {
            let (y, m, day) = civil_from_days(d);
            assert_eq!(days_from_civil(y, m, day), d);
        }
    }

    #[test]
    fn the_clock_reads_after_this_code_was_written() {
        assert!(now_unix_ms() > 1_700_000_000_000);
    }

    #[test]
    fn a_move_is_the_distance_from_the_wall_clock_either_way() {
        assert_eq!(moved_by(1_000, 4_000), 3_000);
        assert_eq!(moved_by(4_000, 1_000), -3_000);
        assert_eq!(moved_by(7, 7), 0);
        assert_eq!(
            moved_by(0, u64::MAX),
            i64::MAX,
            "a distance past i64 stops at it"
        );
    }

    #[test]
    fn a_moved_clock_reads_where_it_was_put_and_runs_on_at_the_wall_clocks_rate() {
        // Far ahead, so a test beside this one in a shared process still reads after this code was written.
        let at = 4_102_444_800_000;
        move_clock_to(at);
        let first = now_unix_ms();
        assert!(
            (at..at + 5_000).contains(&first),
            "{first} is not where it was put, {at}"
        );
        std::thread::sleep(std::time::Duration::from_millis(50));
        let later = now_unix_ms();
        assert!(
            later >= first + 50 && later < first + 5_000,
            "ran from {first} to {later} in 50 ms"
        );
        reset_clock();
        assert!(now_unix_ms() < at, "the clock is back on the wall clock");
    }
}
