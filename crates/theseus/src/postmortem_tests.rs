//! `theseus postmortem`'s flags: the times they take.

use super::*;
use theseus_client::render::time::{fmt_stamp, pin_for_tests};

/// 2026-03-14 09:12:05 UTC, which is 02:12:05 at the tests' UTC−7.
const NOW: u64 = 1_773_479_525_000;

#[test]
fn an_age_is_that_long_before_now() {
    pin_for_tests();
    assert_eq!(when("2h", NOW).unwrap(), NOW - 7_200_000);
    assert_eq!(when("30m", NOW).unwrap(), NOW - 1_800_000);
    assert_eq!(when("1d", NOW).unwrap(), NOW - 86_400_000);
}

#[test]
fn a_time_of_day_reads_on_this_machines_clock() {
    pin_for_tests();
    assert_eq!(
        fmt_stamp(when("yesterday 22:00", NOW).unwrap()),
        "2026-03-13 22:00:00"
    );
    assert_eq!(
        fmt_stamp(when("today 01:30", NOW).unwrap()),
        "2026-03-14 01:30:00"
    );
    assert_eq!(
        fmt_stamp(when("yesterday", NOW).unwrap()),
        "2026-03-13 00:00:00"
    );
    // A bare time that is still ahead today is yesterday's.
    assert_eq!(
        fmt_stamp(when("22:00", NOW).unwrap()),
        "2026-03-13 22:00:00"
    );
    assert_eq!(
        fmt_stamp(when("01:00", NOW).unwrap()),
        "2026-03-14 01:00:00"
    );
}

#[test]
fn an_rfc_3339_time_is_itself() {
    pin_for_tests();
    assert_eq!(when("2026-03-14T09:12:05Z", NOW).unwrap(), NOW);
    assert_eq!(when("2026-03-14T02:12:05-07:00", NOW).unwrap(), NOW);
}

#[test]
fn nonsense_is_refused_with_the_forms_it_could_take() {
    pin_for_tests();
    for bad in ["", "soon", "25:00", "yesterday 9", "7y"] {
        let e = when(bad, NOW).unwrap_err().to_string();
        assert!(e.contains("yesterday 22:00"), "{bad}: {e}");
    }
}
