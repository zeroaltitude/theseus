//! The progress check's steer or stop (`turn.progress`, theseus-2uy56;
//! `turn/progress_step.rs`): the numbers that fired it. The note's own words
//! ride in the frame beside it, a harness node.

use serde_json::{json, Value};
use theseus_protocol::LedgerKind;
use theseus_protocol::NarrativePart::Model;

use super::{Fact, Say};
use crate::trace::Trace;
use crate::turn::progress_step::Signals;

/// The check told the model, at loop `loop_index`'s end, to replan
/// (`steer`) or to stop and report (`stop`).
pub struct TurnProgress<'a> {
    pub decision: &'static str,
    pub loop_index: u32,
    pub signals: &'a Signals,
    /// The turn's time used and its deadline, when it has one.
    pub elapsed_ms: Option<u64>,
    pub deadline_ms: Option<u64>,
    /// This turn's spend, and its execution's limit, when known.
    pub spent_usd: Option<f64>,
    pub limit_usd: Option<f64>,
}

impl TurnProgress<'_> {
    fn data(&self) -> Value {
        let s = self.signals;
        json!({
            "decision": self.decision,
            "loop": self.loop_index,
            "time_share": s.time,
            "spend_share": s.spend,
            "since_pass": s.since_pass,
            "since_check": s.since_check,
            "failing": s.failing,
            "guarded": s.guarded,
            "elapsed_ms": self.elapsed_ms,
            "deadline_ms": self.deadline_ms,
            "spent_usd": self.spent_usd,
            "limit_usd": self.limit_usd,
        })
    }
}

impl Fact for TurnProgress<'_> {
    const KIND: Option<LedgerKind> = Some(LedgerKind::TurnProgress);

    fn row(&self) -> Value {
        self.data()
    }

    fn span(&self, trace: &mut Trace) {
        trace.mark("progress", "mark", self.data());
    }

    fn narrate(&self, say: &mut Say<'_>) {
        let what = match self.decision {
            "stop" => "to stop and report",
            _ => "that this is not a stop",
        };
        say.line(
            Model,
            format!(
                "No check had passed in {} loops, with {} of the turn's time and {} of its spend \
                 limit used, so the harness told the model {what}.",
                self.signals.since_check,
                share(self.signals.time),
                share(self.signals.spend),
            ),
        );
    }
}

/// A share as a percentage, or `no measure` without one.
fn share(s: Option<f64>) -> String {
    s.map_or_else(|| "no measure".into(), |s| format!("{:.0}%", s * 100.0))
}
