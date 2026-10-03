//! A repeating wake's series (37a, theseus-d4pt; design m7-surface §2.2):
//! `wake.at { every, days, until }` keeps one pending wake whose next
//! occurrence the turn that takes it puts back on the list (`take_wakes`).
//!
//! - **The times.** Occurrence k (from 0) is due at `first + k·every`, in
//!   the daemon's zone (`KernelConfig::zone`), by jiff's zoned arithmetic: a
//!   span of minutes or hours is exact time, and one of days or weeks is a
//!   calendar span, so "daily 21:00" stays 21:00 across a daylight-saving
//!   change. A wall time a change skips lands after the gap, as jiff's
//!   compatible rule says.
//! - **The next.** The first occurrence after now that the days allow, and
//!   none after `until`: the series then ends.
//! - **Missed.** The occurrences that fell due between the one a turn takes
//!   and now (the daemon was down, or the session busy) are counted, never
//!   run: one turn, never a burst.

pub use jiff::tz::TimeZone;
use jiff::{civil::Weekday, Span, Timestamp};
use serde::{Deserialize, Serialize};

/// The unit of a repeat's span.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Unit {
    #[serde(rename = "m")]
    Minutes,
    #[serde(rename = "h")]
    Hours,
    #[serde(rename = "d")]
    Days,
    #[serde(rename = "w")]
    Weeks,
}

/// A repeat's span: `5m`, `2h`, `1d`, `1w`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Every {
    pub n: u32,
    pub unit: Unit,
}

impl Every {
    /// `Nm`, `Nh`, `Nd`, or `Nw`, N a whole number from 1.
    pub fn parse(s: &str) -> Result<Every, String> {
        let bad = || format!("`every` must be a span such as 30m, 2h, 1d, or 1w, not `{s}`");
        let t = s.trim();
        let (digits, unit) = t.split_at(t.find(|c: char| !c.is_ascii_digit()).ok_or_else(bad)?);
        let n: u32 = digits.parse().map_err(|_| bad())?;
        let unit = match unit.trim().to_ascii_lowercase().as_str() {
            "m" | "min" | "mins" | "minute" | "minutes" => Unit::Minutes,
            "h" | "hr" | "hrs" | "hour" | "hours" => Unit::Hours,
            "d" | "day" | "days" => Unit::Days,
            "w" | "wk" | "week" | "weeks" => Unit::Weeks,
            _ => return Err(bad()),
        };
        if n == 0 {
            return Err(bad());
        }
        Ok(Every { n, unit })
    }

    /// Its length on a day with no change of offset.
    pub fn nominal_ms(&self) -> u64 {
        let unit: u64 = match self.unit {
            Unit::Minutes => 60_000,
            Unit::Hours => 3_600_000,
            Unit::Days => 86_400_000,
            Unit::Weeks => 7 * 86_400_000,
        };
        u64::from(self.n) * unit
    }

    /// `k` of it, as a span jiff adds in a zone.
    fn times(&self, k: u64) -> Option<Span> {
        let n = i64::try_from(k.checked_mul(u64::from(self.n))?).ok()?;
        match self.unit {
            Unit::Minutes => Span::new().try_minutes(n),
            Unit::Hours => Span::new().try_hours(n),
            Unit::Days => Span::new().try_days(n),
            Unit::Weeks => Span::new().try_weeks(n),
        }
        .ok()
    }
}

impl std::fmt::Display for Every {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let u = match self.unit {
            Unit::Minutes => "m",
            Unit::Hours => "h",
            Unit::Days => "d",
            Unit::Weeks => "w",
        };
        write!(f, "{}{u}", self.n)
    }
}

/// A weekday a daily series may be limited to.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Day {
    Mon,
    Tue,
    Wed,
    Thu,
    Fri,
    Sat,
    Sun,
}

impl Day {
    pub const ALL: [Day; 7] = [
        Day::Mon,
        Day::Tue,
        Day::Wed,
        Day::Thu,
        Day::Fri,
        Day::Sat,
        Day::Sun,
    ];

