//! The date a recalled item's header gives (theseus-q1tbn): when the note
//! was *said*, in the owner's zone with the weekday. What a "yesterday" in
//! its words meant is the model's to read (issue 46).
//!
//! `said Tue 2026-03-17 19:40 MST`
//!
//! Computed from the node alone, never from the clock, so a header's bytes
//! are the same whenever it is built; a header is frozen when recalled.

use std::ffi::CStr;

use crate::wake::{civil_from_days, local};

/// A time zone at an instant: its offset east of UTC, and its name.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Zone {
    pub offset_secs: i64,
    pub name: String,
}

/// The zone at an instant (daylight saving moves it).
pub type ZoneAt<'a> = &'a dyn Fn(u64) -> Zone;

/// The daemon's own zone at `ms`: the system's, as `localtime_r` reads it,
/// named as the C library names it (`MST`), else by its offset from UTC
/// (`UTC-07:00`).
pub fn system(ms: u64) -> Zone {
    let offset_secs = local(ms).offset_secs;
    let named = (|| {
        let secs: libc::time_t = (ms / 1000).try_into().ok()?;
        // SAFETY: `localtime_r` writes only the `tm` it is given, on this
        // frame; the zone name it points at is the C library's and is read
        // before any other call.
        let mut tm: libc::tm = unsafe { std::mem::zeroed() };
        if unsafe { libc::localtime_r(&secs, &mut tm) }.is_null() || tm.tm_zone.is_null() {
            return None;
        }
        let name = unsafe { CStr::from_ptr(tm.tm_zone) }
            .to_str()
            .ok()?
            .to_string();
        (!name.is_empty()).then_some(name)
    })();
    let name = named.unwrap_or_else(|| {
        let sign = if offset_secs < 0 { '-' } else { '+' };
        let off = offset_secs.unsigned_abs();
        format!("UTC{sign}{:02}:{:02}", off / 3600, off % 3600 / 60)
    });
    Zone { offset_secs, name }
}

/// The local days since the epoch, and the seconds into the day, of `ms`.
fn parts(ms: u64, z: &Zone) -> (i64, i64) {
    let local = (ms / 1000) as i64 + z.offset_secs;
    (local.div_euclid(86_400), local.rem_euclid(86_400))
}

/// `Tue 2026-03-17`: local day number `days`.
fn day_name(days: i64) -> String {
    // 1970-01-01 was a Thursday.
    const DAYS: [&str; 7] = ["Thu", "Fri", "Sat", "Sun", "Mon", "Tue", "Wed"];
    let (y, m, d) = civil_from_days(days);
    format!("{} {y:04}-{m:02}-{d:02}", DAYS[days.rem_euclid(7) as usize])
}

/// `Tue 2026-03-17 19:40 MST`: `ms` in `zone`.
pub fn said_in(ms: u64, zone: ZoneAt<'_>) -> String {
    let z = zone(ms);
    let (days, secs) = parts(ms, &z);
    format!(
        "{} {:02}:{:02} {}",
        day_name(days),
        secs / 3600,
        secs / 60 % 60,
        z.name
    )
}

/// `Tue 2026-03-17`: the local day of `ms` in `zone`.
pub fn day_in(ms: u64, zone: ZoneAt<'_>) -> String {
    day_name(parts(ms, &zone(ms)).0)
}

/// The local day of `ms` in the daemon's zone, as `2026-03-17`.
pub fn date(ms: u64) -> String {
    day_in(ms, &system)[4..].to_string()
}
