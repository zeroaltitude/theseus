//! Repeating wakes (37a, theseus-d4pt): a series is one pending wake that
//! the frame taking it puts back at its next occurrence; occurrences passed
//! over while the daemon was down are counted, never run; a cancel ends the
//! series, and `until` ends it with a `wake.ended` row; the series counts
//! once against the cap; and its calendar spans hold their wall time across
//! a change of offset.

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;

use jiff::tz::TimeZone;

use crate::kernel::*;
use crate::repeat::{Day, Every, Repeat};
use crate::tests::*;
use crate::types::*;
use crate::wakes::{wake_id, FiredWake, MAX_PENDING};

const MIN: u64 = 60_000;
const HOUR: u64 = 60 * MIN;
const DAY: u64 = 24 * HOUR;

/// New York's rule, as a POSIX TZ string, so no tz database is read: EST,
/// and EDT from the second Sunday of March to the first of November.
fn new_york() -> TimeZone {
    TimeZone::posix("EST5EDT,M3.2.0,M11.1.0").unwrap()
}

/// Phoenix: MST all year, no change.
fn phoenix() -> TimeZone {
    TimeZone::posix("MST7").unwrap()
}

/// Unix ms of an RFC 3339 time.
fn ms(s: &str) -> u64 {
    s.parse::<jiff::Timestamp>().unwrap().as_millisecond() as u64
}

/// `ms` as wall time in `zone`: `2026-03-08T21:00:00`.
fn wall(zone: &TimeZone, ms: u64) -> String {
    jiff::Timestamp::from_millisecond(ms as i64)
        .unwrap()
        .to_zoned(zone.clone())
        .datetime()
        .to_string()
}

fn every(s: &str) -> Every {
    Every::parse(s).unwrap()
}

fn series(e: &str, first_ms: u64) -> Repeat {
    Repeat {
        every: every(e),
        first_ms,
        days: vec![],
        until_ms: None,
    }
}

fn exec(w: &World, id: &str) -> Execution {
    w.kernel.execution(id).unwrap().unwrap()
}

/// A conversation whose turn set a repeating wake, first due `in_ms` from
/// now, and parked on input: the execution and the wake's id.
fn with_series(w: &World, in_ms: u64, repeat: impl FnOnce(u64) -> Repeat) -> (Execution, String) {
    let (_, e, g) = running(w);
    let call = new_id("act");
    let first = w.kernel.now_ms() + in_ms;
    let set = w
        .kernel
        .set_wake(
            &g,
            &call,
            first,
            "write me a haiku",
            None,
            Some(repeat(first)),
        )
        .unwrap();
    assert!(set.set);
    assert_eq!(set.wake.occurrence, 1, "the first occurrence is #1");
    w.kernel
        .end_turn(g, TurnEnd::Wait { wake: Wake::Input })
        .unwrap();
    (exec(w, &e.id), wake_id(&call))
}

/// The due scan queues `id`, a turn takes its due wakes, and parks on input.
fn fire(w: &World, id: &str) -> Vec<FiredWake> {
    assert_eq!(
        w.kernel.reconcile(&NoEvidence).unwrap().woke_due,
        vec![id.to_string()],
        "the due scan queues it"
    );
    let g = w.kernel.admit(id).unwrap();
    let fired = w.kernel.take_wakes(&g, |_| Ok(vec![])).unwrap();
    w.kernel
        .end_turn(g, TurnEnd::Wait { wake: Wake::Input })
        .unwrap();
    fired
}

/// A series' span parses as the tool takes it, and says itself back.
#[test]
fn a_span_parses_in_its_units_and_says_itself_back() {
    assert_eq!(every("5m").nominal_ms(), 5 * MIN);
    assert_eq!(every("2h").nominal_ms(), 2 * HOUR);
    assert_eq!(every("1d").nominal_ms(), DAY);
    assert_eq!(every(" 2 weeks ").nominal_ms(), 14 * DAY);
    assert_eq!(every("90 minutes").to_string(), "90m");
    for bad in [
        "",
        "d",
        "0d",
        "1.5h",
        "-1d",
        "1y",
        "10s",
        "1d2h",
        "99999999999m",
    ] {
        assert!(
            Every::parse(bad).unwrap_err().contains("such as 30m"),
            "{bad}"
        );
    }
    assert_eq!(Day::parse("Monday"), Some(Day::Mon));
    assert_eq!(Day::parse("thurs"), Some(Day::Thu));
    assert_eq!(Day::parse("SUN"), Some(Day::Sun));
    assert_eq!(Day::parse("mo"), None);
    assert_eq!(Day::parse("mondays"), None);
}