    pub fn as_str(self) -> &'static str {
        match self {
            Day::Mon => "mon",
            Day::Tue => "tue",
            Day::Wed => "wed",
            Day::Thu => "thu",
            Day::Fri => "fri",
            Day::Sat => "sat",
            Day::Sun => "sun",
        }
    }

    /// `mon`, `Monday`, `TUE`: three letters of its name at least.
    pub fn parse(s: &str) -> Option<Day> {
        const NAMES: [&str; 7] = [
            "monday",
            "tuesday",
            "wednesday",
            "thursday",
            "friday",
            "saturday",
            "sunday",
        ];
        let l = s.trim().to_ascii_lowercase();
        if l.len() < 3 {
            return None;
        }
        NAMES
            .iter()
            .position(|n| n.starts_with(&l))
            .map(|i| Day::ALL[i])
    }

    fn of(w: Weekday) -> Day {
        match w {
            Weekday::Monday => Day::Mon,
            Weekday::Tuesday => Day::Tue,
            Weekday::Wednesday => Day::Wed,
            Weekday::Thursday => Day::Thu,
            Weekday::Friday => Day::Fri,
            Weekday::Saturday => Day::Sat,
            Weekday::Sunday => Day::Sun,
        }
    }
}

/// What makes a pending wake repeat (37a).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Repeat {
    pub every: Every,
    /// The first occurrence's due time: occurrence k is due at
    /// `first_ms + k·every` in the daemon's zone.
    pub first_ms: u64,
    /// The weekdays it runs on, in the zone; empty is every day.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub days: Vec<Day>,
    /// No occurrence after this time: the one before it ends the series.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub until_ms: Option<u64>,
}

/// The most occurrences in a row the days may pass over before the series
/// is taken as ended: two weeks of a daily one.
const MOST_SKIPPED: usize = 14;
/// The most occurrences `between` looks at one by one (with days): a year
/// of a daily series, and then it stops counting.
const MOST_COUNTED: u64 = 400;

impl Repeat {
    /// Occurrence `k`'s due time, or `None` past what a time can be.
    pub fn at(&self, zone: &TimeZone, k: u64) -> Option<u64> {
        let first = Timestamp::from_millisecond(i64::try_from(self.first_ms).ok()?).ok()?;
        let z = first
            .to_zoned(zone.clone())
            .checked_add(self.every.times(k)?)
            .ok()?;
        u64::try_from(z.timestamp().as_millisecond()).ok()
    }

    /// Whether the days allow a time.
    pub fn allows(&self, zone: &TimeZone, ms: u64) -> bool {
        if self.days.is_empty() {
            return true;
        }
        let Some(t) = i64::try_from(ms)
            .ok()
            .and_then(|m| Timestamp::from_millisecond(m).ok())
        else {
            return false;
        };
        self.days
            .contains(&Day::of(t.to_zoned(zone.clone()).weekday()))
    }

    /// The first occurrence index whose time is after `after_ms`.
    fn index_after(&self, zone: &TimeZone, after_ms: u64) -> Option<u64> {
        if after_ms < self.first_ms {
            return Some(0);
        }
        // From the nominal estimate, which a change of offset moves by at
        // most an occurrence, to the exact one.
        let mut k = (after_ms - self.first_ms) / self.every.nominal_ms().max(1);
        while self.at(zone, k)? <= after_ms {
            k += 1;
        }
        while k > 0 && self.at(zone, k - 1)? > after_ms {
            k -= 1;
        }
        Some(k)
    }

    /// The first occurrence after `after_ms` the days allow, or `None` when
    /// the series has ended by then (past `until`).
    pub fn next_after(&self, zone: &TimeZone, after_ms: u64) -> Option<u64> {
        let from = self.index_after(zone, after_ms)?;
        for k in (from..).take(MOST_SKIPPED) {
            let t = self.at(zone, k)?;
            if self.until_ms.is_some_and(|u| t > u) {
                return None;
            }
            if self.allows(zone, t) {
                return Some(t);
            }
        }
        None
    }

    /// The series' first due time: its first occurrence the days allow.
    pub fn first(&self, zone: &TimeZone) -> Option<u64> {
        self.next_after(zone, self.first_ms.checked_sub(1)?)
    }

    /// How many occurrences the days allow fall in `(after_ms, upto_ms]`,
    /// none past `until`: those a turn that takes the one due at `after_ms`
    /// at `upto_ms` passes over.
    pub fn between(&self, zone: &TimeZone, after_ms: u64, upto_ms: u64) -> u64 {
        let upto = self.until_ms.map_or(upto_ms, |u| u.min(upto_ms));
        if upto <= after_ms {
            return 0;
        }
        let (Some(lo), Some(hi)) = (
            self.index_after(zone, after_ms),
            self.index_after(zone, upto),
        ) else {
            return 0;
        };
        if self.days.is_empty() {
            return hi.saturating_sub(lo);
        }
        (lo..hi.min(lo + MOST_COUNTED))
            .filter(|k| self.at(zone, *k).is_some_and(|t| self.allows(zone, t)))
            .count() as u64
    }
}
