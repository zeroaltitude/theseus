//! How a turn ended, as `theseus ask`'s exit code (theseus-n88g.2), so a
//! harness that runs it headless (`theseus --spawn ask`: a benchmark's
//! adapter, CI, a script) knows without parsing. `--json` says the same in
//! full: the turn's `stop_reason` and `awaiting_confirm`.
//!
//! The codes every command shares stay as they were: 0 ok, 1 an error from
//! the daemon or the provider (a failed turn is one), 2 usage, 3 cannot
//! connect, and `wait`'s 4 for a wait that timed out. A turn that returned
//! without the model ending it takes one of its own, from 5. Under
//! `--spawn`, a SIGINT or SIGTERM stops the turn as `/stop` does (9), and a
//! second one ends the run at once: 128 and the signal's number, as a shell
//! says it.

use theseus_protocol::TurnSubmitResult;

use crate::CallError;

/// The stop reason a turn cut at the model's context window reports
/// (`theseus_core::provider::WINDOW_EXCEEDED`, the API's own word).
const WINDOW_EXCEEDED: &str = "model_context_window_exceeded";

/// How one turn ended.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TurnEnd {
    /// The model ended its turn.
    Done,
    /// The turn failed: a provider's fault, a tool's or the daemon's.
    Failed,
    /// The session reached its spend limit: the turn waits on the operator's reset.
    SpendLimit,
    /// A call waits for the operator's approval, which a headless run cannot give.
    Waiting,
    /// The model refused.
    Refused,
    /// A limit ended the turn before the model did: the loop cap, the output
    /// limit, or the context window.
    Cut,
    /// An operator stopped it (`/stop`, `theseus stop`).
    Stopped,
}

impl TurnEnd {
    /// Every end, in the order of its code.
    pub const ALL: [TurnEnd; 7] = [
        Self::Done,
        Self::Failed,
        Self::SpendLimit,
        Self::Waiting,
        Self::Refused,
        Self::Cut,
        Self::Stopped,
    ];

    /// The process's exit code.
    pub const fn code(self) -> i32 {
        match self {
            Self::Done => 0,
            Self::Failed => 1,
            Self::SpendLimit => 5,
            Self::Waiting => 6,
            Self::Refused => 7,
            Self::Cut => 8,
            Self::Stopped => 9,
        }
    }

    /// A turn that returned, by its stop reason. One that names no end of its
    /// own (`no_tool_calls`, `end_turn`, `nothing_new`) is done.
    pub fn of(r: &TurnSubmitResult) -> Self {
        match r.stop_reason.as_str() {
            "budget" => Self::SpendLimit,
            "awaiting_confirm" => Self::Waiting,
            "refusal" => Self::Refused,
            "max_loops" | "max_tokens" | WINDOW_EXCEEDED => Self::Cut,
            "stopped" => Self::Stopped,
            _ if r.awaiting_confirm.is_some() => Self::Waiting,
            _ => Self::Done,
        }
    }

    /// What it says on stderr when the turn did not end done.
    pub fn words(self, stop_reason: &str) -> String {
        match self {
            Self::Done => "the model ended its turn".into(),
            Self::Failed => "the turn failed".into(),
            Self::SpendLimit => {
                "the session reached its spend limit: the turn waits for the operator to reset it"
                    .into()
            }
            Self::Waiting => "the turn is parked: a call waits for the operator's approval".into(),
            Self::Refused => "the model refused".into(),
            Self::Cut => format!("a limit ended the turn before the model did ({stop_reason})"),
            Self::Stopped => {
                "the turn was stopped: by an operator, or by a signal to a --spawn run".into()
            }
        }
    }
}

/// A turn that returned without the model ending it: `ask` returns this as
/// its error, so the connection still closes (a spawned daemon's clean stop)
/// before the process exits with its code.
#[derive(Debug)]
pub struct Ended {
    pub end: TurnEnd,
    pub stop_reason: String,
}

impl std::fmt::Display for Ended {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "{} (exit {})",
            self.end.words(&self.stop_reason),
            self.end.code()
        )
    }
}

impl std::error::Error for Ended {}

/// A second SIGINT or SIGTERM under `--spawn` ended the run before the turn
/// stopped: the turn was already told to stop, so the store's next open
/// resumes nothing, and the spawned daemon still gets its clean stop.
#[derive(Debug)]
pub struct Signalled {
    pub signal: i32,
}

impl Signalled {
    /// 128 and the signal's number, as a shell reports a process it ended.
    pub const fn code(&self) -> i32 {
        128 + self.signal
    }
}

impl std::fmt::Display for Signalled {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "a second {} ended the run before the turn stopped (exit {})",
            signal_name(self.signal),
            self.code()
        )
    }
}

