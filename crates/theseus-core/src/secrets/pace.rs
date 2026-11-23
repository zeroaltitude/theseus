//! How often the daemon reads the vault (issue 44): a count of its own
//! reads, the pace a failing entry is fetched again at, the vault's refusal
//! told apart from a missing entry, and a day's ceiling.
//!
//! - **The count** ([`VaultReads`]): each `op` process the daemon starts, and
//!   the entries it asks for (one per `op read`, each reference of an `op
//!   inject`), per local day. Counts only: never a name or a value. Health's
//!   `secrets.vault_today` reads it.
//! - **The ladder** ([`LADDER`]): a failing entry is fetched again after 5 s,
//!   10 s, 20 s, 40 s and 60 s, then 10 minutes, then every hour, each wait
//!   20% either way at random so that daemons started together do not ask
//!   together. It was every minute for ever, which is about 1,440 rounds a
//!   day for one bad entry; now it is about 30.
//! - **A refusal** ([`Refused`]): when `op` says the vault refused (a rate
//!   limit, or the token itself), no entry is read one by one to name the bad
//!   one, since none is bad, and the next try waits an hour, or until the
//!   reset `op` names.
//! - **The ceiling** ([`default_vault_reads_per_day`]): before every round the
//!   resolver keeps room for that round's worst case ([`worst_case`]): a round
//!   that could pass the day's ceiling does not start until the next local
//!   midnight ([`next_civil_midnight`]) or the next start, and health says so.
//!   So the daemon's automatic reads stay within it; a look or a keep the owner
//!   asks for is counted, and not stopped. It is a constant, not a setting.

use std::sync::{Arc, Mutex, OnceLock};
use std::time::Duration;

use futures_util::future::BoxFuture;

/// The waits between a failing entry's rounds, in order; the last one
/// repeats.
pub const LADDER: [Duration; 7] = [
    Duration::from_secs(5),
    Duration::from_secs(10),
    Duration::from_secs(20),
    Duration::from_secs(40),
    Duration::from_secs(60),
    Duration::from_secs(600),
    Duration::from_secs(3600),
];

/// How far either way a wait is moved at random, in percent.
pub const JITTER_PERCENT: u64 = 20;

/// The wait after the vault refused, when `op` names no reset.
pub const REFUSED_WAIT: Duration = Duration::from_secs(3600);

/// The entries the daemon reads from the vault in a local day past which a
/// failing entry is not fetched again (issue 44: the vault's whole account
/// allows 50,000 requests a day, shared by every reader). A constant, the
/// owner's choice of fewer settings; it is a `default_*_per_day` function so
/// that the keel's cap rule reads its number and a raise is a finding.
pub fn default_vault_reads_per_day() -> u32 {
    2000
}

/// The least wait before the day's ceiling is looked at again, should a wake
/// come before the day changed (a clock that stepped): never a loop a second
/// apart.
pub const DAY_WAKE_FLOOR: Duration = Duration::from_secs(60);

/// The time the pace reads: the daemon's clock, or a test's.
pub trait Clock: Send + Sync {
    /// Unix milliseconds now.
    fn now_ms(&self) -> u64;
    /// Wait `d`.
    fn sleep(&self, d: Duration) -> BoxFuture<'static, ()>;
    /// The zone's offset east of UTC at `unix_ms`, in seconds: the daemon's
    /// zone, as `wake::local` reads a calendar day, or a test's.
    fn offset_secs(&self, unix_ms: u64) -> i64 {
        crate::wake::local(unix_ms).offset_secs
    }
}

/// The local day `unix_ms` falls on at `offset_secs`, as a day number since
/// 1970-01-01: dates compare as numbers.
pub fn day_number(unix_ms: u64, offset_secs: i64) -> i64 {
    i64::try_from(unix_ms / 1000)
        .unwrap_or(i64::MAX)
        .saturating_add(offset_secs)
        .div_euclid(86_400)
}