/// "Daily 21:00" in New York stays 21:00 on both sides of each change of a
/// year (March 8 and November 1, 2026): the day across the spring change is
/// 23 hours, the one across the autumn change 25. An hourly series is exact
/// time, so its wall clock moves by the change. A wall time the spring
/// change skips (02:30) lands after the gap that day, and is 02:30 again the
/// next.
#[test]
fn a_daily_series_keeps_its_wall_time_across_both_changes_of_a_year() {
    let ny = new_york();
    for (first, change) in [
        ("2026-03-05T21:00:00-05:00", "2026-03-08"),
        ("2026-10-29T21:00:00-04:00", "2026-11-01"),
    ] {
        let r = series("1d", ms(first));
        let times: Vec<u64> = (0..6).map(|k| r.at(&ny, k).unwrap()).collect();
        for t in &times {
            assert!(wall(&ny, *t).ends_with("T21:00:00"), "{}", wall(&ny, *t));
        }
        let lengths: Vec<u64> = times.windows(2).map(|p| (p[1] - p[0]) / HOUR).collect();
        let spring = change.starts_with("2026-03");
        // Thu, Fri, Sat → Sun crosses the change.
        assert_eq!(
            lengths,
            vec![24, 24, if spring { 23 } else { 25 }, 24, 24],
            "{change}"
        );
        // The next after each occurrence is the one after it.
        for k in 0..5 {
            assert_eq!(r.next_after(&ny, times[k]), Some(times[k + 1]));
            assert_eq!(r.next_after(&ny, times[k + 1] - 1), Some(times[k + 1]));
        }
    }
    let hourly = series("1h", ms("2026-03-08T00:30:00-05:00"));
    let t: Vec<String> = (0..4)
        .map(|k| wall(&ny, hourly.at(&ny, k).unwrap()))
        .collect();
    assert_eq!(
        t,
        [
            "2026-03-08T00:30:00",
            "2026-03-08T01:30:00",
            "2026-03-08T03:30:00",
            "2026-03-08T04:30:00"
        ]
    );
    let gap = series("1d", ms("2026-03-07T02:30:00-05:00"));
    let t: Vec<String> = (0..3).map(|k| wall(&ny, gap.at(&ny, k).unwrap())).collect();
    assert_eq!(
        t,
        [
            "2026-03-07T02:30:00",
            "2026-03-08T03:30:00",
            "2026-03-09T02:30:00"
        ]
    );
}

/// Phoenix has no change: every day of a daily series is 24 hours, through
/// the dates New York changes on, and 21:00 is 04:00 UTC all year.
#[test]
fn a_daily_series_in_a_zone_with_no_change_is_24_hours_every_day() {
    let az = phoenix();
    let r = series("1d", ms("2026-03-01T21:00:00-07:00"));
    for k in 0..300 {
        let t = r.at(&az, k).unwrap();
        assert_eq!(t - r.first_ms, k * DAY);
        assert!(wall(&az, t).ends_with("T21:00:00"));
        assert_eq!(wall(&TimeZone::UTC, t).get(11..), Some("04:00:00"));
    }
}

