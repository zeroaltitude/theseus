//! Schedules for routines (theseus-pvm5c; the horizon report's §5.4): a
//! 5-field cron expression, and the plain words people write for a series,
//! both read in the daemon's zone over jiff, with no new crate.
//!
//! - **`Cron`**: `minute hour day-of-month month day-of-week`, each field
//!   `*`, a number, a range `a-b`, a list `a,b-c`, and a step `*/n` or
//!   `a-b/n`. Day of week is 0 to 7 (0 and 7 are Sunday); with both the day of
//!   month and the day of week restricted a day matches either, as Vixie cron
//!   has it. The next time is by civil date and wall time, resolved in the
//!   zone by jiff's compatible rule, so `0 8 * * *` stays 08:00 across a change
//!   of offset exactly as `Repeat::at` holds a daily series (the property tests
//!   hold the two together); a wall time a change skips lands after the gap.
//! - **`parse_schedule`**: what a person writes: `weekdays 08:00`, `daily 7:30am`,
//!   `mon,wed 09:00`, `every 30m`, `monthly 09:00`, `the 1st 09:00`, `1mo`, or a
//!   cron expression. It gives the `Repeat` the wake carries and the first time.
//! - **`describe`**: the series in the same words, for lists and the approval
//!   card.

use jiff::civil::{Date, Weekday};
use jiff::tz::TimeZone;
use jiff::{Span, Timestamp};

use crate::repeat::{Day, Every, Repeat, Unit};

/// How far ahead `next_after` looks for a date that matches (a 29 February).
const HORIZON_DAYS: i32 = 5 * 366;

/// A parsed cron expression.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Cron {
    minutes: u64,
    hours: u32,
    /// Bit `d` for the day of the month `d` (1 to 31).
    dom: u32,
    /// Bit `m` for the month `m` (1 to 12).
    months: u16,
    /// Bit `d` for the weekday `d`, 0 Sunday.
    dow: u8,
    dom_star: bool,
    dow_star: bool,
}

/// One field: its allowed numbers as a set of bits, and whether it was `*`.
fn field(text: &str, lo: u32, hi: u32, name: &str) -> Result<(u64, bool), String> {
    let bad = |why: &str| format!("cron {name} `{text}`: {why}");
    let mut bits = 0u64;
    let star = text == "*";
    for part in text.split(',') {
        let (range, step) = match part.split_once('/') {
            Some((r, s)) => (
                r,
                s.parse::<u32>()
                    .ok()
                    .filter(|n| *n > 0)
                    .ok_or_else(|| bad("a step is a whole number from 1"))?,
            ),
            None => (part, 1),
        };
        let (a, b) = if range == "*" {
            (lo, hi)
        } else if let Some((a, b)) = range.split_once('-') {
            (
                a.parse::<u32>().map_err(|_| bad("not a number"))?,
                b.parse::<u32>().map_err(|_| bad("not a number"))?,
            )
        } else {
            let n = range.parse::<u32>().map_err(|_| bad("not a number"))?;
            // `n/step` runs from n to the end, as cron has it.
            (n, if part.contains('/') { hi } else { n })
        };
        if a < lo || b > hi || a > b {
            return Err(bad(&format!("must lie within {lo} to {hi}")));
        }
        let mut n = a;
        while n <= b {
            bits |= 1 << n;
            n += step;
        }
    }
    Ok((bits, star))
}

impl Cron {
    /// `0 8 * * 1-5`: five fields.
    pub fn parse(text: &str) -> Result<Cron, String> {
        let f: Vec<&str> = text.split_whitespace().collect();
        let [mi, h, d, mo, w] = f[..] else {
            return Err(format!(
                "a cron expression has five fields (minute hour day-of-month month \
                 day-of-week), not {}: `{text}`",
                f.len()
            ));
        };
        let (minutes, _) = field(mi, 0, 59, "minute")?;
        let (hours, _) = field(h, 0, 23, "hour")?;
        let (dom, dom_star) = field(d, 1, 31, "day-of-month")?;
        let (months, _) = field(mo, 1, 12, "month")?;
        let (mut dow, dow_star) = field(w, 0, 7, "day-of-week")?;
        if dow & (1 << 7) != 0 {
            dow = (dow | 1) & !(1 << 7);
        }
        Ok(Cron {
            minutes,
            hours: hours as u32,
            dom: dom as u32,
            months: months as u16,
            dow: dow as u8,
            dom_star,
            dow_star,
        })
    }