/// The next local midnight after `now_ms`: tomorrow's date at 00:00 in the
/// zone `offset` gives, as a civil date (the offset of tomorrow's midnight,
/// not today's, so a day the clock springs or falls is a day). A midnight the
/// zone skips gives tomorrow's first instant.
pub fn next_civil_midnight(now_ms: u64, offset: &dyn Fn(u64) -> i64) -> u64 {
    let today = day_number(now_ms, offset(now_ms));
    let local = today.saturating_add(1).saturating_mul(86_400);
    let at = |off: i64| {
        u64::try_from(local.saturating_sub(off).max(0))
            .unwrap_or(0)
            .saturating_mul(1000)
    };
    let mut off = offset(now_ms);
    for _ in 0..3 {
        let o = offset(at(off));
        if o == off {
            break;
        }
        off = o;
    }
    let mut ms = at(off).max(now_ms.saturating_add(1));
    // Back to the earliest instant of tomorrow, and on past a skipped midnight.
    while ms > now_ms.saturating_add(60_000) && day_number(ms - 60_000, offset(ms - 60_000)) > today
    {
        ms -= 60_000;
    }
    while day_number(ms, offset(ms)) <= today {
        ms = ms.saturating_add(60_000);
    }
    ms
}

/// The most entries one round can read for `vault_refs` distinct references:
/// one `op read` for one; an injection of all and, when it fails, a read of
/// each, for more.
pub fn worst_case(vault_refs: usize) -> u32 {
    match vault_refs {
        0 => 0,
        1 => 1,
        n => u32::try_from(n).unwrap_or(u32::MAX).saturating_mul(2),
    }
}

/// The system's clock, and tokio's timer.
pub struct SystemClock;

impl Clock for SystemClock {
    fn now_ms(&self) -> u64 {
        theseus_protocol::now_unix_ms()
    }
    fn sleep(&self, d: Duration) -> BoxFuture<'static, ()> {
        Box::pin(tokio::time::sleep(d))
    }
}

#[derive(Default)]
struct Day {
    /// The day's number ([`day_number`]); 0 before the first read.
    day: i64,
    processes: u32,
    references: u32,
}

/// The daemon's own vault reads in the local day.
pub struct VaultReads {
    clock: Arc<dyn Clock>,
    day: Mutex<Day>,
}

impl VaultReads {
    pub fn new(clock: Arc<dyn Clock>) -> Arc<Self> {
        Arc::new(Self {
            clock,
            day: Mutex::default(),
        })
    }

    /// The process's count, which every reader `OpReader::from_env` makes
    /// shares: the daemon's secrets, its config note, its looks and writes.
    pub fn process() -> Arc<Self> {
        static PROCESS: OnceLock<Arc<VaultReads>> = OnceLock::new();
        PROCESS
            .get_or_init(|| Self::new(Arc::new(SystemClock)))
            .clone()
    }

    pub fn clock(&self) -> Arc<dyn Clock> {
        self.clock.clone()
    }

    /// The day's tally, the date sampled under its lock: a later date starts a
    /// new one, an earlier one (a clock that stepped back, a sample taken a
    /// moment before another) counts into the later day it already holds.
    fn with_today<T>(&self, f: impl FnOnce(&mut Day) -> T) -> T {
        let mut d = self.day.lock().unwrap_or_else(|p| p.into_inner());
        let now = self.clock.now_ms();
        let today = day_number(now, self.clock.offset_secs(now));
        if today > d.day {
            *d = Day {
                day: today,
                ..Day::default()
            };
        }
        f(&mut d)
    }

    /// One `op` process started, asking for `references` entries.
    pub fn note(&self, references: usize) {
        let n = u32::try_from(references).unwrap_or(u32::MAX);
        self.with_today(|d| {
            d.processes = d.processes.saturating_add(1);
            d.references = d.references.saturating_add(n);
        });
    }

    /// Today's processes and entries.
    pub fn today(&self) -> (u32, u32) {
        self.with_today(|d| (d.processes, d.references))
    }
}

