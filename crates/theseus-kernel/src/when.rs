//! A time as a person says it, in the daemon's zone (theseus-8ux6x): `tomorrow
//! 9:00`, `fri 9:00`, `17:30`, `today 5pm`, `2026-10-14 09:00`. The kernel
//! reads no clock, so `now_ms` is given; the zone is `KernelConfig::zone`.
//! Durations (`10m`) and RFC 3339 times are the core's (`wake::parse_after`,
//! `parse_at`), which wrap this.
//!
//! - A bare time is the next one: today if it is still ahead, else tomorrow.
//! - A weekday is the next one, today if its time is still ahead; with no
//!   time it is 9:00.
//! - `tomorrow` and `today` with no time are 9:00.
//! - A date and a time are that wall time; a wall time a daylight-saving
//!   change skips lands after the gap (jiff's compatible rule).

use jiff::civil::{Date, DateTime, Time, Weekday};
use jiff::{tz::TimeZone, Timestamp};

/// The hour a day with no time means.
const DEFAULT_HOUR: i8 = 9;

/// The time `text` names, in Unix ms; `None` when it is not a calendar time
/// (the caller tries its other forms), `Some(Err)` when it is one that cannot
/// be (a time that does not exist).
pub fn parse_calendar(text: &str, now_ms: u64, zone: &TimeZone) -> Option<Result<u64, String>> {
    let t = text.trim().to_ascii_lowercase();
    let words: Vec<&str> = t.split_whitespace().collect();
    if words.is_empty() {
        return None;
    }
    let now = Timestamp::from_millisecond(i64::try_from(now_ms).ok()?)
        .ok()?
        .to_zoned(zone.clone());
    let today = now.date();
    let (day, rest): (Option<Day>, &[&str]) = match words[0] {
        "today" => (Some(Day::Date(today)), &words[1..]),
        "tomorrow" => (Some(Day::Date(today.tomorrow().ok()?)), &words[1..]),
        w => match weekday(w) {
            Some(wd) => (Some(Day::Weekday(wd)), &words[1..]),
            None => match date(w) {
                Some(d) => (Some(Day::Fixed(d)), &words[1..]),
                None => (None, &words[..]),
            },
        },
    };
    // The time: one word (`9:00`, `5pm`) or two (`9:00 am`).
    let time = match rest {
        [] => None,
        [one] => Some(clock(one, None)?),
        [one, ampm] => Some(clock(one, Some(ampm))?),
        _ => return None,
    };
    if day.is_none() && time.is_none() {
        return None;
    }
    let at = |d: Date, t: Time| -> Option<Result<u64, String>> {
        let z = DateTime::from_parts(d, t).to_zoned(zone.clone()).ok()?;
        Some(
            u64::try_from(z.timestamp().as_millisecond())
                .map_err(|_| "that time is before 1970".to_string()),
        )
    };
    let default = Time::new(DEFAULT_HOUR, 0, 0, 0).ok()?;
    match day {
        // A bare time: the next one.
        None => {
            let time = time?;
            let first = at(today, time)?;
            if first.as_ref().is_ok_and(|ms| *ms > now_ms) {
                return Some(first);
            }
            at(today.tomorrow().ok()?, time)
        }
        Some(Day::Date(d) | Day::Fixed(d)) => at(d, time.unwrap_or(default)),
        Some(Day::Weekday(wd)) => {
            let time = time.unwrap_or(default);
            // Today counts while its time is ahead; else the next such day.
            let mut d = today;
            for _ in 0..8 {
                if d.weekday() == wd {
                    match at(d, time)? {
                        Ok(ms) if ms > now_ms => return Some(Ok(ms)),
                        Err(e) => return Some(Err(e)),
                        _ => {}
                    }
                }
                d = d.tomorrow().ok()?;
            }
            None
        }
    }
}

enum Day {
    Date(Date),
    Fixed(Date),
    Weekday(Weekday),
}

fn weekday(w: &str) -> Option<Weekday> {
    Some(match w {
        "mon" | "monday" => Weekday::Monday,
        "tue" | "tues" | "tuesday" => Weekday::Tuesday,
        "wed" | "weds" | "wednesday" => Weekday::Wednesday,
        "thu" | "thur" | "thurs" | "thursday" => Weekday::Thursday,
        "fri" | "friday" => Weekday::Friday,
        "sat" | "saturday" => Weekday::Saturday,
        "sun" | "sunday" => Weekday::Sunday,
        _ => return None,
    })
}