    fn day_matches(&self, d: Date) -> bool {
        if self.months & (1 << d.month() as u16) == 0 {
            return false;
        }
        let by_dom = self.dom & (1 << d.day() as u32) != 0;
        let wd = match d.weekday() {
            Weekday::Sunday => 0,
            Weekday::Monday => 1,
            Weekday::Tuesday => 2,
            Weekday::Wednesday => 3,
            Weekday::Thursday => 4,
            Weekday::Friday => 5,
            Weekday::Saturday => 6,
        };
        let by_dow = self.dow & (1 << wd) != 0;
        match (self.dom_star, self.dow_star) {
            (false, false) => by_dom || by_dow,
            (false, true) => by_dom,
            (true, false) => by_dow,
            (true, true) => true,
        }
    }

    /// The first time after `after_ms` the expression names, in `zone`;
    /// none within five years.
    pub fn next_after(&self, zone: &TimeZone, after_ms: u64) -> Option<u64> {
        let after = Timestamp::from_millisecond(i64::try_from(after_ms).ok()?).ok()?;
        let mut date = after.to_zoned(zone.clone()).date();
        for _ in 0..HORIZON_DAYS {
            if self.day_matches(date) {
                for h in (0..24).filter(|h| self.hours & (1 << h) != 0) {
                    for m in (0..60).filter(|m| self.minutes & (1 << m) != 0) {
                        let at = date.at(h as i8, m as i8, 0, 0).to_zoned(zone.clone());
                        let ms = at.ok()?.timestamp().as_millisecond();
                        if ms > after.as_millisecond() {
                            return u64::try_from(ms).ok();
                        }
                    }
                }
            }
            date = date.tomorrow().ok()?;
        }
        None
    }
}

/// A series from words or cron, and its first time.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Parsed {
    pub repeat: Repeat,
    /// The first time it is due: after `now`.
    pub first_ms: u64,
}

/// `HH:MM`, `H:MM`, `7am`, `7:30pm`: hour and minute.
fn clock(s: &str) -> Option<(i8, i8)> {
    let l = s.trim().to_ascii_lowercase();
    let (body, pm) = match (l.strip_suffix("am"), l.strip_suffix("pm")) {
        (Some(b), _) => (b.trim(), Some(false)),
        (_, Some(b)) => (b.trim(), Some(true)),
        _ => (l.as_str(), None),
    };
    let (h, m) = match body.split_once(':') {
        Some((h, m)) => (h.parse::<i8>().ok()?, m.parse::<i8>().ok()?),
        None if pm.is_some() => (body.parse::<i8>().ok()?, 0),
        None => return None,
    };
    let h = match pm {
        Some(true) if (1..=12).contains(&h) => h % 12 + 12,
        Some(false) if (1..=12).contains(&h) => h % 12,
        Some(_) => return None,
        None => h,
    };
    ((0..24).contains(&h) && (0..60).contains(&m)).then_some((h, m))
}

/// `mon,wed`, `mon-fri`, `weekdays`, `weekends`, `daily`: the days, empty for
/// every day.
fn days_of(s: &str) -> Option<Vec<Day>> {
    let l = s.trim().to_ascii_lowercase();
    match l.as_str() {
        "daily" | "day" | "days" | "everyday" => return Some(vec![]),
        "weekdays" | "weekday" => return Some(Day::ALL[..5].to_vec()),
        "weekends" | "weekend" => return Some(Day::ALL[5..].to_vec()),
        _ => {}
    }
    let mut out: Vec<Day> = Vec::new();
    for part in l.split(',') {
        if let Some((a, b)) = part.split_once('-') {
            let (a, b) = (Day::parse(a)?, Day::parse(b)?);
            let (i, j) = (
                Day::ALL.iter().position(|d| *d == a)?,
                Day::ALL.iter().position(|d| *d == b)?,
            );
            let span: Vec<Day> = if i <= j {
                Day::ALL[i..=j].to_vec()
            } else {
                Day::ALL[i..]
                    .iter()
                    .chain(&Day::ALL[..=j])
                    .copied()
                    .collect()
            };
            out.extend(span);
        } else {
            out.push(Day::parse(part)?);
        }
    }
    out.sort();
    out.dedup();
    Some(out)
}

/// `1st`, `the 15th`, `1`: a day of the month.
fn month_day(s: &str) -> Option<i8> {
    let l = s.trim().to_ascii_lowercase();
    let l = l.strip_prefix("the ").unwrap_or(&l).trim();
    let digits: String = l.chars().take_while(char::is_ascii_digit).collect();
    let rest = &l[digits.len()..];
    if !matches!(rest, "" | "st" | "nd" | "rd" | "th") {
        return None;
    }
    digits.parse::<i8>().ok().filter(|d| (1..=31).contains(d))
}

fn bad_schedule(t: &str) -> String {
    format!(
        "cannot read the schedule `{t}`: write `weekdays 08:00`, `daily 7:30am`, `mon,thu \
         09:00`, `every 30m`, `monthly 09:00`, `the 1st 09:00`, or a cron expression such as \
         `0 8 * * 1-5`"
    )
}