/// The days limit a daily series to weekdays: Friday's next is Monday, and
/// a series first set on a Saturday starts on Monday.
#[test]
fn a_series_limited_to_weekdays_passes_over_the_weekend() {
    let ny = new_york();
    let weekdays = vec![Day::Mon, Day::Tue, Day::Wed, Day::Thu, Day::Fri];
    let r = Repeat {
        days: weekdays.clone(),
        ..series("1d", ms("2026-10-02T07:00:00-04:00"))
    };
    // Friday the 2nd, then Monday the 5th.
    assert_eq!(
        r.next_after(&ny, r.first_ms).map(|t| wall(&ny, t)),
        Some("2026-10-05T07:00:00".into())
    );
    assert_eq!(r.first(&ny), Some(r.first_ms));
    let sat = Repeat {
        days: weekdays,
        ..series("1d", ms("2026-10-03T07:00:00-04:00"))
    };
    assert_eq!(
        sat.first(&ny).map(|t| wall(&ny, t)),
        Some("2026-10-05T07:00:00".into())
    );
    // From Friday's to the next Friday's: the weekdays between, 4.
    assert_eq!(r.between(&ny, r.first_ms, r.first_ms + 7 * DAY), 5);
    assert_eq!(r.between(&ny, r.first_ms, r.first_ms + 7 * DAY - 1), 4);
}

/// The take that runs a due occurrence puts the next back on the list in
/// its own frame: the same id, the next occurrence's number, due one span
/// after the last, its row saying which ran and when the next is due. It
/// fires again at that time, and the take writes one frame each time, as a
/// one-shot wake's does.
#[test]
fn a_due_occurrence_is_put_back_at_the_next_in_the_frame_that_takes_it() {
    let w = world_with(KernelConfig {
        zone: TimeZone::UTC,
        ..KernelConfig::default()
    });
    let frames = Arc::new(AtomicUsize::new(0));
    let f = frames.clone();
    assert!(w.kernel.observe(Arc::new(move |_| {
        f.fetch_add(1, Ordering::SeqCst);
    })));
    let (e, wid) = with_series(&w, 5 * MIN, |first| series("5m", first));
    let first = e.wakes[0].due_at_ms;
    let set = &rows(&w, &e.session_id, "wake.set")[0];
    assert_eq!(set["every"], "5m");

    w.clock.advance(5 * MIN);
    w.kernel.fire_due(&e.id).unwrap().unwrap();
    let g = w.kernel.admit(&e.id).unwrap();
    let before = frames.load(Ordering::SeqCst);
    let fired = w.kernel.take_wakes(&g, |_| Ok(vec![])).unwrap();
    assert_eq!(
        frames.load(Ordering::SeqCst) - before,
        1,
        "the take is one frame"
    );
    w.kernel
        .end_turn(g, TurnEnd::Wait { wake: Wake::Input })
        .unwrap();
    assert_eq!(fired.len(), 1);
    assert_eq!(
        (
            fired[0].wake.occurrence,
            fired[0].missed,
            fired[0].next_due_at_ms
        ),
        (1, 0, Some(first + 5 * MIN))
    );
    assert!(!fired[0].ended());
    let e1 = exec(&w, &e.id);
    assert_eq!(e1.wakes.len(), 1, "one wake: the series");
    let next = &e1.wakes[0];
    assert_eq!(
        (next.id.as_str(), next.occurrence, next.due_at_ms),
        (wid.as_str(), 2, first + 5 * MIN)
    );
    assert_eq!(next.repeat, e.wakes[0].repeat, "the series is the same");
    let row = &rows(&w, &e.session_id, "wake.fired")[0];
    assert_eq!(
        (
            row["occurrence"].as_u64(),
            row["missed"].as_u64(),
            row["next_due_at_ms"].as_u64(),
            row["every"].as_str()
        ),
        (Some(1), Some(0), Some(first + 5 * MIN), Some("5m"))
    );

    w.clock.advance(5 * MIN);
    let fired = fire(&w, &e.id);
    assert_eq!(
        (fired[0].wake.occurrence, fired[0].next_due_at_ms),
        (2, Some(first + 10 * MIN))
    );
    assert_eq!(exec(&w, &e.id).wakes[0].occurrence, 3);
    assert_eq!(rows(&w, &e.session_id, "wake.fired").len(), 2);
    assert!(rows(&w, &e.session_id, "wake.ended").is_empty());
}

