//! A person's messages taken into the turn that was running when they came
//! (`turn.steered`, theseus-qu1lc; `turn/steer_step.rs`).

use serde_json::{json, Value};
use theseus_protocol::LedgerKind;
use theseus_protocol::NarrativePart::Model;

use super::{Fact, Say};
use crate::trace::Trace;

/// The loop boundary took `nodes` (one per message, in arrival order) into
/// the turn, before the loop `loop_index`'s model call. Their nodes ride in
/// the same frame as this row, and each message's queued request is answered
/// with the turn's result.
pub struct TurnSteered<'a> {
    pub nodes: &'a [String],
    pub loop_index: u32,
    /// The longest any waited in the queue, in milliseconds.
    pub waited_ms: u64,
}

impl Fact for TurnSteered<'_> {
    const KIND: Option<LedgerKind> = Some(LedgerKind::TurnSteered);

    fn row(&self) -> Value {
        json!({"nodes": self.nodes, "loop": self.loop_index, "waited_ms": self.waited_ms})
    }

    fn span(&self, trace: &mut Trace) {
        trace.mark(
            "steered",
            "mark",
            json!({"messages": self.nodes.len(), "loop": self.loop_index, "waited_ms": self.waited_ms}),
        );
    }

    fn narrate(&self, say: &mut Say<'_>) {
        say.line(
            Model,
            format!(
                "{} message(s) sent while this turn ran join it before loop {} ({} ms queued), \
                 and none runs as a turn of its own.",
                self.nodes.len(),
                self.loop_index,
                self.waited_ms
            ),
        );
    }
}
