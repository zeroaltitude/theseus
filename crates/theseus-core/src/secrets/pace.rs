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
//! - **The ceiling** ([`default_vault_reads_per_day`]): past it, no failing
//!   entry is fetched again until the next local day, and health says so. It
//!   is a constant, not a setting.

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

/// The time the pace reads: the daemon's clock, or a test's.
pub trait Clock: Send + Sync {
    /// Unix milliseconds now.
    fn now_ms(&self) -> u64;
    /// Wait `d`.
    fn sleep(&self, d: Duration) -> BoxFuture<'static, ()>;
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
    day: String,
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

    fn with_today<T>(&self, f: impl FnOnce(&mut Day) -> T) -> T {
        let today = crate::judge::spend::local_day(self.clock.now_ms());
        let mut d = self.day.lock().unwrap_or_else(|p| p.into_inner());
        if d.day != today {
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
            Self::RateLimited(Some(d)) => d.clamp(Duration::from_secs(60), Duration::from_secs(86_400)),
            _ => REFUSED_WAIT,
        }
    }

    /// In a word, for the log and health.
    pub fn word(self) -> &'static str {
        match self {
            Self::RateLimited(_) => "the vault's rate limit",
            Self::Token => "the vault refused the token",
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
    let n: u64 = digits.parse().ok()?;
    let unit = rest[digits.len()..].trim_start();
    let secs = if unit.starts_with("h") {
        n * 3600
    } else if unit.starts_with("min") || unit.starts_with("m ") || unit == "m" {
        n * 60
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
        assert_eq!(Refused::of("could not read item-0429 (no such field)"), None);
        assert_eq!(Refused::of("could not find item x"), None);
        assert_eq!(Refused::RateLimited(None).wait(), REFUSED_WAIT);
        assert_eq!(
            Refused::RateLimited(Some(Duration::from_secs(5))).wait(),
            Duration::from_secs(60),
            "at least a minute"
        );
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
