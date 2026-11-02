//! The static fan-out cue's verdict (`fan_out.cue`, theseus-z9mze.4; `turn/route_step/fan_out_cue.rs`), and the two
//! rows of a turn's wait on its tasks (`task.wait.started` and `task.wait.ended`, theseus-z9mze.6;
//! `task_wait.rs`), which Ops reads for "waiting on n tasks".

use serde_json::{json, Value};
use theseus_protocol::LedgerKind;

use super::Fact;
use crate::trace::Trace;

/// The cue looked at a turn whose verdict carries Jev's `split` answer, and fired or did not.
/// Both outcomes are written, so a reader of the log sees the misses beside the hits.
pub struct FanOutCue<'a> {
    /// The option Jev chose (`splits_by_area`, `one_area`, `unclear`).
    pub split: &'a str,
    pub confidence: f64,
    /// Its band reached the question's confirm bar.
    pub sure: bool,
    pub fired: bool,
}

impl Fact for FanOutCue<'_> {
    const KIND: Option<LedgerKind> = Some(LedgerKind::FanOutCue);

    fn row(&self) -> Value {
        json!({"split": self.split, "confidence": self.confidence, "sure": self.sure, "fired": self.fired})
    }

    fn span(&self, trace: &mut Trace) {
        trace.mark(
            "fan_out_cue",
            "mark",
            json!({"split": self.split, "sure": self.sure, "fired": self.fired}),
        );
    }
}

/// A turn began to wait on tasks it started (`task.wait`, or the wait `task.fan_out` makes). `fan_out` is the
/// call's id when the tasks are its helpers.
pub struct TaskWaitStarted<'a> {
    pub tasks: u32,
    pub fan_out: Option<&'a str>,
    pub timeout_s: u64,
}

impl Fact for TaskWaitStarted<'_> {
    const KIND: Option<LedgerKind> = Some(LedgerKind::TaskWaitStarted);

    fn row(&self) -> Value {
        json!({"tasks": self.tasks, "fan_out": self.fan_out, "timeout_s": self.timeout_s})
    }
}

/// The wait ended: how its tasks stood, and how it ended. `budget` is counted apart from `failed` (a helper that
/// reached its carve did its job within its money), and `cut` says the fan-in bound shortened a report.
pub struct TaskWaitEnded<'a> {
    pub tasks: u32,
    /// Reports taken into the turn by this wait.
    pub delivered: u32,
    pub running: u32,
    pub failed: u32,
    pub cancelled: u32,
    pub budget: u32,
    pub waited_ms: u64,
    pub cut: bool,
    pub fan_out: Option<&'a str>,
}

impl Fact for TaskWaitEnded<'_> {
    const KIND: Option<LedgerKind> = Some(LedgerKind::TaskWaitEnded);

    fn row(&self) -> Value {
        json!({"tasks": self.tasks, "delivered": self.delivered, "running": self.running,
            "failed": self.failed, "cancelled": self.cancelled, "budget": self.budget,
            "waited_ms": self.waited_ms, "cut": self.cut, "fan_out": self.fan_out})
    }
}
