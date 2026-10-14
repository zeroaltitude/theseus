//! Schedules for routines (theseus-pvm5c): cron and the plain words, in the
//! daemon's zone. The property tests hold cron to `Repeat::at` and
//! `Repeat::next_after` across the daylight-saving changes of four zones (given
//! as POSIX TZ strings, so no tz database is read), with a fixed seed.

use jiff::tz::TimeZone;

use crate::cron::{describe, parse_schedule, Cron};
use crate::repeat::{Day, Every, Repeat};

const MIN: u64 = 60_000;
const HOUR: u64 = 60 * MIN;
const DAY: u64 = 24 * HOUR;
/// 2026-01-05T00:00:00Z, a Monday.
const JAN_5: u64 = 1_767_571_200_000;

fn zones() -> Vec<(&'static str, TimeZone)> {
    vec![
        ("UTC", TimeZone::UTC),
        (
            "New York",
            TimeZone::posix("EST5EDT,M3.2.0,M11.1.0").unwrap(),
        ),
        (
            "Sydney",
            TimeZone::posix("AEST-10AEDT,M10.1.0,M4.1.0/3").unwrap(),
        ),
        (
            "Lord Howe (a half hour change)",
            TimeZone::posix("<+1030>-10:30<+11>-11,M10.1.0,M4.1.0").unwrap(),
        ),
    ]
}

/// xorshift64*: a fixed seed, so a failure is the same failure.
struct Rng(u64);
impl Rng {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 >> 12;
        self.0 ^= self.0 << 25;
        self.0 ^= self.0 >> 27;
        self.0.wrapping_mul(0x2545_F491_4F6C_DD1D)
    }
    fn below(&mut self, n: u64) -> u64 {
        self.next() % n
    }
}

fn wall(zone: &TimeZone, ms: u64) -> String {
    jiff::Timestamp::from_millisecond(ms as i64)
        .unwrap()
        .to_zoned(zone.clone())
        .datetime()
        .to_string()
}

#[test]
fn a_daily_cron_is_a_daily_series_across_every_change_of_offset() {
    let mut rng = Rng(0x5eed_0001);
    for (name, zone) in zones() {
        for _ in 0..60 {
            let (h, m) = (rng.below(24), rng.below(60));
            let cron = Cron::parse(&format!("{m} {h} * * *")).unwrap();
            let start = JAN_5 + rng.below(20 * DAY);
            let first = cron.next_after(&zone, start).unwrap();
            let series = Repeat {
                every: Every::parse("1d").unwrap(),
                first_ms: first,
                days: vec![],
                until_ms: None,
                cron: None,
            };
            let mut t = first - 1;
            for k in 0..400 {
                t = cron.next_after(&zone, t).unwrap();
                assert_eq!(
                    Some(t),
                    series.at(&zone, k),
                    "{name}: `{m} {h} * * *`, occurrence {k}: cron {} against the series {:?}",
                    wall(&zone, t),
                    series.at(&zone, k).map(|a| wall(&zone, a))
                );
            }
        }
    }
}

#[test]
fn a_weekday_cron_is_a_daily_series_limited_to_weekdays() {
    let mut rng = Rng(0x5eed_0002);
    for (name, zone) in zones() {
        for _ in 0..40 {
            let (h, m) = (rng.below(24), rng.below(60));
            let cron = Cron::parse(&format!("{m} {h} * * 1-5")).unwrap();
            let first = cron.next_after(&zone, JAN_5 + rng.below(9 * DAY)).unwrap();
            let series = Repeat {
                every: Every::parse("1d").unwrap(),
                first_ms: first,
                days: Day::ALL[..5].to_vec(),
                until_ms: None,
                cron: None,
            };
            let mut t = first - 1;
            for k in 0..300 {
                let want = cron.next_after(&zone, t).unwrap();
                assert_eq!(
                    series.next_after(&zone, t),
                    Some(want),
                    "{name}: `{m} {h} * * 1-5`, step {k} after {}",
                    wall(&zone, t)
                );
                t = want;
            }
        }
    }
}

#[test]
fn a_monthly_cron_is_a_month_span() {
    let mut rng = Rng(0x5eed_0003);
    for (name, zone) in zones() {
        for _ in 0..40 {
            let (h, m, d) = (rng.below(24), rng.below(60), 1 + rng.below(28));
            let cron = Cron::parse(&format!("{m} {h} {d} * *")).unwrap();
            let first = cron.next_after(&zone, JAN_5 + rng.below(30 * DAY)).unwrap();
            let series = Repeat {
                every: Every::parse("1mo").unwrap(),
                first_ms: first,
                days: vec![],
                until_ms: None,
                cron: None,
            };
            let mut t = first - 1;
            for k in 0..30 {
                t = cron.next_after(&zone, t).unwrap();
                assert_eq!(
                    series.at(&zone, k),
                    Some(t),
                    "{name}: `{m} {h} {d} * *`, month {k}: cron {}",
                    wall(&zone, t)
                );
            }
        }
    }
}

