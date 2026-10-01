//! The circuit breaker (design §2.2). Five transient failures in a row open
//! it for 60 s, then one probe call goes out. While it is open, shadow skips
//! and live abstains at once, so an outage adds no latency to anything.
//!
//! A pure state machine over explicit instants, so tests drive its clock.
//! Only Jev's transient failures count (a timeout, the network, a 429, a 5xx);
//! an answer of any kind, malformed or refused included, shows the service is
//! there and ends the streak. A call that never went out (shed, no key)
//! changes nothing, except that a probe which did not go out frees the probe
//! slot at once.

use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct BreakerConfig {
    /// Transient failures in a row that open it.
    pub failures: u32,
    /// How long it stays open before a probe.
    pub open_for: Duration,
    /// How long a probe may stay out before another may go, in case the
    /// first was dropped without a word.
    pub probe_timeout: Duration,
}

impl Default for BreakerConfig {
    fn default() -> Self {
        Self {
            failures: 5,
            open_for: Duration::from_secs(60),
            probe_timeout: Duration::from_secs(10),
        }
    }
}

/// How a call went, as the breaker counts it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Outcome {
    /// Jev answered (well or not).
    Answered,
    /// A transient failure of Jev's.
    Transient,
    /// The call never went out.
    NotSent,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum State {
    Closed { failures: u32 },
    Open { until: Instant },
    HalfOpen { since: Instant },
}

/// A change the core ledgers as a `judge.circuit` row (and a health line,
/// and a narrative line).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "circuit", rename_all = "snake_case")]
pub enum Transition {
    /// Opened after `failures` transient failures in a row, for `for_secs`.
    Opened { failures: u32, for_secs: u64 },
    /// A probe failed; open again for `for_secs`.
    Reopened { for_secs: u64 },
    /// A probe was answered; closed.
    Closed,
}

/// What health shows.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "state", rename_all = "snake_case")]
pub enum Status {
    Closed { failures: u32 },
    Open { secs_left: u64 },
    HalfOpen,
}

#[derive(Debug, Clone)]
pub struct Breaker {
    config: BreakerConfig,
    state: State,
}

impl Breaker {
    pub fn new(config: BreakerConfig) -> Self {
        Self {
            config,
            state: State::Closed { failures: 0 },
        }
    }

    /// Whether a call may go out now. When it is open and its time is up,
    /// the first caller becomes the probe; the rest wait out the probe.
    pub fn admit(&mut self, now: Instant) -> bool {
        match self.state {
            State::Closed { .. } => true,
            State::Open { until } if now >= until => {
                self.state = State::HalfOpen { since: now };
                true
            }
            State::Open { .. } => false,
            State::HalfOpen { since }
                if now.saturating_duration_since(since) >= self.config.probe_timeout =>
            {
                self.state = State::HalfOpen { since: now };
                true
            }
            State::HalfOpen { .. } => false,
        }
    }

    /// Counts one admitted call's outcome; returns the change, if any.
    pub fn record(&mut self, now: Instant, outcome: Outcome) -> Option<Transition> {
        let open_for = self.config.open_for;
        match (self.state, outcome) {
            (_, Outcome::NotSent) => {
                if let State::HalfOpen { .. } = self.state {
                    // The probe never went out: the next caller probes.
                    self.state = State::Open { until: now };
                }
                None
            }
            (State::Closed { .. }, Outcome::Answered) => {
                self.state = State::Closed { failures: 0 };
                None
            }
            (State::Closed { failures }, Outcome::Transient) => {
                let failures = failures + 1;
                if failures >= self.config.failures {
                    self.state = State::Open {
                        until: now + open_for,
                    };
                    Some(Transition::Opened {
                        failures,
                        for_secs: open_for.as_secs(),
                    })
                } else {
                    self.state = State::Closed { failures };
                    None
                }
            }
            (State::HalfOpen { .. }, Outcome::Answered) => {
                self.state = State::Closed { failures: 0 };
                Some(Transition::Closed)
            }
            (State::HalfOpen { .. }, Outcome::Transient) => {
                self.state = State::Open {
                    until: now + open_for,
                };
                Some(Transition::Reopened {
                    for_secs: open_for.as_secs(),
                })
            }
            // A call admitted before it opened, landing late: the probe
            // decides, not stragglers.
            (State::Open { .. }, _) => None,
        }
    }

