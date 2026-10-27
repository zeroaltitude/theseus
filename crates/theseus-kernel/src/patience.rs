//! How long a job may be quiet (theseus-x1zz8). A job's limit used to be a
//! clock: past it, the whole tree was stopped, a `cargo build` that printed
//! for an hour as much as one wedged at minute two. Now, where the wrapper can
//! see the job's output, the limit is a bound on **silence**: a job is stopped
//! when it has printed nothing for the limit. A job still printing when the
//! limit has run from its start is not stopped; the completion's `detail`
//! says so (`ran_past_limit_ms`). It also has a ceiling, [`CEILING_FACTOR`]
//! times the limit of wall time, so a job that prints a byte a minute cannot
//! run forever. Where the output is not seen (a watch's probe), the limit is
//! the clock it always was.

use std::time::{Duration, Instant};

use serde_json::{json, Value};

/// The wall time past which a job is stopped though it still prints, in
/// limits: a one-hour limit has a day's ceiling.
pub const CEILING_FACTOR: u32 = 24;

/// Why a job's limit stopped it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Cut {
    /// The limit ran from the start, and the output is not watched.
    Clock,
    /// It printed nothing for this long.
    Silent(Duration),
    /// It ran for the ceiling, printing still.
    Ceiling(Duration),
}

/// A job's limit, and what it has printed since it began.
pub struct Patience {
    t0: Instant,
    limit: Duration,
    ceiling: Duration,
    /// The bytes the job has printed so far, as a count that only grows.
    bytes: Option<Box<dyn Fn() -> u64 + Send>>,
    seen: u64,
    last_new: Instant,
    /// The wall time at which the limit passed with the job printing.
    noticed: Option<Duration>,
    cut: Option<Cut>,
}

impl Patience {
    /// A limit that is a clock: the output is not watched.
    pub fn clock(limit_ms: u64, t0: Instant) -> Self {
        Self::new(limit_ms, t0, None)
    }

    /// A limit on silence, by `bytes`, the count of what the job has printed.
    pub fn silence(limit_ms: u64, t0: Instant, bytes: impl Fn() -> u64 + Send + 'static) -> Self {
        Self::new(limit_ms, t0, Some(Box::new(bytes)))
    }

    fn new(limit_ms: u64, t0: Instant, bytes: Option<Box<dyn Fn() -> u64 + Send>>) -> Self {
        let limit = Duration::from_millis(limit_ms);
        Self {
            t0,
            limit,
            ceiling: limit.saturating_mul(CEILING_FACTOR),
            seen: bytes.as_ref().map_or(0, |b| b()),
            bytes,
            last_new: t0,
            noticed: None,
            cut: None,
        }
    }

    pub fn limit_ms(&self) -> u64 {
        self.limit.as_millis() as u64
    }

    pub fn ceiling_ms(&self) -> u64 {
        self.ceiling.as_millis() as u64
    }

    /// Whether the limit has stopped the job as of now, and the notice once
    /// the limit has passed with the job printing.
    pub fn check(&mut self) -> Option<Cut> {
        let elapsed = self.t0.elapsed();
        let Some(bytes) = &self.bytes else {
            return (elapsed >= self.limit).then_some(Cut::Clock);
        };
        let b = bytes();
        if b != self.seen {
            self.seen = b;
            self.last_new = Instant::now();
        }
        let quiet = self.last_new.elapsed();
        let cut = if quiet >= self.limit {
            Some(Cut::Silent(quiet))
        } else if elapsed >= self.ceiling {
            Some(Cut::Ceiling(elapsed))
        } else {
            if elapsed >= self.limit && self.noticed.is_none() {
                self.noticed = Some(elapsed);
            }
            None
        };
        self.cut = cut;
        cut
    }

    /// How long the wrapper may sleep before it must look again: to the
    /// quiet limit counted from the last byte, or the ceiling, or the limit's
    /// notice, whichever is first.
    pub fn sleep(&self) -> Duration {
        let elapsed = self.t0.elapsed();
        let left = if self.bytes.is_none() {
            self.limit.saturating_sub(elapsed)
        } else {
            let mut left = self
                .limit
                .saturating_sub(self.last_new.elapsed())
                .min(self.ceiling.saturating_sub(elapsed));
            if self.noticed.is_none() {
                left = left.min(self.limit.saturating_sub(elapsed));
            }
            left
        };
        left.max(Duration::from_millis(1))
    }

    /// What the completion says of the limit: how the job was cut, or that
    /// it ran past the limit while it printed.
    pub fn note(&self, detail: &mut Value) {
        // A job the silence stopped was not let run on: only the stop is said.
        let silent = matches!(self.cut, Some(Cut::Silent(_)));
        if let (Some(ms), false) = (self.noticed, silent) {
            detail["ran_past_limit_ms"] = json!({
                "limit_ms": self.limit_ms(),
                "noticed_at_ms": ms.as_millis() as u64,
                "ceiling_ms": self.ceiling_ms(),
            });
        }
        match self.cut {
            Some(Cut::Silent(q)) => detail["silent_ms"] = json!(q.as_millis() as u64),
            Some(Cut::Ceiling(_)) => detail["ceiling_hit"] = json!(true),
            _ => {}
        }
    }

    /// The completion's note for a cut.
    pub fn words(&self, cut: Cut, how: &str) -> String {
        match cut {
            Cut::Clock => format!("deadline {} ms exceeded; {how}", self.limit_ms()),
            Cut::Silent(q) => format!(
                "deadline {} ms exceeded: it printed nothing for {} ms; {how}",
                self.limit_ms(),
                q.as_millis()
            ),
            Cut::Ceiling(e) => format!(
                "ceiling {} ms exceeded ({CEILING_FACTOR} times its {} ms limit): it was still \
                 printing after {} ms; {how}",
                self.ceiling_ms(),
                self.limit_ms(),
                e.as_millis()
            ),
        }
    }
}
