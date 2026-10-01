//! Retries by retry class (AWS design §3.1, "Retry class"). Generic SDK-style
//! retry never runs, since it would contradict the class:
//!
//! | failure | `safe_to_repeat` and `idempotent_with_key` | `non_repeatable` |
//! |---|---|---|
//! | never sent (the connection failed) | retry | retry |
//! | a throttle (the request was refused) | retry | retry |
//! | a server error, a timeout, or a drop after sending | retry | `outcome_unknown` |
//! | any other error | fail | fail |
//!
//! An `idempotent_with_key` call keeps its token across attempts, so AWS
//! runs it at most once.

use std::collections::hash_map::RandomState;
use std::hash::{BuildHasher, Hasher};
use std::time::Duration;

use theseus_aws_catalog::RetryClass;

use crate::error::{AwsError, ErrorRetry};

/// How often, and how patiently, a call is retried. The defaults are the
/// SDKs' standard mode: three attempts, backoff from 1 s, at most 20 s, with
/// full jitter.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RetryPolicy {
    pub max_attempts: u32,
    pub initial_backoff: Duration,
    pub max_backoff: Duration,
}

impl Default for RetryPolicy {
    fn default() -> Self {
        RetryPolicy {
            max_attempts: 3,
            initial_backoff: Duration::from_secs(1),
            max_backoff: Duration::from_secs(20),
        }
    }
}

impl RetryPolicy {
    /// The wait before attempt `next` (2 or later): a random share of the
    /// exponential backoff.
    pub(crate) fn delay(&self, next: u32) -> Duration {
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

/// How an attempt failed.
#[derive(Debug)]
pub(crate) enum Failure<'a> {
    Aws(&'a AwsError),
    /// The connection failed before the request was sent.
    NotSent,
    /// Sent, then no complete answer: a timeout, or a dropped connection.
    AfterSend,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Decision {
    Retry,
    Fail,
    /// The request may have run, and repeating it is not safe.
    Unknown,
}

pub(crate) fn decide(class: RetryClass, f: &Failure<'_>) -> Decision {
    let repeatable = class != RetryClass::NonRepeatable;
    match f {
        Failure::NotSent => Decision::Retry,
        Failure::AfterSend if repeatable => Decision::Retry,
        Failure::AfterSend => Decision::Unknown,
        Failure::Aws(e) => match e.retry {
            ErrorRetry::Throttle => Decision::Retry,
            ErrorRetry::Transient if repeatable => Decision::Retry,
            ErrorRetry::Transient => Decision::Unknown,
            ErrorRetry::No => Decision::Fail,
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn err(retry: ErrorRetry) -> AwsError {
        AwsError {
            status: 400,
            code: "X".into(),
            message: String::new(),
            request_id: None,
            retry,
            denial: None,
        }
    }

    #[test]
    fn the_table() {
        use Decision::*;
        use RetryClass::*;
        let throttle = err(ErrorRetry::Throttle);
        let transient = err(ErrorRetry::Transient);
        let other = err(ErrorRetry::No);
        for class in [SafeToRepeat, IdempotentWithKey, NonRepeatable] {
            let repeat = class != NonRepeatable;
            assert_eq!(decide(class, &Failure::NotSent), Retry);
            assert_eq!(decide(class, &Failure::Aws(&throttle)), Retry);
            assert_eq!(decide(class, &Failure::Aws(&other)), Fail);
            assert_eq!(
                decide(class, &Failure::AfterSend),
                if repeat { Retry } else { Unknown }
            );
            assert_eq!(
                decide(class, &Failure::Aws(&transient)),
                if repeat { Retry } else { Unknown }
            );
        }
    }

    #[test]
    fn backoff_grows_within_its_cap() {
        let p = RetryPolicy::default();
        for next in 2..10 {
            let d = p.delay(next);
            let cap = Duration::from_secs(1u64 << (next - 2)).min(p.max_backoff);
            assert!(d <= cap, "attempt {next}: {d:?} over {cap:?}");
        }
    }
}