/// How the vault refused, as `op`'s own words say it: the vault's answer,
/// not one entry's.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Refused {
    /// Too many requests; the reset, when `op` names one.
    RateLimited(Option<Duration>),
    /// The token itself was refused.
    Token,
}

impl Refused {
    /// Whether `why` (an `op` failure, its own words) is the vault's refusal.
    /// The words are those a rate limit and a refused token are commonly given
    /// in (`rate limit`, `too many requests`, a 429 status; `unauthorized`, a
    /// 401 status, an invalid token); a missing item or a field matches none,
    /// and so does a bare number, which an item's name may hold.
    pub fn of(why: &str) -> Option<Self> {
        let w = why.to_ascii_lowercase();
        let status = |n: &str| {
            [format!("({n})"), format!("status {n}"), format!("http {n}")]
                .iter()
                .any(|s| w.contains(s.as_str()))
        };
        if w.contains("rate limit")
            || w.contains("rate-limit")
            || w.contains("too many requests")
            || status("429")
        {
            return Some(Self::RateLimited(reset_in(&w)));
        }
        let token = w.contains("unauthorized")
            || status("401")
            || w.contains("invalid token")
            || w.contains("token is invalid")
            || w.contains("invalid service account token");
        token.then_some(Self::Token)
    }

    /// The wait before the next try: the reset `op` named, at least a minute
    /// and at most a day, else an hour.
    pub fn wait(self) -> Duration {
        match self {
            Self::RateLimited(Some(d)) => {
                d.clamp(Duration::from_secs(60), Duration::from_secs(86_400))
            }
            _ => REFUSED_WAIT,
        }
    }

    /// Of two refusals, the one with the longer wait (round 3: a round's next
    /// wait is its longest refusal's).
    pub fn longer(a: Option<Self>, b: Option<Self>) -> Option<Self> {
        match (a, b) {
            (Some(x), Some(y)) if y.wait() > x.wait() => Some(y),
            (Some(x), _) => Some(x),
            (None, y) => y,
        }
    }

    /// In a word, for the log.
    pub fn word(self) -> &'static str {
        match self {
            Self::RateLimited(_) => "the vault's rate limit",
            Self::Token => "the vault refused the token",
        }
    }

    /// What an entry the vault refused says in health: fixed words and the
    /// bounded wait, never `op`'s own.
    pub fn said(self) -> String {
        let again = crate::wake::span(u64::try_from(self.wait().as_millis()).unwrap_or(u64::MAX));
        match self {
            Self::RateLimited(_) => {
                format!("the vault refused the read: its rate limit; it is asked again in {again}")
            }
            Self::Token => format!("the vault refused the token; it is asked again in {again}"),
        }
    }
}

/// The reset in `w` (lowercase): a number after `retry after`, `retry-after:`
/// or `try again in`, in seconds unless a unit follows (`min`, `minute`,
/// `hour`, `h`).
fn reset_in(w: &str) -> Option<Duration> {
    let at = ["retry after", "retry-after:", "retry-after", "try again in"]
        .iter()
        .find_map(|k| w.find(k).map(|i| i + k.len()))?;
    let rest = w[at..].trim_start();
    let digits: String = rest.chars().take_while(char::is_ascii_digit).collect();
    if digits.is_empty() {
        return None;
    }
    // A number past u64 is as long as can be; the clamp bounds it.
    let n: u64 = digits.parse().unwrap_or(u64::MAX);
    let unit = rest[digits.len()..].trim_start();
    let secs = if unit.starts_with('h') {
        n.saturating_mul(3600)
    } else if unit.starts_with("min") || unit.starts_with("m ") || unit == "m" {
        n.saturating_mul(60)
    } else {
        n
    };
    Some(Duration::from_secs(secs))
}

/// The pace a failing entry is fetched again at.
#[derive(Clone)]
pub struct Pace {
    pub ladder: Vec<Duration>,
    /// Whether each wait is moved at random ([`JITTER_PERCENT`]).
    pub jitter: bool,
    /// [`default_vault_reads_per_day`], or a test's.
    pub ceiling: u32,
    pub clock: Arc<dyn Clock>,
}