/// Down across three occurrences: after the restart one turn takes the one
/// that fell due first, counts the three it passed over, and the next is
/// the first after now, numbered past them. Never a burst: the next scan
/// finds nothing due.
#[test]
fn occurrences_missed_while_down_are_counted_and_run_once() {
    let cfg = || KernelConfig {
        zone: TimeZone::UTC,
        ..KernelConfig::default()
    };
    let w = world_with(cfg());
    let (e, wid) = with_series(&w, 5 * MIN, |first| series("5m", first));
    let first = e.wakes[0].due_at_ms;
    // Down from now until 22 minutes on: due at 5, 10, 15, and 20.
    w.clock.advance(22 * MIN);
    let (w, _) = crash(w, cfg());
    let fired = fire(&w, &e.id);
    assert_eq!(fired.len(), 1, "one turn");
    let f = &fired[0];
    assert_eq!(f.wake.id, wid);
    assert_eq!(f.wake.due_at_ms, first);
    assert!(f.while_down);
    assert_eq!(f.missed, 3);
    assert_eq!(f.late_ms, 17 * MIN);
    assert_eq!(f.next_due_at_ms, Some(first + 20 * MIN));
    let next = &exec(&w, &e.id).wakes[0];
    assert_eq!((next.occurrence, next.due_at_ms), (5, first + 20 * MIN));
    let row = &rows(&w, &e.session_id, "wake.fired")[0];
    assert_eq!(
        (row["missed"].as_u64(), row["while_down"].as_bool()),
        (Some(3), Some(true))
    );
    assert!(
        w.kernel.reconcile(&NoEvidence).unwrap().woke_due.is_empty(),
        "never a burst"
    );
    assert_eq!(rows(&w, &e.session_id, "wake.fired").len(), 1);
}

/// A cancel ends the series: nothing fires at its next occurrence, or after.
#[test]
fn a_cancel_ends_the_series() {
    let w = world_with(KernelConfig {
        zone: TimeZone::UTC,
        ..KernelConfig::default()
    });
    let (e, wid) = with_series(&w, 5 * MIN, |first| series("5m", first));
    w.clock.advance(5 * MIN);
    fire(&w, &e.id);
    let (after, gone) = w
        .kernel
        .cancel_wake(&e.id, &wid, "the CLI")
        .unwrap()
        .unwrap();
    assert_eq!(gone.occurrence, 2);
    assert!(after.wakes.is_empty());
    for _ in 0..4 {
        w.clock.advance(5 * MIN);
        assert!(w.kernel.reconcile(&NoEvidence).unwrap().woke_due.is_empty());
    }
    assert!(w.kernel.pending_wakes().unwrap().is_empty());
    assert_eq!(rows(&w, &e.session_id, "wake.fired").len(), 1);
    assert_eq!(rows(&w, &e.session_id, "wake.cancelled").len(), 1);
}

/// `until` ends the series: its last occurrence is the last before it, and
/// the frame that takes that one writes `wake.ended` and puts nothing back.
/// A series down across its `until` runs once, and ends.
#[test]
fn until_ends_the_series_with_a_row() {
    let cfg = || KernelConfig {
        zone: TimeZone::UTC,
        ..KernelConfig::default()
    };
    let w = world_with(cfg());
    let (e, wid) = with_series(&w, 5 * MIN, |first| Repeat {
        until_ms: Some(first + 12 * MIN),
        ..series("5m", first)
    });
    let first = e.wakes[0].due_at_ms;
    for k in 0..2 {
        w.clock.advance(5 * MIN);
        let f = &fire(&w, &e.id)[0];
        assert_eq!(f.next_due_at_ms, Some(first + (k + 1) * 5 * MIN));
    }
    w.clock.advance(5 * MIN);
    let f = &fire(&w, &e.id)[0];
    assert_eq!(f.wake.occurrence, 3);
    assert!(f.ended() && f.next_due_at_ms.is_none());
    assert!(exec(&w, &e.id).wakes.is_empty());
    let ended = rows(&w, &e.session_id, "wake.ended");
    assert_eq!(ended.len(), 1);
    assert_eq!(
        (
            ended[0]["wake_id"].as_str(),
            ended[0]["occurrence"].as_u64(),
            ended[0]["until_ms"].as_u64(),
            ended[0]["why"].as_str()
        ),
        (
            Some(wid.as_str()),
            Some(3),
            Some(first + 12 * MIN),
            Some("until")
        )
    );
    w.clock.advance(HOUR);
    assert!(w.kernel.reconcile(&NoEvidence).unwrap().woke_due.is_empty());

    let (e2, _) = with_series(&w, 5 * MIN, |first| Repeat {
        until_ms: Some(first + 12 * MIN),
        ..series("5m", first)
    });
    w.clock.advance(HOUR);
    let (w, _) = crash(w, cfg());
    let f = &fire(&w, &e2.id)[0];
    assert_eq!((f.missed, f.ended()), (2, true), "5 and 10 passed over");
    assert!(exec(&w, &e2.id).wakes.is_empty());
}

