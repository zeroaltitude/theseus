//! The phases of a start, timed from process start (FAST, §2; P5b): those on
//! the path to answering the socket (config, store, kernel, core, socket) and
//! those after it (the secrets, and each consumer's wait for its own), so a
//! slow start names its cause in health and the Observatory the first time
//! it happens.

use std::sync::{Mutex, OnceLock};
use std::time::Instant;

use serde_json::Value;
use theseus_protocol::StartupPhase;

/// When this process's clean stop began (theseus-26r). Each of the stop's
/// phases after it, to the process's exit, is logged at info with the time
/// since (`stop_phase`), so a slow stop names the phase that held it in the
/// journal the first time it happens (theseus-vjn7).
static STOP_BEGAN: OnceLock<Instant> = OnceLock::new();

/// The clean stop begins now: its row is the first thing it writes. A second
/// call keeps the first time.
pub fn stop_began() {
    let _ = STOP_BEGAN.set(Instant::now());
    stop_phase("began");
}

/// Whether this process's clean stop has begun. A pass on the runtime's
/// blocking pool that waits out a busy machine (the adjacency projection's
/// warm build) stops waiting then: the runtime's end, the stop's last phase,
/// waits for every task on that pool.
pub fn stop_has_begun() -> bool {
    STOP_BEGAN.get().is_some()
}

/// A phase of the stop has ended: one line at info with the milliseconds
/// since the stop began. Install #9's stop took 11.87 s with these at debug,
/// so the journal could not say where (theseus-vjn7). Nothing before a stop
/// began.
pub fn stop_phase(phase: &str) {
    if let Some(t) = STOP_BEGAN.get() {
        tracing::info!(
            phase,
            ms = (t.elapsed().as_secs_f64() * 1000.0 * 100.0).round() / 100.0,
            "stop"
        );
    }
}

pub struct StartupLog {
    origin: Instant,
    phases: Mutex<Vec<StartupPhase>>,
}

impl Default for StartupLog {
    fn default() -> Self {
        Self::new(Instant::now())
    }
}

impl StartupLog {
    /// `origin` is the process's start, as near as `main` can take it.
    pub fn new(origin: Instant) -> Self {
        Self {
            origin,
            phases: Mutex::default(),
        }
    }

    pub fn origin(&self) -> Instant {
        self.origin
    }

    /// Microseconds from process start to `t`.
    pub fn us(&self, t: Instant) -> u64 {
        t.saturating_duration_since(self.origin).as_micros() as u64
    }

    /// A phase from `start` to now.
    pub fn record(&self, name: &str, background: bool, start: Instant, detail: Value) {
        let end = self.us(Instant::now());
        self.phases.lock().unwrap().push(StartupPhase {
            name: name.into(),
            background,
            start_us: self.us(start),
            end_us: Some(end),
            detail,
        });
    }

    /// A phase that has begun at `start`; `end` closes it. Health shows it
    /// open meanwhile.
    pub fn begin(&self, name: &str, background: bool, start: Instant) -> usize {
        let mut p = self.phases.lock().unwrap();
        p.push(StartupPhase {
            name: name.into(),
            background,
            start_us: self.us(start),
            end_us: None,
            detail: Value::Null,
        });
        p.len() - 1
    }

    pub fn end(&self, i: usize, detail: Value) {
        let end = self.us(Instant::now());
        if let Some(p) = self.phases.lock().unwrap().get_mut(i) {
            p.end_us = Some(end);
            p.detail = detail;
        }
    }

    /// Whether a phase of this name was recorded (a consumer's first wait is
    /// the one that counts).
    pub fn has(&self, name: &str) -> bool {
        self.phases.lock().unwrap().iter().any(|p| p.name == name)
    }

    /// Every phase, in the order each began.
    pub fn snapshot(&self) -> Vec<StartupPhase> {
        let mut v = self.phases.lock().unwrap().clone();
        v.sort_by_key(|p| p.start_us);
        v
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    #[test]
    fn phases_are_timed_from_the_origin_and_sorted_by_start() {
        let origin = Instant::now();
        let log = StartupLog::new(origin);
        let open = log.begin("secrets", true, origin + Duration::from_millis(2));
        log.record(
            "config",
            false,
            origin,
            serde_json::json!({"source": "file"}),
        );
        let snap = log.snapshot();
        assert_eq!(snap[0].name, "config");
        assert_eq!(snap[1].name, "secrets");
        assert_eq!(snap[1].start_us, 2000);
        assert!(snap[1].end_us.is_none(), "still open");
        log.end(open, serde_json::json!({"state": "ready"}));
        assert!(log.snapshot()[1].end_us.is_some());
        assert!(log.has("config") && !log.has("store"));
    }
}