impl Pace {
    /// The daemon's: the ladder, with jitter, the day's ceiling, on `clock`.
    pub fn daemon(clock: Arc<dyn Clock>) -> Self {
        Self {
            ladder: LADDER.to_vec(),
            jitter: true,
            ceiling: default_vault_reads_per_day(),
            clock,
        }
    }

    /// The wait after the `step`th failed round (0 for the first), jittered.
    pub fn wait(&self, step: usize) -> Duration {
        let base = match self.ladder.get(step).or(self.ladder.last()) {
            Some(d) => *d,
            None => return LADDER[0],
        };
        if !self.jitter {
            return base;
        }
        let spread = base.as_millis() as u64 * JITTER_PERCENT / 100;
        let r = (uuid::Uuid::new_v4().as_u128() as u64) % (2 * spread + 1);
        Duration::from_millis(base.as_millis() as u64 - spread + r)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_vaults_refusal_is_told_from_a_missing_entry() {
        assert_eq!(
            Refused::of("Too many requests. Retry after 600 seconds"),
            Some(Refused::RateLimited(Some(Duration::from_secs(600))))
        );
        assert_eq!(
            Refused::of("rate limit exceeded, try again in 2 hours"),
            Some(Refused::RateLimited(Some(Duration::from_secs(7200))))
        );
        assert_eq!(
            Refused::of("(429) the service account's daily limit is reached"),
            Some(Refused::RateLimited(None))
        );
        assert_eq!(
            Refused::of("unauthorized: invalid service account token"),
            Some(Refused::Token)
        );
        assert_eq!(Refused::of("\"x\" isn't an item in the \"v\" vault"), None);
        assert_eq!(
            Refused::of("could not read item-0429 (no such field)"),
            None
        );
        assert_eq!(Refused::of("could not find item x"), None);
        assert_eq!(Refused::RateLimited(None).wait(), REFUSED_WAIT);
        assert_eq!(
            Refused::RateLimited(Some(Duration::from_secs(5))).wait(),
            Duration::from_secs(60),
            "at least a minute"
        );
    }

    /// A zone with both clock changes, as Denver has them in 2026: UTC-7, and
    /// UTC-6 from 2026-03-08 02:00 local (09:00 UTC) to 2026-11-01 02:00 local
    /// (08:00 UTC).
    fn denver(ms: u64) -> i64 {
        let (on, off) = (utc(2026, 3, 8, 9), utc(2026, 11, 1, 8));
        if (on..off).contains(&ms) {
            -6 * 3600
        } else {
            -7 * 3600
        }
    }

    /// `y-m-d h:00` UTC, in Unix milliseconds.
    fn utc(y: i64, m: u32, d: u32, h: u64) -> u64 {
        crate::wake::days_from_civil(y, m, d) as u64 * 86_400_000 + h * 3_600_000
    }

    /// Round 2: the next midnight is tomorrow's date at 00:00 local, across a
    /// spring-forward and a fall-back day, and the first instant of a day whose
    /// midnight the zone skips.
    #[test]
    fn the_next_civil_midnight_is_tomorrows_date_at_its_start() {
        // 2026-03-07 22:00 MST: 2026-03-08 00:00 MST.
        assert_eq!(
            next_civil_midnight(utc(2026, 3, 8, 5), &denver),
            utc(2026, 3, 8, 7)
        );
        // 2026-03-08 12:00 MDT, the day the clock sprang: 2026-03-09 00:00 MDT.
        assert_eq!(
            next_civil_midnight(utc(2026, 3, 8, 18), &denver),
            utc(2026, 3, 9, 6)
        );
        // 2026-10-31 20:00 MDT: 2026-11-01 00:00 MDT.
        assert_eq!(
            next_civil_midnight(utc(2026, 11, 1, 2), &denver),
            utc(2026, 11, 1, 6)
        );
        // 2026-11-01 12:00 MST, the day the clock fell back: 2026-11-02 00:00 MST.
        assert_eq!(
            next_civil_midnight(utc(2026, 11, 1, 19), &denver),
            utc(2026, 11, 2, 7)
        );
        // A zone that skips 2026-03-08's midnight (00:00 MST becomes 01:00 MDT):
        // that day begins at 07:00 UTC.
        let skips = |ms: u64| {
            if ms >= utc(2026, 3, 8, 7) {
                -6 * 3600
            } else {
                -7 * 3600
            }
        };
        assert_eq!(
            next_civil_midnight(utc(2026, 3, 8, 5), &skips),
            utc(2026, 3, 8, 7)
        );
    }

    /// A clock a test sets, in UTC.
    struct Set(std::sync::atomic::AtomicU64);

    impl Clock for Set {
        fn now_ms(&self) -> u64 {
            self.0.load(std::sync::atomic::Ordering::SeqCst)
        }
        fn sleep(&self, _: Duration) -> BoxFuture<'static, ()> {
            Box::pin(std::future::ready(()))
        }
        fn offset_secs(&self, _: u64) -> i64 {
            0
        }
    }