#[test]
fn a_cron_series_answers_for_next_after_between_and_until() {
    let zone = TimeZone::UTC;
    let r = |until_ms| Repeat {
        every: Every::parse("1d").unwrap(),
        first_ms: JAN_5 + 8 * HOUR,
        days: vec![],
        until_ms,
        cron: Some("0 8 * * 1-5".into()),
    };
    let rule = r(None);
    assert_eq!(rule.first(&zone), Some(JAN_5 + 8 * HOUR));
    // Friday's 08:00 is followed by Monday's.
    let fri = JAN_5 + 4 * DAY + 8 * HOUR;
    assert_eq!(rule.next_after(&zone, fri), Some(fri + 3 * DAY));
    assert_eq!(rule.at(&zone, 5), Some(JAN_5 + 7 * DAY + 8 * HOUR));
    // The weekdays between a Monday and the Friday after: four more.
    assert_eq!(rule.between(&zone, JAN_5 + 8 * HOUR, fri), 4);
    // `until` ends it: nothing after Wednesday.
    let wed = JAN_5 + 2 * DAY + 8 * HOUR;
    let ended = r(Some(wed));
    assert_eq!(
        ended.next_after(&zone, JAN_5 + 8 * HOUR),
        Some(JAN_5 + DAY + 8 * HOUR)
    );
    assert_eq!(ended.next_after(&zone, wed), None);
}

#[test]
fn cron_fields_are_read_as_cron_reads_them() {
    let zone = TimeZone::UTC;
    let next = |expr: &str, after: u64| Cron::parse(expr).unwrap().next_after(&zone, after);
    // A step, a list, a range with a step.
    assert_eq!(next("*/15 * * * *", JAN_5), Some(JAN_5 + 15 * MIN));
    assert_eq!(
        next("5,35 9 * * *", JAN_5),
        Some(JAN_5 + 9 * HOUR + 5 * MIN)
    );
    assert_eq!(next("0 0-6/3 * * *", JAN_5), Some(JAN_5 + 3 * HOUR));
    // Both day fields restricted: either matches. The 7th is a Wednesday here;
    // the next Monday (the 12th) comes first, then the 7th... in order.
    let either = |t| next("0 0 7 * 1", t).unwrap();
    assert_eq!(either(JAN_5), JAN_5 + 2 * DAY);
    assert_eq!(either(JAN_5 + 2 * DAY), JAN_5 + 7 * DAY);
    // Sunday is 0 and 7.
    assert_eq!(next("0 0 * * 7", JAN_5), next("0 0 * * 0", JAN_5));
    // The 29th of February waits for a leap year.
    assert_eq!(
        wall(&zone, next("0 0 29 2 *", JAN_5).unwrap()),
        "2028-02-29T00:00:00"
    );
    for bad in [
        "* * * *",
        "61 * * * *",
        "* 24 * * *",
        "*/0 * * * *",
        "a * * * *",
        "5-1 * * * *",
    ] {
        assert!(Cron::parse(bad).is_err(), "{bad}");
    }
}

#[test]
fn the_words_people_write_become_series() {
    let zone = TimeZone::posix("MST7").unwrap();
    // Monday 2026-01-05 10:00 in Phoenix (UTC-7).
    let now = JAN_5 + 17 * HOUR;
    let parse = |s: &str| parse_schedule(s, &zone, now, 5 * MIN).unwrap();
    let w = parse("weekdays 08:00");
    assert_eq!(w.repeat.days.len(), 5);
    assert_eq!(w.first_ms, JAN_5 + DAY + 15 * HOUR, "tomorrow 08:00 MST");
    assert_eq!(describe(&w.repeat, &zone), "weekdays 08:00");
    // Today's 07:30 is past; 11:30 is not.
    assert_eq!(
        parse("daily 7:30am").first_ms,
        JAN_5 + DAY + 14 * HOUR + 30 * MIN
    );
    assert_eq!(
        parse("every day at 11:30").first_ms,
        JAN_5 + 18 * HOUR + 30 * MIN
    );
    assert_eq!(parse("mon,thu 09:00").repeat.days, vec![Day::Mon, Day::Thu]);
    assert_eq!(
        parse("mon-wed 6pm").repeat.days,
        vec![Day::Mon, Day::Tue, Day::Wed]
    );
    let e = parse("every 30m");
    assert_eq!(
        (e.repeat.every, e.first_ms),
        (Every::parse("30m").unwrap(), now + 30 * MIN)
    );
    let month = parse("monthly 09:00");
    assert_eq!(month.repeat.every, Every::parse("1mo").unwrap());
    assert_eq!(wall(&zone, month.first_ms), "2026-02-01T09:00:00");
    assert_eq!(describe(&month.repeat, &zone), "monthly the 1st 09:00");
    assert_eq!(
        wall(&zone, parse("the 15th at 18:00").first_ms),
        "2026-01-15T18:00:00"
    );
    assert_eq!(parse("1mo").repeat.every.to_string(), "1mo");
    let c = parse("0 8 * * 1-5");
    assert_eq!(c.repeat.cron.as_deref(), Some("0 8 * * 1-5"));
    assert_eq!(c.first_ms, JAN_5 + DAY + 15 * HOUR);
    assert_eq!(
        c.repeat.every,
        Every::parse("1d").unwrap(),
        "its shortest gap"
    );
    // What does not read, and what is too often.
    for bad in [
        "",
        "sometimes",
        "weekdays",
        "0 8 * *",
        "every 1m",
        "* * * * *",
    ] {
        assert!(parse_schedule(bad, &zone, now, 5 * MIN).is_err(), "{bad:?}");
    }
    assert!(parse_schedule("every 1m", &zone, now, MIN).is_ok());
}
