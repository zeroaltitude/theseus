//! Each measured turn's own line (theseus-w7dk): its wall time, the daemon's,
//! its frames, and its slowest frame, so a lone slow run in ten says where
//! its time went. With ten runs the p95 is the maximum and the summaries
//! show one slow turn without saying why: a frame whose sync stalled, or
//! time outside the frames (the scheduler, a first-of-kind cost in the
//! daemon).
//!
//! The slowest frame is the daemon's: the store's writer times each batch it
//! writes, syncs, and indexes (`theseus_store::FrameTime`), and the turn puts
//! the slowest answered since its input arrived on its trace
//! (`attrs.slowest_frame`: the frame's first position and its microseconds).
//! The bench finds that frame among the ones it read from the WAL and names
//! it by its records. The turn's last frame, which carries the trace, is
//! written after it and so is not among them.

use serde::Serialize;
use serde_json::Value;

use crate::walcount::Frame;

/// A run is flagged when its wall time is over this many times the kind's p50.
pub const OUTLIER_TIMES: f64 = 2.0;

/// A turn's slowest frame.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Slowest {
    /// The writer's time over the frame, in ms.
    pub ms: f64,
    /// The frame's records, as the frame counts print them; `(not found
    /// among this turn's frames)` when the WAL read holds no frame at the
    /// position the daemon named (a neighbour's frame, in a daemon that
    /// ran another turn meanwhile).
    pub frame: String,
}

/// One measured turn.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Run {
    /// Its place among its kind's runs, from 1.
    pub n: usize,
    /// `turn.submit` sent to its answer, by the bench's clock, in ms.
    pub wall_ms: f64,
    /// The daemon's own time (`elapsed_ms`), in ms.
    pub daemon_ms: f64,
    /// Frames from before the turn to a quiet stretch after it.
    pub frames: u64,
    /// The slowest frame the daemon timed; none from a daemon that does not
    /// say (a build before theseus-w7dk).
    pub slowest: Option<Slowest>,
}

impl Run {
    /// A run from its answer: the turn's trace names its slowest frame, which
    /// is found among the frames the WAL held.
    pub fn of(n: usize, answer: &Value, wall_ms: f64, frames: &[Frame]) -> Self {
        let slow = &answer["trace"]["attrs"]["slowest_frame"];
        let slowest = slow["first"].as_u64().map(|first| Slowest {
            ms: slow["us"].as_u64().unwrap_or(0) as f64 / 1000.0,
            frame: frames.iter().find(|f| f.first == first).map_or_else(
                || "(not found among this turn's frames)".into(),
                Frame::label,
            ),
        });
        Self {
            n,
            wall_ms,
            daemon_ms: answer["elapsed_ms"].as_f64().unwrap_or(0.0),
            frames: frames.len() as u64,
            slowest,
        }
    }

    /// The run's line; `p50` is its kind's, for the flag on an outlier.
    pub fn line(&self, p50: f64) -> String {
        let slowest = self.slowest.as_ref().map_or_else(
            || "slowest frame not named".to_string(),
            |s| format!("slowest frame {:.1} ms {}", s.ms, s.frame),
        );
        let flag = if self.wall_ms > OUTLIER_TIMES * p50 {
            format!("  <- over {OUTLIER_TIMES}x the p50")
        } else {
            String::new()
        };
        format!(
            "    {:>3}  wall {:>7.1} ms  daemon {:>7.1} ms  {} frames  {slowest}{flag}",
            self.n, self.wall_ms, self.daemon_ms, self.frames
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn frame(first: u64, records: &[&str]) -> Frame {
        Frame {
            first,
            records: records.iter().map(|r| r.to_string()).collect(),
        }
    }

    fn answer(first: u64, us: u64) -> Value {
        json!({"elapsed_ms": 41.5, "trace": {"attrs": {"frames": 3,
            "slowest_frame": {"first": first, "us": us}}}})
    }

    /// The frame the daemon names is the one whose records are printed, and
    /// not the first frame, nor the last.
    #[test]
    fn the_slowest_frame_is_the_one_the_trace_names() {
        let frames = [
            frame(10, &["ledger:turn.started", "node"]),
            frame(12, &["ledger:call.completed"]),
            frame(13, &["ledger:turn.ended"]),
        ];
        let run = Run::of(3, &answer(12, 9_300), 55.0, &frames);
        let s = run.slowest.as_ref().unwrap();
        assert_eq!(s.frame, "[ledger:call.completed]");
        assert_eq!(s.ms, 9.3);
        assert_eq!((run.n, run.frames, run.daemon_ms), (3, 3, 41.5));
    }

    #[test]
    fn a_frame_the_wal_does_not_hold_is_said_so() {
        let run = Run::of(1, &answer(99, 1_000), 5.0, &[frame(10, &["node"])]);
        assert!(run.slowest.unwrap().frame.contains("not found"));
    }

    #[test]
    fn a_daemon_that_does_not_name_one_leaves_it_unnamed() {
        let run = Run::of(1, &json!({"elapsed_ms": 2.0}), 5.0, &[]);
        assert!(run.slowest.is_none());
        assert!(run.line(5.0).contains("slowest frame not named"));
    }

    #[test]
    fn a_runs_line_has_its_numbers_and_flags_an_outlier_only() {
        let run = Run::of(7, &answer(10, 12_345), 90.0, &[frame(10, &["node"])]);
        let line = run.line(40.0);
        assert_eq!(
            line,
            "      7  wall    90.0 ms  daemon    41.5 ms  1 frames  slowest frame 12.3 ms [node]  \
             <- over 2x the p50"
        );
        assert!(!run.line(45.0).contains("<-"), "90 is not over twice 45");
        let v = serde_json::to_value(&run).unwrap();
        assert_eq!(v["slowest"]["frame"], "[node]");
    }
}
