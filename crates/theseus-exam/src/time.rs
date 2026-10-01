//! Wall-clock times for the fixtures and the recall note: a local
//! `YYYY-MM-DD HH:MM` at a fixed offset, to and from Unix milliseconds. The
//! exam's past is written in one zone (its file's `utc_offset_min`), so no
//! zone database is needed. Days from the civil calendar by Howard Hinnant's
//! algorithm (proleptic Gregorian).

use anyhow::{bail, Context, Result};

/// Days since 1970-01-01 of a civil date.
fn days_from_civil(y: i64, m: u32, d: u32) -> i64 {
    let y = if m <= 2 { y - 1 } else { y };
    let era = if y >= 0 { y } else { y - 399 } / 400;
    let yoe = y - era * 400;
    let mp = (i64::from(m) + 9) % 12;
    let doy = (153 * mp + 2) / 5 + i64::from(d) - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146_097 + doe - 719_468
}

/// The civil date of a day count since 1970-01-01.
fn civil_from_days(z: i64) -> (i64, u32, u32) {
    let z = z + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let m = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    let y = yoe + era * 400 + i64::from(m <= 2);
    (y, m, d)
}

fn days_in_month(y: i64, m: u32) -> u32 {
    match m {
        2 if (y % 4 == 0 && y % 100 != 0) || y % 400 == 0 => 29,
        2 => 28,
        4 | 6 | 9 | 11 => 30,
        _ => 31,
    }
}

/// `YYYY-MM-DD HH:MM`, local at `offset_min` minutes east of UTC, as Unix ms.
pub fn parse_local(s: &str, offset_min: i32) -> Result<u64> {
    let bad = || format!("{s:?} is not a time like 2026-09-14 10:02");
    let (date, clock) = s.trim().split_once(' ').with_context(bad)?;
    let mut d = date.split('-');
    let (y, m, day) = (d.next(), d.next(), d.next());
    let (Some(y), Some(m), Some(day), None) = (y, m, day, d.next()) else {
        bail!(bad());
    };
    let (h, min) = clock.split_once(':').with_context(bad)?;
    let num = |p: &str, w: usize| -> Result<i64> {
        if p.len() != w || !p.bytes().all(|b| b.is_ascii_digit()) {
            bail!(bad());
        }
        Ok(p.parse()?)
    };
    let (y, m, day, h, min) = (
        num(y, 4)?,
        num(m, 2)?,
        num(day, 2)?,
        num(h, 2)?,
        num(min, 2)?,
    );
    if !(1..=12).contains(&m) || h > 23 || min > 59 {
        bail!(bad());
    }
    let m = m as u32;
    if day < 1 || day > i64::from(days_in_month(y, m)) {
        bail!(bad());
    }
    let local_s = days_from_civil(y, m, day as u32) * 86_400 + h * 3600 + min * 60;
    let utc_s = local_s - i64::from(offset_min) * 60;
    if utc_s < 0 {
        bail!("{s:?} is before 1970");
    }
    Ok(utc_s as u64 * 1000)
}

/// Unix ms as `YYYY-MM-DD HH:MM`, local at `offset_min` minutes east of UTC.
pub fn format_local(ms: u64, offset_min: i32) -> String {
    let s = (ms / 1000) as i64 + i64::from(offset_min) * 60;
    let (days, secs) = (s.div_euclid(86_400), s.rem_euclid(86_400));
    let (y, m, d) = civil_from_days(days);
    format!(
        "{y:04}-{m:02}-{d:02} {:02}:{:02}",
        secs / 3600,
        (secs % 3600) / 60
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    const MST: i32 = -7 * 60;

    /// Against `date -u -d … +%s` and `TZ=America/Phoenix date -d @…`.
    #[test]
    fn local_times_match_the_system_clock_tables() {
        assert_eq!(parse_local("1970-01-01 00:00", 0).unwrap(), 0);
        assert_eq!(
            parse_local("2026-09-30 00:00", 0).unwrap(),
            1_790_726_400_000
        );
        assert_eq!(parse_local("2000-02-29 12:34", 0).unwrap(), 951_827_640_000);
        assert_eq!(
            parse_local("2026-09-14 10:02", MST).unwrap(),
            1_789_405_320_000
        );
        assert_eq!(format_local(1_790_000_000_000, MST), "2026-09-21 07:13");
        assert_eq!(format_local(951_827_640_000, 0), "2000-02-29 12:34");
    }

    #[test]
    fn every_minute_of_a_leap_february_round_trips() {
        let start = parse_local("2028-02-27 00:00", MST).unwrap();
        for k in 0..(4 * 24 * 60) {
            let ms = start + k * 60_000;
            let s = format_local(ms, MST);
            assert_eq!(parse_local(&s, MST).unwrap(), ms, "{s}");
        }
        assert_eq!(
            format_local(start + 2 * 86_400_000, MST),
            "2028-02-29 00:00"
        );
    }

    #[test]
    fn malformed_times_are_refused_with_the_shape_expected() {
        for s in [
            "2026-09-14",
            "2026-9-14 10:02",
            "2026-09-14 24:00",
            "2026-09-31 10:00",
            "2027-02-29 10:00",
            "2026-13-01 10:00",
            "2026-09-14 10:2",
            "2026-09-14T10:02",
            "1969-12-31 23:59",
        ] {
            assert!(parse_local(s, 0).is_err(), "{s} parsed");
        }
        let e = parse_local("tomorrow", 0).unwrap_err().to_string();
        assert!(e.contains("2026-09-14 10:02"), "{e}");
    }
}