fn plain(every: Every, first_ms: u64, days: Vec<Day>) -> Repeat {
    Repeat {
        every,
        first_ms,
        days,
        until_ms: None,
        cron: None,
    }
}

fn ms_of(z: &jiff::Zoned) -> Result<u64, String> {
    u64::try_from(z.timestamp().as_millisecond()).map_err(|e| e.to_string())
}

/// Read a schedule: see the module's doc for the forms. `now_ms` is the clock
/// the first time is after; `floor_ms` the shortest span a series may have.
pub fn parse_schedule(
    text: &str,
    zone: &TimeZone,
    now_ms: u64,
    floor_ms: u64,
) -> Result<Parsed, String> {
    let t = text.trim();
    let bad = || bad_schedule(t);
    let now = i64::try_from(now_ms)
        .ok()
        .and_then(|m| Timestamp::from_millisecond(m).ok())
        .filter(|_| !t.is_empty())
        .ok_or_else(bad)?;
    let words: Vec<&str> = t.split_whitespace().collect();
    if words.len() == 5
        && words
            .iter()
            .all(|w| w.chars().all(|c| c.is_ascii_digit() || "*/,-".contains(c)))
    {
        return cron_schedule(&words, zone, now_ms, floor_ms);
    }
    let lower = t.to_ascii_lowercase();
    let rest = lower.strip_prefix("every ").unwrap_or(&lower).trim();
    if let Ok(every) = Every::parse(rest) {
        let r = plain(every, span_first(every, zone, now).ok_or_else(bad)?, vec![]);
        floor_check(&r, floor_ms)?;
        return Ok(Parsed {
            first_ms: r.first_ms,
            repeat: r,
        });
    }
    let words: Vec<&str> = rest.split_whitespace().collect();
    let monthly = words
        .first()
        .is_some_and(|w| matches!(*w, "monthly" | "month" | "the") || month_day(w).is_some());
    if monthly {
        return monthly_schedule(&words, zone, now).ok_or_else(bad);
    }
    weekly_schedule(&words, zone, now).ok_or_else(bad)
}

/// `0 8 * * 1-5`: a cron series, its `every` its shortest gap.
fn cron_schedule(
    words: &[&str],
    zone: &TimeZone,
    now_ms: u64,
    floor_ms: u64,
) -> Result<Parsed, String> {
    let text = words.join(" ");
    let c = Cron::parse(&text)?;
    let first_ms = c
        .next_after(zone, now_ms)
        .ok_or_else(|| bad_schedule(&text))?;
    // Its shortest gap over the next dozen times.
    let mut gap = u64::MAX;
    let mut prev = first_ms;
    for _ in 0..12 {
        let Some(next) = c.next_after(zone, prev) else {
            break;
        };
        gap = gap.min(next - prev);
        prev = next;
    }
    let every = match gap {
        u64::MAX => Every {
            n: 1,
            unit: Unit::Days,
        },
        g if g % 86_400_000 == 0 => Every {
            n: (g / 86_400_000) as u32,
            unit: Unit::Days,
        },
        g if g % 3_600_000 == 0 => Every {
            n: (g / 3_600_000) as u32,
            unit: Unit::Hours,
        },
        g => Every {
            n: (g / 60_000).max(1) as u32,
            unit: Unit::Minutes,
        },
    };
    let mut r = plain(every, first_ms, vec![]);
    r.cron = Some(text);
    floor_check(&r, floor_ms)?;
    Ok(Parsed {
        repeat: r,
        first_ms,
    })
}

/// One span after now.
fn span_first(every: Every, zone: &TimeZone, now: Timestamp) -> Option<u64> {
    let n = i64::from(every.n);
    let span = match every.unit {
        Unit::Months => Span::new().months(n),
        Unit::Weeks => Span::new().weeks(n),
        Unit::Days => Span::new().days(n),
        Unit::Hours => Span::new().hours(n),
        Unit::Minutes => Span::new().minutes(n),
    };
    ms_of(&now.to_zoned(zone.clone()).checked_add(span).ok()?).ok()
}

/// `monthly 09:00`, `monthly on the 1st at 09:00`, `the 1st 09:00`.
fn monthly_schedule(words: &[&str], zone: &TimeZone, now: Timestamp) -> Option<Parsed> {
    let mut day: Option<i8> = None;
    let mut at: Option<(i8, i8)> = None;
    for w in words {
        if matches!(
            *w,
            "monthly" | "month" | "on" | "at" | "of" | "each" | "every" | "the"
        ) {
            continue;
        }
        match clock(w) {
            Some(c) => at = Some(c),
            None => day = Some(month_day(w)?),
        }
    }
    let (h, m) = at.unwrap_or((9, 0));
    let day = day.unwrap_or(1);
    let today = now.to_zoned(zone.clone()).date();
    // The next date with that day of the month (a short month clamps it).
    let first = (0..=13).find_map(|add| {
        let d = today
            .first_of_month()
            .checked_add(Span::new().months(add))
            .ok()?;
        let z = d
            .with()
            .day(day.min(d.days_in_month()))
            .build()
            .ok()?
            .at(h, m, 0, 0)
            .to_zoned(zone.clone())
            .ok()?;
        (z.timestamp() > now).then_some(z)
    })?;
    let first_ms = ms_of(&first).ok()?;
    let r = plain(
        Every {
            n: 1,
            unit: Unit::Months,
        },
        first_ms,
        vec![],
    );
    Some(Parsed {
        repeat: r,
        first_ms,
    })
}

