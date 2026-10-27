//! The static fan-out cue's verdict (`fan_out.cue`, theseus-z9mze.4; `turn/route_step/fan_out_cue.rs`).

use serde_json::{json, Value};
use theseus_protocol::LedgerKind;

use super::Fact;
use crate::trace::Trace;

/// The cue looked at a turn whose route class may fan out, and fired or did not. Both outcomes
/// are written, so a reader of the log sees the misses beside the hits.
pub struct FanOutCue<'a> {
    pub class: &'a str,
    /// The open-ended words the ask used, in the cue's list order.
    pub words: &'a [&'static str],
    pub fired: bool,
    /// The revision of the cue's word list that decided.
    pub words_rev: u32,
}

impl Fact for FanOutCue<'_> {
    const KIND: Option<LedgerKind> = Some(LedgerKind::FanOutCue);

    fn row(&self) -> Value {
        json!({"class": self.class, "words": self.words, "fired": self.fired, "words_rev": self.words_rev})
    }

    fn span(&self, trace: &mut Trace) {
        trace.mark(
            "fan_out_cue",
            "mark",
            json!({"class": self.class, "words": self.words.len(), "fired": self.fired}),
        );
    }
}