/// `2026-10-14`.
fn date(w: &str) -> Option<Date> {
    let mut p = w.split('-');
    let (y, m, d) = (p.next()?, p.next()?, p.next()?);
    if p.next().is_some() || y.len() != 4 {
        return None;
    }
    Date::new(y.parse().ok()?, m.parse().ok()?, d.parse().ok()?).ok()
}

/// `9`, `9:00`, `17:30`, `9am`, `5:15pm`, with a separate `am` or `pm`.
fn clock(w: &str, ampm: Option<&str>) -> Option<Time> {
    let (body, suffix) = match w.strip_suffix("am") {
        Some(b) => (b, Some("am")),
        None => match w.strip_suffix("pm") {
            Some(b) => (b, Some("pm")),
            None => (w, ampm),
        },
    };
    let suffix = match (suffix, ampm) {
        (Some(s), _) => Some(s),
        (None, Some(a)) => Some(a),
        (None, None) => None,
    };
    if !matches!(suffix, None | Some("am") | Some("pm")) {
        return None;
    }
    let (h, m) = match body.split_once(':') {
        Some((h, m)) if m.len() == 2 => (h, m),
        Some(_) => return None,
        // A bare hour with neither a colon nor am or pm is a number, not a time.
        None if suffix.is_some() => (body, "0"),
        None => return None,
    };
    let mut h: i8 = h.parse().ok()?;
    let m: i8 = m.parse().ok()?;
    match suffix {
        Some("am") => {
            if !(1..=12).contains(&h) {
                return None;
            }
            h %= 12;
        }
        Some("pm") => {
            if !(1..=12).contains(&h) {
                return None;
            }
            h = h % 12 + 12;
        }
        _ => {}
    }
    Time::new(h, m, 0, 0).ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn zone() -> TimeZone {
        TimeZone::posix("MST7").unwrap()
    }

    /// 2026-10-12 (a Monday) 10:00 in MST7, as Unix ms.
    fn now() -> u64 {
        DateTime::new(2026, 10, 12, 10, 0, 0, 0)
            .unwrap()
            .to_zoned(zone())
            .unwrap()
            .timestamp()
            .as_millisecond() as u64
    }

    fn wall(ms: u64) -> String {
        let z = Timestamp::from_millisecond(ms as i64)
            .unwrap()
            .to_zoned(zone());
        z.strftime("%a %Y-%m-%d %H:%M").to_string()
    }

    fn at(text: &str) -> String {
        wall(parse_calendar(text, now(), &zone()).unwrap().unwrap())
    }

    #[test]
    fn a_day_and_a_time_name_that_wall_time() {
        assert_eq!(at("tomorrow 9:00"), "Tue 2026-10-13 09:00");
        assert_eq!(at("today 17:30"), "Mon 2026-10-12 17:30");
        assert_eq!(at("tomorrow 5pm"), "Tue 2026-10-13 17:00");
        assert_eq!(at("tomorrow 9:30 am"), "Tue 2026-10-13 09:30");
        assert_eq!(at("2026-12-24 08:15"), "Thu 2026-12-24 08:15");
        assert_eq!(at("tomorrow"), "Tue 2026-10-13 09:00");
    }

    #[test]
    fn a_weekday_is_the_next_one_and_today_counts_while_it_is_ahead() {
        assert_eq!(at("fri 9:00"), "Fri 2026-10-16 09:00");
        assert_eq!(at("friday 9am"), "Fri 2026-10-16 09:00");
        // Monday 10:00 now: Monday 17:00 is today, Monday 9:00 is next week.
        assert_eq!(at("mon 17:00"), "Mon 2026-10-12 17:00");
        assert_eq!(at("mon 9:00"), "Mon 2026-10-19 09:00");
        assert_eq!(at("sun"), "Sun 2026-10-18 09:00");
    }

    #[test]
    fn a_bare_time_is_the_next_one() {
        assert_eq!(at("17:30"), "Mon 2026-10-12 17:30");
        assert_eq!(at("9:00"), "Tue 2026-10-13 09:00");
        assert_eq!(at("5pm"), "Mon 2026-10-12 17:00");
    }

    #[test]
    fn what_is_not_a_calendar_time_is_left_to_the_caller() {
        for t in [
            "",
            "10m",
            "in 10m",
            "soon",
            "2026-10-14T09:00:00-07:00",
            "25:00",
            "9",
            "noon",
        ] {
            assert!(
                parse_calendar(t, now(), &zone()).is_none(),
                "`{t}` is not a calendar time"
            );
        }
    }
}