    /// Round 2: a date that goes back (the clock stepped back over a midnight)
    /// does not reset the day's tally, and its return does not either.
    #[test]
    fn a_backward_date_does_not_reset_the_days_tally() {
        let clock = Arc::new(Set(std::sync::atomic::AtomicU64::new(
            utc(2026, 11, 23, 0) + 60_000,
        )));
        let reads = VaultReads::new(clock.clone());
        reads.note(1);
        clock
            .0
            .store(utc(2026, 11, 22, 23), std::sync::atomic::Ordering::SeqCst);
        reads.note(1);
        assert_eq!(
            reads.today(),
            (2, 2),
            "the earlier date counts into the later day"
        );
        clock
            .0
            .store(utc(2026, 11, 23, 1), std::sync::atomic::Ordering::SeqCst);
        assert_eq!(reads.today(), (2, 2), "and the return resets nothing");
        clock
            .0
            .store(utc(2026, 11, 24, 1), std::sync::atomic::Ordering::SeqCst);
        assert_eq!(reads.today(), (0, 0), "a later day starts afresh");
    }

    /// Round 2: a reset's number is multiplied with saturation before the
    /// clamp, and the refusal's words are fixed: its class and its wait.
    #[test]
    fn a_huge_reset_saturates_and_the_refusals_words_are_fixed() {
        let r = Refused::of("too many requests; try again in 99999999999999999999 hours")
            .expect("a rate limit");
        assert_eq!(r.wait(), Duration::from_secs(86_400));
        let r = Refused::of("too many requests; try again in 9999999999999999 hours").unwrap();
        assert_eq!(r.wait(), Duration::from_secs(86_400));
        assert_eq!(
            Refused::RateLimited(Some(Duration::from_secs(600))).said(),
            "the vault refused the read: its rate limit; it is asked again in 10 min"
        );
        assert_eq!(
            Refused::Token.said(),
            "the vault refused the token; it is asked again in 1 h"
        );
        assert_eq!(worst_case(0), 0);
        assert_eq!(worst_case(1), 1);
        assert_eq!(worst_case(3), 6);
    }

    #[test]
    fn a_wait_climbs_the_ladder_and_stays_on_its_last_rung() {
        let pace = Pace {
            jitter: false,
            ..Pace::daemon(Arc::new(SystemClock))
        };
        let waits: Vec<u64> = (0..9).map(|s| pace.wait(s).as_secs()).collect();
        assert_eq!(waits, [5, 10, 20, 40, 60, 600, 3600, 3600, 3600]);
        let jittered = Pace::daemon(Arc::new(SystemClock));
        for _ in 0..200 {
            let w = jittered.wait(6).as_millis();
            assert!((2_880_000..=4_320_000).contains(&w), "{w}");
        }
        assert_eq!(default_vault_reads_per_day(), 2000);
    }
}
