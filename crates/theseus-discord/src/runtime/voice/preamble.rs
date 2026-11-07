//! What a voice tool turn wrote before its tool calls, said when its loop
//! ends (theseus-xi1x), not with the turn's whole reply.
//!
//! A tool turn's loop writes a line ("I'll pull the daily figures"), runs its
//! calls, and the next loop goes on; the turn's `output` joins every loop's
//! text, so it was all said at the turn's end, seconds after the line was
//! written. The boundary here is the loop's end (`loop.ended`), after its
//! calls ran and before the next model call starts: the line is said while
//! that call runs. A loop's text is held from `model.answered` until then,
//! because only `loop.ended` says it stood: a retry (an answer cut at the
//! window, a refusal's fallback) leaves its text out of the turn, and a loop
//! that ended the turn (no calls, an approval waiting) is the reply itself.

use theseus_core::provider::WINDOW_EXCEEDED;
use theseus_protocol::{LoopEnded, ModelDelta};

use std::collections::BTreeMap;

use theseus_voice::TurnId;

/// The voice turn in flight's loop texts: held until their loop ends, and
/// what was said ahead of the reply.
#[derive(Default)]
pub(super) struct Early {
    /// The engine's id of the turn, which a line goes ahead of the reply of.
    pub(super) turn: Option<TurnId>,
    written: BTreeMap<u32, String>,
    said: Vec<String>,
}

impl Early {
    /// A voice turn begins: nothing written or said yet.
    pub(super) fn of(turn: TurnId) -> Self {
        Self {
            turn: Some(turn),
            ..Self::default()
        }
    }

    /// A loop's text, as `model.answered` sent it.
    pub(super) fn written(&mut self, d: &ModelDelta) {
        self.written.insert(d.loop_index, d.text.clone());
    }

    /// A loop ended: its text, to say now, when the loop made calls and the
    /// turn goes on; a retry's, which the turn leaves out, and a loop that
    /// ended the turn, whose text is its reply, are not.
    pub(super) fn ended(&mut self, l: &LoopEnded) -> Option<String> {
        let text = self.written.remove(&l.loop_index)?;
        let kept = !matches!(
            l.provider_stop_reason.as_deref(),
            Some("refusal") | Some(WINDOW_EXCEEDED)
        );
        if l.decision != "continue" || l.tool_calls == 0 || !kept || text.trim().is_empty() {
            return None;
        }
        self.said.push(text.clone());
        Some(text)
    }

    /// The turn's output without what was said ahead of it: each line out
    /// once, with the paragraph break that joined it to its neighbour.
    pub(super) fn rest(&self, output: &str) -> String {
        let mut out = output.to_string();
        for said in &self.said {
            let Some(at) = out.find(said.as_str()) else {
                continue;
            };
            let end = at + said.len();
            out.replace_range(at..end, "");
            if out[at..].starts_with("\n\n") {
                out.replace_range(at..at + 2, "");
            } else if out[..at].ends_with("\n\n") {
                out.replace_range(at - 2..at, "");
            }
        }
        match self.said.is_empty() {
            true => out,
            false => out.trim().to_string(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn delta(loop_index: u32, text: &str) -> ModelDelta {
        ModelDelta {
            turn_id: "t".into(),
            loop_index,
            text: text.into(),
        }
    }

    fn ended(loop_index: u32, stop: &str, calls: u32, decision: &str) -> LoopEnded {
        LoopEnded {
            turn_id: "t".into(),
            loop_index,
            provider_stop_reason: Some(stop.into()),
            tool_calls: calls,
            advancer: "until_no_tool_calls".into(),
            decision: decision.into(),
        }
    }

    #[test]
    fn a_loop_with_calls_that_goes_on_is_said_at_its_end() {
        let mut e = Early::default();
        e.written(&delta(0, "I'll check."));
        assert_eq!(
            e.ended(&ended(0, "tool_use", 2, "continue")).as_deref(),
            Some("I'll check.")
        );
    }

    #[test]
    fn a_loop_that_ended_the_turn_or_made_no_call_is_the_reply_not_a_preamble() {
        let mut e = Early::default();
        e.written(&delta(1, "Done."));
        assert_eq!(e.ended(&ended(1, "end_turn", 0, "end_turn:stop")), None);
        e.written(&delta(2, "Waiting on you."));
        assert_eq!(
            e.ended(&ended(2, "tool_use", 1, "end_turn:awaiting_confirm")),
            None
        );
        assert!(e.said.is_empty());
    }

    #[test]
    fn a_retried_loops_text_is_never_said() {
        let mut e = Early::default();
        for stop in ["refusal", WINDOW_EXCEEDED] {
            e.written(&delta(0, "Cut off."));
            assert_eq!(e.ended(&ended(0, stop, 1, "continue")), None, "{stop}");
        }
        assert!(e.said.is_empty());
    }

    #[test]
    fn a_loop_ends_once() {
        let mut e = Early::default();
        e.written(&delta(0, "First."));
        assert!(e.ended(&ended(0, "tool_use", 1, "continue")).is_some());
        assert!(e.ended(&ended(0, "tool_use", 1, "continue")).is_none());
    }

    #[test]
    fn the_reply_leaves_out_what_was_said_with_its_paragraph_break() {
        let mut e = Early::default();
        for (i, t) in [(0, "I'll check."), (1, "Still looking.")] {
            e.written(&delta(i, t));
            e.ended(&ended(i, "tool_use", 1, "continue")).unwrap();
        }
        assert_eq!(
            e.rest("I'll check.\n\nStill looking.\n\nUp four percent.\n\nAll good."),
            "Up four percent.\n\nAll good."
        );
        // A said line in the middle, or at the end: no stray break left.
        assert_eq!(e.rest("Up.\n\nI'll check.\n\nStill looking."), "Up.");
        // Nothing said, nothing changed: the output as it is.
        assert_eq!(Early::default().rest("  A.\n\nB. "), "  A.\n\nB. ");
    }

    #[test]
    fn a_line_the_output_does_not_hold_leaves_it_whole() {
        let mut e = Early::default();
        e.written(&delta(0, "I'll check."));
        e.ended(&ended(0, "tool_use", 1, "continue")).unwrap();
        assert_eq!(e.rest("Something else."), "Something else.");
    }
}
