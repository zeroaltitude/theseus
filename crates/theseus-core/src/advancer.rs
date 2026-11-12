//! The Advancer (spec §3.3a): after every loop, `continue` or `end_turn`.
//! It decides whether to loop again, never what a loop may do.

use serde::Serialize;

#[derive(Debug, Clone, Serialize)]
pub struct LoopOutcome {
    pub loop_index: u32,
    pub provider_stop_reason: Option<String>,
    pub tool_calls: u32,
    pub output_chars: usize,
    /// How often the commonest identical loop signature has appeared in this
    /// turn, this loop counted (`turn::repeat_step`): the number that bounds a
    /// turn re-planning the same work for ever. 0 for a loop that counts
    /// nothing — one with no tool calls, or one that only waits.
    pub repeats: u32,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case", tag = "decision", content = "reason")]
pub enum Decision {
    Continue,
    EndTurn(String),
}

impl Decision {
    pub fn label(&self) -> String {
        match self {
            Decision::Continue => "continue".into(),
            Decision::EndTurn(r) => format!("end_turn:{r}"),
        }
    }
}

pub trait Advancer: Send + Sync {
    fn name(&self) -> &'static str;
    fn decide(&self, outcome: &LoopOutcome) -> Decision;
}

/// The conventional agent loop with a cap: at `max_loops` the turn ends, or,
/// when the cap notifies (`notify`, theseus-usei), it goes on, and the turn
/// posts its notice (`turn/budget_step.rs`).
pub struct UntilNoToolCalls {
    pub max_loops: u32,
    pub notify: bool,
    /// How often one loop signature may repeat before the turn ends
    /// (`turn::repeat_step::MAX_REPEATS`); 0 turns the rule off. Unlike
    /// `max_loops` this arm is NOT conditional on `notify`: a turn re-planning
    /// the same work reaches nobody, so a cap that notified and went on would
    /// be no cap at all (`p5kln-p0-unbounded-hold`).
    pub max_repeats: u32,
}

impl Advancer for UntilNoToolCalls {
    fn name(&self) -> &'static str {
        "until_no_tool_calls"
    }
    fn decide(&self, o: &LoopOutcome) -> Decision {
        if o.tool_calls == 0 {
            Decision::EndTurn("no_tool_calls".into())
        } else if self.max_repeats > 0 && o.repeats >= self.max_repeats {
            // Before the `max_loops` arm and outside its `notify` guard: this
            // is the one cap on a loop that no notice and no judge can reach.
            Decision::EndTurn(crate::turn::repeat_step::REPEATING.into())
        } else if !self.notify && o.loop_index + 1 >= self.max_loops {
            Decision::EndTurn("max_loops".into())
        } else {
            Decision::Continue
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn outcome(i: u32, tools: u32) -> LoopOutcome {
        repeated(i, tools, 0)
    }

    fn repeated(i: u32, tools: u32, repeats: u32) -> LoopOutcome {
        LoopOutcome {
            loop_index: i,
            provider_stop_reason: None,
            tool_calls: tools,
            output_chars: 0,
            repeats,
        }
    }

    #[test]
    fn until_no_tool_calls_caps() {
        let a = UntilNoToolCalls {
            max_loops: 3,
            notify: false,
            max_repeats: 0,
        };
        assert_eq!(a.decide(&outcome(0, 1)), Decision::Continue);
        assert_eq!(
            a.decide(&outcome(2, 1)),
            Decision::EndTurn("max_loops".into())
        );
        assert_eq!(
            a.decide(&outcome(0, 0)),
            Decision::EndTurn("no_tool_calls".into())
        );
    }

    #[test]
    fn a_cap_that_notifies_goes_on_until_no_tool_calls() {
        let a = UntilNoToolCalls {
            max_loops: 3,
            notify: true,
            max_repeats: 0,
        };
        assert_eq!(a.decide(&outcome(2, 1)), Decision::Continue);
        assert_eq!(a.decide(&outcome(8, 1)), Decision::Continue);
        assert_eq!(
            a.decide(&outcome(9, 0)),
            Decision::EndTurn("no_tool_calls".into())
        );
    }

    /// `p5kln-p0-unbounded-hold`: the repeat arm ends the turn though the cap
    /// notifies, which is the shipped default and which makes the `max_loops`
    /// arm above unreachable.
    #[test]
    fn a_repeating_turn_ends_even_where_the_cap_only_notifies() {
        let a = UntilNoToolCalls {
            max_loops: 3,
            notify: true,
            max_repeats: crate::turn::repeat_step::MAX_REPEATS,
        };
        let max = crate::turn::repeat_step::MAX_REPEATS;
        // Under the cap it goes on, however many loops have run: `notify` is
        // true, so nothing else would have stopped it.
        assert_eq!(a.decide(&repeated(40, 1, max - 1)), Decision::Continue);
        assert_eq!(
            a.decide(&repeated(40, 1, max)),
            Decision::EndTurn("repeating".into()),
            "the repeat cap must not be conditional on `notify`"
        );
        // And past it, which a signature counted after the cap can reach.
        assert_eq!(
            a.decide(&repeated(41, 1, max + 10)),
            Decision::EndTurn("repeating".into())
        );
        // It is the same answer with `notify` false, and it comes BEFORE the
        // loop cap: a repeat is the more specific reason.
        let b = UntilNoToolCalls {
            max_loops: 3,
            notify: false,
            max_repeats: max,
        };
        assert_eq!(
            b.decide(&repeated(9, 1, max)),
            Decision::EndTurn("repeating".into())
        );
        // No tool calls still wins: that loop ends the turn by itself.
        assert_eq!(
            b.decide(&repeated(9, 0, max)),
            Decision::EndTurn("no_tool_calls".into())
        );
        // `max_repeats: 0` is the rule off, and the old behaviour exactly.
        let off = UntilNoToolCalls {
            max_loops: 3,
            notify: true,
            max_repeats: 0,
        };
        assert_eq!(off.decide(&repeated(40, 1, 9_999)), Decision::Continue);
        assert_eq!(
            Decision::EndTurn("repeating".into()).label(),
            "end_turn:repeating"
        );
    }
}