impl std::error::Error for Signalled {}

/// `SIGINT` or `SIGTERM`, the two a `--spawn` run stops its turn on.
pub fn signal_name(signal: i32) -> &'static str {
    match signal {
        libc::SIGINT => "SIGINT",
        libc::SIGTERM => "SIGTERM",
        _ => "signal",
    }
}

/// The exit code for what a command returned as its error: a turn's end
/// (`Ended`), a second signal's (`Signalled`), 3 when the daemon could not be
/// reached or spawned, a failed turn's own (a stop's, else 1), and 1 for
/// anything else.
pub fn exit_code(e: &anyhow::Error) -> i32 {
    if let Some(ended) = e.downcast_ref::<Ended>() {
        return ended.end.code();
    }
    if let Some(s) = e.downcast_ref::<Signalled>() {
        return s.code();
    }
    let msg = format!("{e:#}");
    if msg.contains("connecting to theseusd") || msg.contains("spawning") {
        return 3;
    }
    match e.downcast_ref::<CallError>() {
        Some(c) if c.data.get("class").and_then(|v| v.as_str()) == Some("stopped") => {
            TurnEnd::Stopped.code()
        }
        _ => TurnEnd::Failed.code(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn turn(stop_reason: &str, awaiting: Option<&str>) -> TurnSubmitResult {
        serde_json::from_value(json!({
            "session_id": "ses_invented", "turn_id": "turn_invented", "loops": 1,
            "output": "", "stop_reason": stop_reason, "provider_stop_reason": null,
            "model": "claude-sonnet-5-5", "usage": {"input_tokens": 1, "output_tokens": 1},
            "elapsed_ms": 1, "awaiting_confirm": awaiting,
        }))
        .unwrap()
    }

    /// Every way a returned turn ends has its code, and the codes differ
    /// from each other and from the shared ones (2 usage, 3 cannot connect,
    /// 4 `wait`'s timeout).
    #[test]
    fn each_end_of_a_turn_has_its_own_exit_code() {
        for (stop, awaiting, end) in [
            ("no_tool_calls", None, TurnEnd::Done),
            ("end_turn", None, TurnEnd::Done),
            ("nothing_new", None, TurnEnd::Done),
            ("budget", Some("corr_budget"), TurnEnd::SpendLimit),
            ("awaiting_confirm", Some("corr_call"), TurnEnd::Waiting),
            ("refusal", None, TurnEnd::Refused),
            ("max_loops", None, TurnEnd::Cut),
            ("max_tokens", None, TurnEnd::Cut),
            (WINDOW_EXCEEDED, None, TurnEnd::Cut),
            ("stopped", None, TurnEnd::Stopped),
        ] {
            assert_eq!(TurnEnd::of(&turn(stop, awaiting)), end, "{stop}");
        }
        let codes: Vec<i32> = TurnEnd::ALL.iter().map(|e| e.code()).collect();
        assert_eq!(codes, [0, 1, 5, 6, 7, 8, 9]);
    }

    /// A command's error takes its code: a turn's end, a daemon that could
    /// not be reached, a failed turn, and an operator's stop.
    #[test]
    fn an_error_takes_the_code_of_what_it_was() {
        let ended = anyhow::Error::new(Ended {
            end: TurnEnd::SpendLimit,
            stop_reason: "budget".into(),
        });
        assert_eq!(exit_code(&ended), 5);
        assert!(ended.to_string().ends_with("(exit 5)"), "{ended}");
        let unreachable = anyhow::anyhow!("connecting to theseusd at /tmp/x.sock (is it running?)");
        assert_eq!(exit_code(&unreachable), 3);
        let failed = anyhow::Error::new(CallError {
            code: -32003,
            message: "the provider is overloaded".into(),
            data: json!({"class": "overloaded", "transient": true}),
        });
        assert_eq!(exit_code(&failed), 1);
        let stopped = anyhow::Error::new(CallError {
            code: -32003,
            message: "stopped by discord:dm".into(),
            data: json!({"class": "stopped"}),
        });
        assert_eq!(exit_code(&stopped), 9);
        assert_eq!(exit_code(&anyhow::anyhow!("bad line from server")), 1);
        // A second signal under --spawn: 128 and its number, as a shell says.
        for (signal, code, name) in [
            (libc::SIGINT, 130, "SIGINT"),
            (libc::SIGTERM, 143, "SIGTERM"),
        ] {
            let e = anyhow::Error::new(Signalled { signal });
            assert_eq!(exit_code(&e), code);
            assert!(
                e.to_string().starts_with(&format!("a second {name} ")),
                "{e}"
            );
        }
    }
}
