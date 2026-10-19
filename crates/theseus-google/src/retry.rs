//! Retries (Google design §4.3, `theseus-aws`'s shape): a throttle and a
//! server error back off with full jitter, three attempts from 1 s up to 20 s,
//! honouring `Retry-After`. A call that must not repeat (a send, an authority
//! method) that may have gone out is `Unknown`, never sent again.
//!
//! | failure | repeatable | non-repeatable |
//! |---|---|---|
//! | never sent (the connection failed) | retry | retry |
//! | a throttle (it was refused) | retry | retry |
//! | a server error, a timeout, a drop after sending | retry | unknown |
//! | any other error | fail | fail |

use std::collections::hash_map::RandomState;
use std::hash::{BuildHasher, Hasher};
use std::time::Duration;

use crate::error::{GoogleError, Kind};

/// Whether a request may be sent again once it may have run.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Repeat {
    Safe,
    NonRepeatable,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Policy {
    pub max_attempts: u32,
    pub initial_backoff: Duration,
    pub max_backoff: Duration,
}

impl Default for Policy {
    fn default() -> Self {
        Policy {
            max_attempts: 3,
            initial_backoff: Duration::from_secs(1),
            max_backoff: Duration::from_secs(20),
        }
    }
}

impl Policy {
    /// The wait before attempt `next` (2 or later): a random share of the
    /// exponential backoff.
    pub fn delay(&self, next: u32) -> Duration {
        let exp = next.saturating_sub(2).min(16);
        let cap = self
            .initial_backoff
            .saturating_mul(1u32 << exp)
            .min(self.max_backoff);
        let mut h = RandomState::new().build_hasher();
        h.write_u32(next);
        let share = (h.finish() >> 11) as f64 / (1u64 << 53) as f64;
        cap.mul_f64(share)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Decision {
    Retry,
    Fail,
    /// The request may have run, and repeating it is not safe.
    Unknown,
}

pub fn decide(repeat: Repeat, e: &GoogleError) -> Decision {
    match e.kind {
        Kind::Network if e.reason == "connect" => Decision::Retry,
        Kind::Network if repeat == Repeat::Safe => Decision::Retry,
        Kind::Network => Decision::Unknown,
        Kind::Throttle => Decision::Retry,
        Kind::Server if repeat == Repeat::Safe => Decision::Retry,
        Kind::Server => Decision::Unknown,
        _ => Decision::Fail,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn err(kind: Kind, reason: &str) -> GoogleError {
        GoogleError {
            status: 500,
            reason: reason.into(),
            message: String::new(),
            kind,
        }
    }

    #[test]
    fn the_table() {
        use Decision::*;
        let server = err(Kind::Server, "backendError");
        let throttle = err(Kind::Throttle, "rateLimitExceeded");
        let other = err(Kind::Refused, "notFound");
        let sent = err(Kind::Network, "timeout");
        let not_sent = err(Kind::Network, "connect");
        for repeat in [Repeat::Safe, Repeat::NonRepeatable] {
            let safe = repeat == Repeat::Safe;
            assert_eq!(decide(repeat, &throttle), Retry);
            assert_eq!(decide(repeat, &other), Fail);
            assert_eq!(decide(repeat, &not_sent), Retry);
            assert_eq!(decide(repeat, &server), if safe { Retry } else { Unknown });
            assert_eq!(decide(repeat, &sent), if safe { Retry } else { Unknown });
        }
    }

    #[test]
    fn backoff_grows_within_its_cap() {
        let p = Policy::default();
        for next in 2..10 {
            let cap = Duration::from_secs(1u64 << (next - 2)).min(p.max_backoff);
            assert!(p.delay(next) <= cap);
        }
    }
}