/// `weekdays 08:00`, `daily 7:30am`, `mon,wed at 09:00`.
fn weekly_schedule(words: &[&str], zone: &TimeZone, now: Timestamp) -> Option<Parsed> {
    let words: Vec<&str> = words
        .iter()
        .copied()
        .filter(|w| !matches!(*w, "at" | "on" | "every" | "each"))
        .collect();
    let [d, c] = words[..] else {
        return None;
    };
    let (days, (h, m)) = (days_of(d)?, clock(c)?);
    let today = now.to_zoned(zone.clone()).date();
    let at = |date: jiff::civil::Date| date.at(h, m, 0, 0).to_zoned(zone.clone()).ok();
    let first = Some(at(today)?)
        .filter(|z| z.timestamp() > now)
        .or_else(|| at(today.tomorrow().ok()?))?;
    let r = plain(
        Every {
            n: 1,
            unit: Unit::Days,
        },
        ms_of(&first).ok()?,
        days,
    );
    let first_ms = r.first(zone)?;
    Some(Parsed {
        repeat: r,
        first_ms,
    })
}

fn floor_check(r: &Repeat, floor_ms: u64) -> Result<(), String> {
    if r.every.nominal_ms() < floor_ms {
        return Err(format!(
            "a routine may repeat every {} minutes at the most often, not every {}",
            floor_ms / 60_000,
            r.every
        ));
    }
    Ok(())
}

/// The series in words: `weekdays 08:00`, `daily 07:30`, `every 30m`,
/// `monthly the 1st 09:00`, `cron 0 8 * * 1-5`.
pub fn describe(r: &Repeat, zone: &TimeZone) -> String {
    if let Some(c) = &r.cron {
        return format!("cron {c}");
    }
    let wall = |ms: u64| {
        Timestamp::from_millisecond(i64::try_from(ms).unwrap_or(0))
            .map(|t| t.to_zoned(zone.clone()))
            .ok()
    };
    let at = wall(r.first_ms);
    let hm = at
        .as_ref()
        .map(|z| format!("{:02}:{:02}", z.hour(), z.minute()))
        .unwrap_or_default();
    match r.every.unit {
        Unit::Months => {
            let d = at.as_ref().map_or(1, |z| z.day());
            format!("monthly the {d}{} {hm}", ordinal(d))
        }
        Unit::Days if r.every.n == 1 => {
            let label = match r.days[..] {
                [] => "daily".to_string(),
                ref d if *d == Day::ALL[..5] => "weekdays".to_string(),
                ref d if *d == Day::ALL[5..] => "weekends".to_string(),
                ref d => d.iter().map(|d| d.as_str()).collect::<Vec<_>>().join(","),
            };
            format!("{label} {hm}")
        }
        _ => format!("every {}", r.every),
    }
}

fn ordinal(d: i8) -> &'static str {
    match (d % 10, d % 100) {
        (_, 11..=13) => "th",
        (1, _) => "st",
        (2, _) => "nd",
        (3, _) => "rd",
        _ => "th",
    }
}

/// A time in `zone` written by a strftime `fmt`: `local(zone, ms, "%a %H:%M")`.
pub fn local(zone: &TimeZone, ms: u64, fmt: &str) -> String {
    i64::try_from(ms)
        .ok()
        .and_then(|m| Timestamp::from_millisecond(m).ok())
        .map(|t| t.to_zoned(zone.clone()).strftime(fmt).to_string())
        .unwrap_or_default()
}

/// The local month a time falls in: `2026-10`.
pub fn month_key(zone: &TimeZone, ms: u64) -> String {
    local(zone, ms, "%Y-%m")
}

/// `Fri 08:00`.
pub fn when(zone: &TimeZone, ms: u64) -> String {
    local(zone, ms, "%a %H:%M")
}

/// `08:00`.
pub fn hm(zone: &TimeZone, ms: u64) -> String {
    local(zone, ms, "%H:%M")
}

/// `08:00:00.3`: the time to a tenth of a second.
pub fn hms_tenths(zone: &TimeZone, ms: u64) -> String {
    format!("{}.{}", local(zone, ms, "%H:%M:%S"), (ms % 1000) / 100)
}