/// A series counts once against the cap, however often it runs, and the
/// floor refuses a span shorter than `min_repeat_ms`, writing nothing.
#[test]
fn a_series_counts_once_against_the_cap_and_the_floor_holds() {
    let w = world_with(KernelConfig {
        zone: TimeZone::UTC,
        ..KernelConfig::default()
    });
    let (_, e, g) = running(&w);
    let now = w.kernel.now_ms();
    let before = w.kernel.store().last_position();
    let err = w
        .kernel
        .set_wake(
            &g,
            &new_id("act"),
            now + MIN,
            "too often",
            None,
            Some(series("4m", now + MIN)),
        )
        .unwrap_err();
    assert!(
        err.to_string()
            .contains("every 5 minutes at the most often"),
        "{err}"
    );
    assert_eq!(w.kernel.store().last_position(), before, "nothing written");
    w.kernel
        .set_wake(
            &g,
            &new_id("act"),
            now + 5 * MIN,
            "series",
            None,
            Some(series("5m", now + 5 * MIN)),
        )
        .unwrap();
    for i in 1..MAX_PENDING as u64 {
        w.kernel
            .set_wake(&g, &new_id("act"), now + i * DAY, "one-shot", None, None)
            .unwrap();
    }
    w.kernel
        .end_turn(g, TurnEnd::Wait { wake: Wake::Input })
        .unwrap();
    for _ in 0..3 {
        w.clock.advance(5 * MIN);
        fire(&w, &e.id);
        assert_eq!(exec(&w, &e.id).wakes.len(), MAX_PENDING);
    }
    w.kernel.wake_input(&e.id).unwrap();
    let g = w.kernel.admit(&e.id).unwrap();
    let err = w
        .kernel
        .set_wake(&g, &new_id("act"), now + DAY / 2, "a sixth", None, None)
        .unwrap_err();
    assert!(matches!(
        err.downcast_ref::<KernelError>(),
        Some(KernelError::TooManyWakes { .. })
    ));
    drop(g);
}

/// The series runs on the kernel's zone: a daily wake at 21:00 in New York
/// is put back at 21:00 across the spring change (23 hours later) and the
/// autumn one (25), and in Phoenix 24 hours later both times.
#[test]
fn the_take_puts_a_daily_wake_back_at_its_wall_time_in_the_kernels_zone() {
    for (zone, first, hours) in [
        (new_york(), "2026-03-07T21:00:00-05:00", 23),
        (new_york(), "2026-10-31T21:00:00-04:00", 25),
        (phoenix(), "2026-03-07T21:00:00-07:00", 24),
        (phoenix(), "2026-10-31T21:00:00-07:00", 24),
    ] {
        let w = world_with(KernelConfig {
            zone: zone.clone(),
            ..KernelConfig::default()
        });
        let first = ms(first);
        w.clock.set(first - HOUR);
        let (e, _) = with_series(&w, HOUR, |f| series("1d", f));
        assert_eq!(e.wakes[0].due_at_ms, first);
        w.clock.set(first);
        let f = &fire(&w, &e.id)[0];
        let next = f.next_due_at_ms.unwrap();
        assert_eq!((next - first) / HOUR, hours, "{}", wall(&zone, next));
        assert!(wall(&zone, next).ends_with("T21:00:00"));
        assert_eq!(exec(&w, &e.id).wakes[0].due_at_ms, next);
    }
}
