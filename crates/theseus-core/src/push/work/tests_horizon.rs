//! The agenda's expansion of a series (theseus-nuhsz), without a core: a
//! weekday series and a cron one across a change of daylight saving time keep
//! their wall time; a window far ahead is jumped to with its numbers; the
//! series' end and a paused wake are respected; and a watch is one line, its
//! expiry.

use theseus_kernel::{Day, Every, PendingWake, Purpose, Repeat, TimeZone, WakeProbe, WakeRun};
use theseus_protocol::work::horizon::{AgendaParams, AGENDA_MAX};

use super::horizon::lines_for_tests;

/// 2026-03-02 08:00 PST, a Monday; DST began on the 8th.
const FIRST: u64 = 1_772_467_200_000;
/// 2026-03-20 00:00 UTC.
const END: u64 = 1_773_964_800_000;
const DAY: u64 = 86_400_000;

fn zone() -> TimeZone {
    TimeZone::posix("PST8PDT,M3.2.0,M11.1.0").unwrap()
}

fn series(cron: Option<&str>) -> PendingWake {
    PendingWake {
        id: "wak_00000a".into(),
        due_at_ms: FIRST,
        note: "Morning digest".into(),
        set_at_ms: FIRST - DAY,
        occurrence: 1,
        repeat: Some(Repeat {
            every: Every::parse("1d").unwrap(),
            first_ms: FIRST,
            days: Day::ALL[..5].to_vec(),
            until_ms: None,
            cron: cron.map(str::to_string),
        }),
        ..Default::default()
    }
}

fn window(from: u64, to: u64) -> AgendaParams {
    AgendaParams {
        from_ms: from,
        to_ms: to,
        roots: None,
    }
}

fn expand_for_tests(w: &PendingWake, p: &AgendaParams, z: &TimeZone) -> Vec<(u64, u32)> {
    lines_for_tests(w, p, z, None).0
}

fn wall(ms: u64) -> String {
    theseus_kernel::cron::local(&zone(), ms, "%a %H:%M %Z")
}

/// A daily 08:00 on weekdays is 08:00 before and after the change: the UTC
/// time moves an hour at the 8th, the wall time does not.
#[test]
fn a_weekday_series_keeps_its_wall_time_across_a_dst_change() {
    let got = expand_for_tests(&series(None), &window(FIRST, END), &zone());
    assert_eq!(
        got.len(),
        14,
        "three weeks less the weekends and the 20th: {got:?}"
    );
    for (at, _) in &got {
        let w = wall(*at);
        assert!(!w.starts_with("Sat") && !w.starts_with("Sun"), "{w}");
        assert!(w.contains("08:00"), "{w}");
    }
    // Friday before the change at 16:00 UTC; Monday after it at 15:00 UTC.
    let fri = got.iter().find(|(_, n)| *n == 5).unwrap().0;
    let mon = got.iter().find(|(_, n)| *n == 6).unwrap().0;
    assert_eq!(fri, 1_772_812_800_000);
    assert_eq!(mon, 1_773_068_400_000);
    assert_eq!(
        mon - fri,
        3 * DAY - 3_600_000,
        "an hour shorter, by the calendar"
    );
}

/// A cron series (`0 8 * * 1-5`) does the same.
#[test]
fn a_cron_series_keeps_its_wall_time_across_a_dst_change() {
    let mut w = series(Some("0 8 * * 1-5"));
    w.repeat.as_mut().unwrap().days.clear();
    let got = expand_for_tests(&w, &window(FIRST, END), &zone());
    assert_eq!(got.len(), 14, "{got:?}");
    assert!(
        got.iter().all(|(at, _)| wall(*at).contains("08:00")),
        "{got:?}"
    );
    assert_eq!(
        got.iter().find(|(_, n)| *n == 6).unwrap().0,
        1_773_068_400_000
    );
}

/// A window far ahead is jumped to, and the numbers are the series' own.
#[test]
fn a_window_far_ahead_carries_the_numbers_on() {
    let all = expand_for_tests(&series(None), &window(FIRST, FIRST + 60 * DAY), &zone());
    let from = FIRST + 28 * DAY;
    let later = expand_for_tests(&series(None), &window(from, from + 7 * DAY), &zone());
    assert!(!later.is_empty());
    for (at, n) in &later {
        assert!(
            all.contains(&(*at, *n)),
            "{at} {n}: the same occurrence as walking up to it"
        );
    }
}

/// `until` ends the series; a minute series is cut at the cap, which says so.
#[test]
fn the_end_of_a_series_and_the_cap_hold() {
    let mut w = series(None);
    w.repeat.as_mut().unwrap().until_ms = Some(FIRST + 7 * DAY);
    let got = expand_for_tests(&w, &window(FIRST, FIRST + 60 * DAY), &zone());
    assert_eq!(got.len(), 6, "Monday to Monday: {got:?}");
    let mut m = series(None);
    {
        let r = m.repeat.as_mut().unwrap();
        r.every = Every::parse("1m").unwrap();
        r.days.clear();
    }
    let (lines, more) = lines_for_tests(&m, &window(FIRST, FIRST + 2 * DAY), &zone(), None);
    assert_eq!(lines.len(), AGENDA_MAX);
    assert!(more);
}

/// A watch is not a series of events: its one line is when it gives up.
#[test]
fn a_watch_is_one_line_its_expiry() {
    let probe = WakeProbe {
        kind: "file".into(),
        subject: "{\"path\":\"/tmp/x\"}".into(),
        until: "exists".into(),
    };
    let w = PendingWake {
        id: "wak_00000b".into(),
        due_at_ms: FIRST + 3 * DAY,
        note: "x exists?".into(),
        purpose: Purpose::Watch,
        run: WakeRun::Probe(probe),
        repeat: Some(Repeat {
            every: Every::parse("5m").unwrap(),
            first_ms: FIRST,
            days: vec![],
            until_ms: Some(FIRST + 3 * DAY),
            cron: None,
        }),
        ..Default::default()
    };
    let got = expand_for_tests(&w, &window(FIRST, FIRST + 30 * DAY), &zone());
    assert_eq!(got, [(FIRST + 3 * DAY, 0)]);
}