    pub fn status(&self, now: Instant) -> Status {
        match self.state {
            State::Closed { failures } => Status::Closed { failures },
            State::Open { until } => Status::Open {
                secs_left: until.saturating_duration_since(now).as_secs(),
            },
            State::HalfOpen { .. } => Status::HalfOpen,
        }
    }

    pub fn is_open(&self) -> bool {
        !matches!(self.state, State::Closed { .. })
    }
}

impl Default for Breaker {
    fn default() -> Self {
        Self::new(BreakerConfig::default())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fail(b: &mut Breaker, now: Instant, n: u32) -> Vec<Transition> {
        (0..n)
            .filter_map(|_| {
                assert!(b.admit(now));
                b.record(now, Outcome::Transient)
            })
            .collect()
    }

    #[test]
    fn five_transient_failures_in_a_row_open_it_and_four_do_not() {
        let t0 = Instant::now();
        let mut b = Breaker::default();
        assert!(fail(&mut b, t0, 4).is_empty());
        assert_eq!(b.status(t0), Status::Closed { failures: 4 });
        assert_eq!(
            fail(&mut b, t0, 1),
            vec![Transition::Opened {
                failures: 5,
                for_secs: 60
            }]
        );
        assert!(b.is_open());
        assert!(!b.admit(t0 + Duration::from_secs(59)));
        assert_eq!(
            b.status(t0 + Duration::from_secs(30)),
            Status::Open { secs_left: 30 }
        );
    }

    #[test]
    fn an_answer_ends_the_streak() {
        let t0 = Instant::now();
        let mut b = Breaker::default();
        fail(&mut b, t0, 4);
        assert!(b.admit(t0));
        // A malformed or refused answer counts as answered: Jev is there.
        assert_eq!(b.record(t0, Outcome::Answered), None);
        assert!(fail(&mut b, t0, 4).is_empty());
        assert!(!b.is_open());
    }

    #[test]
    fn after_a_minute_one_probe_goes_and_its_answer_closes_it() {
        let t0 = Instant::now();
        let mut b = Breaker::default();
        fail(&mut b, t0, 5);
        let t1 = t0 + Duration::from_secs(60);
        assert!(b.admit(t1), "the probe");
        assert!(!b.admit(t1), "only one probe at a time");
        assert_eq!(b.status(t1), Status::HalfOpen);
        assert_eq!(b.record(t1, Outcome::Answered), Some(Transition::Closed));
        assert!(b.admit(t1));
        assert_eq!(b.status(t1), Status::Closed { failures: 0 });
    }

    #[test]
    fn a_failed_probe_opens_it_for_another_minute() {
        let t0 = Instant::now();
        let mut b = Breaker::default();
        fail(&mut b, t0, 5);
        let t1 = t0 + Duration::from_secs(61);
        assert!(b.admit(t1));
        assert_eq!(
            b.record(t1, Outcome::Transient),
            Some(Transition::Reopened { for_secs: 60 })
        );
        assert!(!b.admit(t1 + Duration::from_secs(59)));
        assert!(b.admit(t1 + Duration::from_secs(60)));
    }

    #[test]
    fn a_probe_that_never_went_out_frees_the_slot_and_a_lost_one_times_out() {
        let t0 = Instant::now();
        let mut b = Breaker::default();
        fail(&mut b, t0, 5);
        let t1 = t0 + Duration::from_secs(60);
        assert!(b.admit(t1));
        assert_eq!(b.record(t1, Outcome::NotSent), None);
        assert!(b.admit(t1), "the next caller probes at once");
        // This probe is lost (no record at all): another may go after 10 s.
        assert!(!b.admit(t1 + Duration::from_secs(9)));
        assert!(b.admit(t1 + Duration::from_secs(10)));
    }

    #[test]
    fn stragglers_landing_while_open_change_nothing() {
        let t0 = Instant::now();
        let mut b = Breaker::default();
        fail(&mut b, t0, 5);
        assert_eq!(b.record(t0, Outcome::Answered), None);
        assert_eq!(b.record(t0, Outcome::Transient), None);
        assert!(!b.admit(t0 + Duration::from_secs(1)));
    }
}
