//! The Advancer (spec §3.3a): after every loop, `continue` or `end_turn`.
//! It decides whether to loop again, never what a loop may do.

use serde::Serialize;

#[derive(Debug, Clone, Serialize)]
pub struct LoopOutcome {
    pub loop_index: u32,
    pub provider_stop_reason: Option<String>,
    pub tool_calls: u32,
    pub output_chars: usize,
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

/// The first policy and the permanent baseline: one loop, then the turn ends.
pub struct StopAfterOneLoop;

impl Advancer for StopAfterOneLoop {
    fn name(&self) -> &'static str {
        "stop_after_one_loop"
    }
    fn decide(&self, _outcome: &LoopOutcome) -> Decision {
        Decision::EndTurn("stop_after_one_loop".into())
    }
}

/// The conventional agent loop with a cap: at `max_loops` the turn ends, or,
/// when the cap notifies (`notify`, theseus-usei), it goes on, and the turn
/// posts its notice (`turn/budget_step.rs`).
pub struct UntilNoToolCalls {
    pub max_loops: u32,
    pub notify: bool,
}

impl Advancer for UntilNoToolCalls {
    fn name(&self) -> &'static str {
        "until_no_tool_calls"
    }
    fn decide(&self, o: &LoopOutcome) -> Decision {
        if o.tool_calls == 0 {
            Decision::EndTurn("no_tool_calls".into())
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
        LoopOutcome {
            loop_index: i,
            provider_stop_reason: None,
            tool_calls: tools,
            output_chars: 0,
        }
    }

    #[test]
    fn stop_after_one_always_ends() {
        let a = StopAfterOneLoop;
        assert!(matches!(a.decide(&outcome(0, 5)), Decision::EndTurn(_)));
    }

    #[test]
    fn until_no_tool_calls_caps() {
        let a = UntilNoToolCalls {
            max_loops: 3,
            notify: false,
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
        };
        assert_eq!(a.decide(&outcome(2, 1)), Decision::Continue);
        assert_eq!(a.decide(&outcome(8, 1)), Decision::Continue);
        assert_eq!(
            a.decide(&outcome(9, 0)),
            Decision::EndTurn("no_tool_calls".into())
        );
    }
}
